"""Offline verification of the exact upstream public roster v1 contract."""

import copy
import hashlib
import http.client
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = Path(__file__).parent / "fixtures/public-roster"
sys.path.insert(0, str(ROOT / "scripts"))
import roster_input


def rehash(envelope):
    payload = {key: envelope[key] for key in ("schema_version", "members", "og_assignments")}
    canonical = json.dumps(payload, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    envelope["content_hash"] = "sha256:" + hashlib.sha256(canonical).hexdigest()
    return json.dumps(envelope, ensure_ascii=False)


class RosterContractTests(unittest.TestCase):
    def setUp(self):
        self.text = (FIXTURES / "fixtures/roster-v1.json").read_text()
        self.envelope = json.loads(self.text)

    def assert_invalid(self, envelope, message):
        with self.assertRaisesRegex(ValueError, message):
            roster_input.parse_roster_json(rehash(envelope))

    def test_upstream_fixture_hash_and_stable_reference_adaptation(self):
        envelope, members = roster_input.parse_roster_json(self.text)
        self.assertEqual(envelope, self.envelope)
        self.assertEqual([member["number"] for member in members], [1, 7, 12, 21, 40, 88])
        self.assertIs(members[1]["sponsor_member"], members[0])
        self.assertIs(members[3]["sponsor_member"], members[1])
        self.assertIs(members[5]["sponsor_member"], members[3])
        self.assertIsNone(members[4]["sponsor_member"])
        self.assertEqual(members[1]["join_date"], "8/31/2026")
        self.assertEqual(members[2]["name"], "Zoë Example")
        self.assertEqual(members[2]["join_date"], "")
        self.assertEqual(members[3]["join_date"], "Aug 21, 2026")
        self.assertEqual(members[5]["qth"], "New York")
        self.assertEqual(members[5]["og_regions"], [{"region_key": "US:VT", "region_label": "VT"}])
        self.assertEqual(members[0]["og_regions"], [])

    def test_empty_upstream_fixture_fails_closed(self):
        with self.assertRaisesRegex(ValueError, "no active members"):
            roster_input.parse_roster_json((FIXTURES / "fixtures/empty-v1.json").read_text())

    def test_reviewed_values_are_preserved_without_identity_normalization(self):
        row = self.envelope["members"][1]
        row.update(callsign="n0Changed/P", name="  Zoë <Reviewed> & Name  ",
                   qth="  New York  ", qso_date="  August 31, 2026  ")
        _, members = roster_input.parse_roster_json(rehash(self.envelope))
        self.assertEqual(members[1]["callsign"], row["callsign"])
        self.assertEqual(members[1]["name"], row["name"])
        self.assertEqual(members[1]["qth"], row["qth"])
        self.assertEqual(members[1]["join_date"], row["qso_date"])
        self.assertEqual(members[3]["sponsor_member"]["callsign"], "n0Changed/P")

    def test_generated_at_is_utc_and_excluded_from_content_hash(self):
        for timestamp in ("2026-09-30T01:02:03Z", "2026-09-30T01:02:03.125+00:00"):
            with self.subTest(timestamp=timestamp):
                self.envelope["generated_at"] = timestamp
                roster_input.parse_roster_json(json.dumps(self.envelope))
        for timestamp in ("2026-09-30", "2026-09-30T01:02:03", "2026-09-30T01:02:03-04:00",
                          "2026-02-30T01:02:03Z", "2026-09-30T25:02:03Z", 123, None):
            with self.subTest(timestamp=timestamp):
                invalid = copy.deepcopy(self.envelope)
                invalid["generated_at"] = timestamp
                self.assert_invalid(invalid, "ISO UTC")

    def test_schema_rejects_missing_private_and_unsupported_fields(self):
        for target in ("envelope", "member", "og"):
            for operation in ("missing", "extra"):
                with self.subTest(target=target, operation=operation):
                    invalid = copy.deepcopy(self.envelope)
                    value = invalid if target == "envelope" else invalid["members"][0] if target == "member" else invalid["og_assignments"][0]
                    if operation == "extra":
                        value["private_notes"] = "should never be public"
                    else:
                        del value["generated_at" if target == "envelope" else "name" if target == "member" else "region_label"]
                    self.assert_invalid(invalid, "missing or additional")
        self.envelope["schema_version"] = "2"
        self.assert_invalid(self.envelope, "Unsupported")

    def test_members_require_unique_sorted_positive_safe_integer_numbers(self):
        for number in (True, 1.0, 0, -1, "1", roster_input.MAX_BKG_NUMBER + 1):
            with self.subTest(number=number):
                invalid = copy.deepcopy(self.envelope)
                invalid["members"][0]["bkg_number"] = number
                self.assert_invalid(invalid, "bkg_number")
        for operation in ("duplicate", "unsorted"):
            invalid = copy.deepcopy(self.envelope)
            if operation == "duplicate":
                invalid["members"][1]["bkg_number"] = 1
            else:
                invalid["members"][0], invalid["members"][1] = invalid["members"][1], invalid["members"][0]
            self.assert_invalid(invalid, "unique ascending")

    def test_callsign_syntax_and_case_insensitive_uniqueness(self):
        for callsign in ("", "N0 BAD", "../N0BAD", "/N0BAD", "N0BAD/", "N0BAD//P", "N0BÅD", "N0BAD<script>"):
            with self.subTest(callsign=callsign):
                invalid = copy.deepcopy(self.envelope)
                invalid["members"][0]["callsign"] = callsign
                self.assert_invalid(invalid, "callsign")
        self.envelope["members"][1]["callsign"] = "n0syn1"
        self.assert_invalid(self.envelope, "Duplicate member callsign")

    def test_public_strings_reject_wrong_types_controls_and_surrogates(self):
        for key in ("name", "qth", "qso_date", "callsign"):
            for value in (None, 17, True, [], "line\nbreak", "tab\tbreak", "hidden\x85control"):
                with self.subTest(key=key, value=value):
                    invalid = copy.deepcopy(self.envelope)
                    invalid["members"][0][key] = value
                    self.assert_invalid(invalid, "string|control")
        text = self.text.replace('"Synthetic First"', '"\\ud800"')
        with self.assertRaisesRegex(ValueError, "invalid Unicode"):
            roster_input.parse_roster_json(text)

    def test_sponsor_numbers_resolve_active_members_and_reject_self_or_cycles(self):
        for sponsor, message in ((True, "sponsor_bkg_number"), (999, "missing active"), (1, "sponsor itself")):
            invalid = copy.deepcopy(self.envelope)
            invalid["members"][0]["sponsor_bkg_number"] = sponsor
            self.assert_invalid(invalid, message)
        self.envelope["members"][0]["sponsor_bkg_number"] = 21
        self.assert_invalid(self.envelope, "cycle")

    def test_members_txt_fields_reject_unicode_line_separators_without_normalizing_qth(self):
        for separator in ("\u2028", "\u2029"):
            for key in ("name", "region_label"):
                with self.subTest(key=key, separator=separator):
                    invalid = copy.deepcopy(self.envelope)
                    row = invalid["members"][0] if key == "name" else invalid["og_assignments"][0]
                    row[key] = f"Stored{separator}Text"
                    self.assert_invalid(invalid, "line separators")
            valid = copy.deepcopy(self.envelope)
            valid["members"][0]["qth"] = f"Stored{separator}Location"
            _, members = roster_input.parse_roster_json(rehash(valid))
            self.assertEqual(members[0]["qth"], f"Stored{separator}Location")

    def test_og_assignments_keep_sorted_stored_regions_and_active_holders(self):
        changes = (("region_key", "", "must not be empty"), ("region_label", "", "must not be empty"),
                   ("bkg_number", True, "bkg_number"), ("bkg_number", 999, "missing active"))
        for key, value, message in changes:
            with self.subTest(key=key, value=value):
                invalid = copy.deepcopy(self.envelope)
                invalid["og_assignments"][0][key] = value
                self.assert_invalid(invalid, message)
        for operation in ("duplicate", "unsorted"):
            invalid = copy.deepcopy(self.envelope)
            if operation == "duplicate":
                invalid["og_assignments"][1]["region_key"] = invalid["og_assignments"][0]["region_key"]
            else:
                invalid["og_assignments"].reverse()
            self.assert_invalid(invalid, "unique ascending region_key")

    def test_hash_corruption_rejected_without_returning_members(self):
        invalid = copy.deepcopy(self.envelope)
        invalid["members"][0]["name"] = "A different reviewed identity"
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            roster_input.parse_roster_json(json.dumps(invalid))
        for value in ("sha256:" + "a" * 63, "sha256:" + "A" * 64, "md5:" + "a" * 64, None):
            invalid["content_hash"] = value
            with self.assertRaisesRegex(ValueError, "content_hash"):
                roster_input.parse_roster_json(json.dumps(invalid))

    def test_json_parser_rejects_duplicates_nonfinite_and_bad_shapes(self):
        invalid_texts = ["", "[]", "null", "{}", "{", self.text.replace('"schema_version": "1"', '"schema_version": "1", "schema_version": "1"'),
                         self.text.replace('"callsign": "N0SYN1"', '"callsign": "N0SYN1", "callsign": "N0SYN1"'),
                         self.text.replace('"bkg_number": 1,', '"bkg_number": NaN,', 1)]
        for text in invalid_texts:
            with self.subTest(text=text[:80]):
                with self.assertRaises(ValueError):
                    roster_input.parse_roster_json(text)
        for key in ("members", "og_assignments"):
            invalid = copy.deepcopy(self.envelope)
            invalid[key] = {}
            with self.assertRaisesRegex(ValueError, "array"):
                roster_input.parse_roster_json(json.dumps(invalid))


class FakeResponse:
    def __init__(self, body, status=200):
        self.body = body
        self.status = status
        self.read_limit = None

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        pass

    def getcode(self):
        return self.status

    def read(self, limit):
        self.read_limit = limit
        return self.body[:limit]


class RosterReadTests(unittest.TestCase):
    def setUp(self):
        self.body = (FIXTURES / "fixtures/roster-v1.json").read_bytes()
        self.environment = {
            "ROSTER_EXPORT_URL": "https://roster.fixture.invalid/api/roster/export",
            "CF_ACCESS_CLIENT_ID": "fixture-client.access",
            "CF_ACCESS_CLIENT_SECRET": "fixture-secret-do-not-log",
        }

    def test_local_file_never_reads_environment_or_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "roster.json"
            target.write_bytes(self.body)
            with patch.object(roster_input.urllib.request, "build_opener", side_effect=AssertionError("No network")):
                self.assertEqual(roster_input.read_roster_json(target, environ={}), self.body.decode())

    def test_explicit_endpoint_auth_headers_and_no_redirect_handler(self):
        response = FakeResponse(self.body)
        with patch.object(roster_input.urllib.request, "build_opener") as build_opener:
            build_opener.return_value.open.return_value = response
            self.assertEqual(roster_input.read_roster_json(environ=self.environment), self.body.decode())
        handler = build_opener.call_args.args[0]
        self.assertIsInstance(handler, roster_input._NoRedirects)
        self.assertIsNone(handler.redirect_request(None, None, 302, "Found", {}, "https://other.fixture.invalid"))
        request = build_opener.return_value.open.call_args.args[0]
        self.assertEqual(request.get_header("Cf-access-client-id"), self.environment["CF_ACCESS_CLIENT_ID"])
        self.assertEqual(request.get_header("Cf-access-client-secret"), self.environment["CF_ACCESS_CLIENT_SECRET"])
        self.assertEqual(request.get_header("Accept"), "application/json")
        self.assertEqual(build_opener.return_value.open.call_args.kwargs["timeout"], 30)
        self.assertEqual(response.read_limit, roster_input.MAX_ROSTER_BYTES + 1)

    def test_anonymous_https_request_has_no_access_headers(self):
        environment = {"ROSTER_EXPORT_URL": self.environment["ROSTER_EXPORT_URL"]}
        with patch.object(roster_input.urllib.request, "build_opener") as opener:
            opener.return_value.open.return_value = FakeResponse(self.body)
            self.assertEqual(roster_input.read_roster_json(environ=environment), self.body.decode())
            self.assertEqual(opener.return_value.open.call_count, 1)
            request = opener.return_value.open.call_args.args[0]
            self.assertEqual(request.get_method(), "GET")
            self.assertFalse(any("access" in key.lower() for key in request.headers))

    def test_http_auth_redirect_and_network_failures_are_sanitized(self):
        endpoint = self.environment["ROSTER_EXPORT_URL"]
        secret = self.environment["CF_ACCESS_CLIENT_SECRET"]
        errors = [urllib.error.HTTPError(endpoint, code, secret, {}, None) for code in (302, 401, 403, 503)]
        errors += [urllib.error.URLError(secret), OSError(secret), http.client.BadStatusLine(secret),
                   http.client.HTTPException(secret), UnicodeError(secret)]
        for error in errors:
            with self.subTest(error=type(error).__name__):
                with patch.object(roster_input.urllib.request, "build_opener") as opener:
                    opener.return_value.open.side_effect = error
                    with self.assertRaises(OSError) as raised:
                        roster_input.read_roster_json(environ=self.environment)
                    self.assertNotIn(secret, str(raised.exception))
                    self.assertNotIn(endpoint, str(raised.exception))
                    self.assertEqual(opener.return_value.open.call_count, 1)

    def test_missing_or_malformed_auth_fails_before_request(self):
        for key in self.environment:
            for value in (None, "", " ", "unsafe\r\nheader"):
                with self.subTest(key=key, value=value):
                    environment = dict(self.environment)
                    if value is None:
                        del environment[key]
                    else:
                        environment[key] = value
                    with patch.object(roster_input.urllib.request, "build_opener") as opener:
                        with self.assertRaises(ValueError):
                            roster_input.read_roster_json(environ=environment)
                        opener.assert_not_called()

    def test_https_and_one_explicit_source_required(self):
        urls = ["http://roster.fixture.invalid/export", "file:///roster.json", "https://user:secret@roster.fixture.invalid/export",
                "https://roster.fixture.invalid/export#fragment", "https://roster.fixture.invalid:0/export",
                "https://roster.fixture.invalid:bad/export", "https://roster.fixture.invalid/ex port", "https:///export",
                "https://roster.fixture.invalid/export?token=never-send"]
        for url in urls:
            with self.subTest(url=url):
                with patch.object(roster_input.urllib.request, "build_opener") as opener:
                    with self.assertRaisesRegex(ValueError, "HTTPS"):
                        roster_input.read_roster_json(url=url, environ=self.environment)
                    opener.assert_not_called()
        with self.assertRaisesRegex(ValueError, "Select one"):
            roster_input.read_roster_json("roster.json", url=self.environment["ROSTER_EXPORT_URL"])
        with self.assertRaisesRegex(ValueError, "ROSTER_EXPORT_URL"):
            roster_input.read_roster_json(environ={})

    def test_local_and_remote_size_limits_utf8_and_status(self):
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "roster.json"
            for body, message in ((b"x" * 101, "size limit"), (b"\xff", "not UTF-8")):
                target.write_bytes(body)
                with patch.object(roster_input, "MAX_ROSTER_BYTES", 100):
                    with self.assertRaisesRegex(ValueError, message):
                        roster_input.read_roster_json(target)
                    with patch.object(roster_input.urllib.request, "build_opener") as opener:
                        opener.return_value.open.return_value = FakeResponse(body)
                        with self.assertRaisesRegex(ValueError, message):
                            roster_input.read_roster_json(environ=self.environment)
        with patch.object(roster_input.urllib.request, "build_opener") as opener:
            opener.return_value.open.return_value = FakeResponse(self.body, status=204)
            with self.assertRaisesRegex(ValueError, "HTTP 200"):
                roster_input.read_roster_json(environ=self.environment)
        with patch.object(roster_input, "MAX_ROSTER_BYTES", 100):
            with self.assertRaisesRegex(ValueError, "size limit"):
                roster_input.parse_roster_json(self.body.decode())


if __name__ == "__main__":
    unittest.main()
