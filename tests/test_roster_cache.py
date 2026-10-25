"""Conditional GET reuses only an endpoint-bound, fully validated snapshot."""

from email.message import Message
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

from test_roster_input import FIXTURES, FakeResponse, roster_input


class RosterCacheTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.cache = Path(temporary.name) / "cache"
        self.url = "https://public.fixture.invalid/api/roster/export/v2"
        self.text = (FIXTURES / "fixtures/roster-v2.json").read_text()
        self.etag = f'W/"{json.loads(self.text)["content_hash"]}"'

    def response(self, text=None, etag=None):
        response = FakeResponse((text or self.text).encode())
        response.headers = Message()
        response.headers["ETag"] = etag or self.etag
        return response

    def read(self, response, *, url=None, version="2", exclude_invalid_callsigns=False):
        with patch.object(roster_input.urllib.request, "build_opener") as opener:
            if isinstance(response, Exception):
                opener.return_value.open.side_effect = response
            else:
                opener.return_value.open.return_value = response
            text = roster_input.read_roster_json(url=url or self.url, environ={},
                                                 expected_version=version, cache_dir=self.cache,
                                                 exclude_invalid_callsigns=exclude_invalid_callsigns)
            self.request = opener.return_value.open.call_args.args[0]
            return text

    def not_modified(self, etag=None):
        headers = Message()
        headers["ETag"] = etag or self.etag
        return urllib.error.HTTPError(self.url, 304, "Not Modified", headers, None)

    def test_200_then_304_preserves_exact_validated_snapshot(self):
        self.assertEqual(self.read(self.response()), self.text)
        self.assertIsNone(self.request.get_header("If-none-match"))
        before = (self.cache / "v2.json").read_bytes()
        self.assertEqual(self.read(self.not_modified()), self.text)
        self.assertEqual(self.request.get_header("If-none-match"), self.etag)
        self.assertEqual((self.cache / "v2.json").read_bytes(), before)

    def test_projection_cache_keeps_source_bytes_and_source_etag_through_304(self):
        original = json.loads(self.text)
        original["members"][0]["callsign"] = "KCØDWZ"
        original["content_hash"] = roster_input.roster_content_hash(original)
        self.text = json.dumps(original, ensure_ascii=False)
        self.etag = f'W/"{original["content_hash"]}"'
        self.assertEqual(self.read(self.response(), exclude_invalid_callsigns=True), self.text)
        projected, _ = roster_input.parse_roster_json(self.text, expected_version="2", exclude_invalid_callsigns=True)
        self.assertNotEqual(projected["content_hash"], original["content_hash"])
        self.assertEqual(self.read(self.not_modified(), exclude_invalid_callsigns=True), self.text)
        self.assertEqual(self.request.get_header("If-none-match"), self.etag)
        self.assertEqual(json.loads((self.cache / "v2.json").read_text())["snapshot"], self.text)

    def test_corrupt_or_wrong_version_cache_forces_unconditional_fetch(self):
        self.read(self.response())
        original = json.loads((self.cache / "v2.json").read_text())
        for text in ("broken", original["snapshot"].replace("Synthetic First", "Corrupted"),
                     (FIXTURES / "fixtures/roster-v1.json").read_text()):
            cached = dict(original, snapshot=text)
            (self.cache / "v2.json").write_text(json.dumps(cached))
            self.assertEqual(self.read(self.response()), self.text)
            self.assertIsNone(self.request.get_header("If-none-match"))

    def test_changed_endpoint_and_separate_v1_cache_do_not_send_v2_etag(self):
        self.read(self.response())
        self.read(self.response(), url=self.url.replace("public.", "other."))
        self.assertIsNone(self.request.get_header("If-none-match"))
        v1 = (FIXTURES / "fixtures/roster-v1.json").read_text()
        etag = f'W/"{json.loads(v1)["content_hash"]}"'
        self.assertEqual(self.read(self.response(v1, etag), version="1"), v1)
        self.assertIsNone(self.request.get_header("If-none-match"))
        self.assertTrue((self.cache / "v1.json").is_file())
        self.assertTrue((self.cache / "v2.json").is_file())

    def test_unsolicited_or_mismatched_304_fails_closed(self):
        with self.assertRaisesRegex(ValueError, "matching validated cache"):
            self.read(self.not_modified())
        self.read(self.response())
        for etag in ('W/"sha256:' + "a" * 64 + '"', None):
            response = self.not_modified(etag)
            if etag is None:
                del response.headers["ETag"]
            with self.assertRaisesRegex(ValueError, "matching validated cache"):
                self.read(response)
        cached = json.loads((self.cache / "v2.json").read_text())
        cached["snapshot"] = cached["snapshot"].replace("Synthetic First", "Corrupted")
        (self.cache / "v2.json").write_text(json.dumps(cached))
        with self.assertRaisesRegex(ValueError, "matching validated cache"):
            self.read(self.not_modified())

    def test_failed_fetch_validation_or_etag_does_not_replace_last_snapshot(self):
        self.read(self.response())
        before = (self.cache / "v2.json").read_bytes()
        invalid = self.text.replace("Synthetic First", "Corrupted")
        cases = (self.response(invalid), self.response(etag='W/"wrong"'),
                 urllib.error.HTTPError(self.url, 503, "Unavailable", {}, None))
        for response in cases:
            with self.assertRaises((ValueError, OSError)):
                self.read(response)
            self.assertEqual((self.cache / "v2.json").read_bytes(), before)

    def test_changed_generation_time_is_saved_on_200_and_not_on_304(self):
        self.read(self.response())
        envelope = json.loads(self.text)
        envelope["generated_at"] = "2026-10-13T12:00:00Z"
        text = json.dumps(envelope)
        self.assertEqual(self.read(self.response(text)), text)
        self.assertEqual(self.request.get_header("If-none-match"), self.etag)
        self.assertEqual(self.read(self.not_modified()), text)


if __name__ == "__main__":
    unittest.main()
