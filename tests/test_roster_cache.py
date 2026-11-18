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
        self.text = (FIXTURES / "fixtures/roster-v2.json").read_text(encoding="utf-8")
        self.etag = f'W/"{json.loads(self.text)["content_hash"]}"'

    def response(self, text=None, etag=None):
        response = FakeResponse((text or self.text).encode())
        response.headers = Message()
        response.headers["ETag"] = etag or self.etag
        return response

    def read(self, response, *, url=None):
        with patch.object(roster_input.urllib.request, "build_opener") as opener:
            if isinstance(response, Exception):
                opener.return_value.open.side_effect = response
            else:
                opener.return_value.open.return_value = response
            text = roster_input.read_roster_json(url=url or self.url, environ={}, cache_dir=self.cache)
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

    def test_cached_snapshot_with_invalid_callsign_forces_unconditional_fetch(self):
        self.read(self.response())
        cache_file = self.cache / "v2.json"
        cached = json.loads(cache_file.read_text(encoding="utf-8"))
        original = json.loads(self.text)
        original["members"][0]["callsign"] = "KCØDWZ"
        original["content_hash"] = roster_input.roster_content_hash(original)
        cached["snapshot"] = json.dumps(original, ensure_ascii=False)
        cached["etag"] = f'W/"{original["content_hash"]}"'
        cache_file.write_text(json.dumps(cached, ensure_ascii=False), encoding="utf-8")
        self.assertEqual(self.read(self.response()), self.text)
        self.assertIsNone(self.request.get_header("If-none-match"))
        self.assertEqual(json.loads(cache_file.read_text(encoding="utf-8"))["snapshot"], self.text)

    def test_corrupt_or_wrong_version_cache_forces_unconditional_fetch(self):
        self.read(self.response())
        original = json.loads((self.cache / "v2.json").read_text(encoding="utf-8"))
        wrong_version = json.loads(self.text)
        wrong_version["schema_version"] = "1"
        wrong_version["content_hash"] = roster_input.roster_content_hash(wrong_version)
        for text in ("broken", original["snapshot"].replace("Synthetic First", "Corrupted"),
                     json.dumps(wrong_version)):
            cached = dict(original, snapshot=text)
            (self.cache / "v2.json").write_text(json.dumps(cached), encoding="utf-8")
            self.assertEqual(self.read(self.response()), self.text)
            self.assertIsNone(self.request.get_header("If-none-match"))

    def test_changed_endpoint_does_not_send_another_endpoints_etag(self):
        self.read(self.response())
        self.read(self.response(), url=self.url.replace("public.", "other."))
        self.assertIsNone(self.request.get_header("If-none-match"))
        self.read(self.response())
        self.assertIsNone(self.request.get_header("If-none-match"))

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
        cached = json.loads((self.cache / "v2.json").read_text(encoding="utf-8"))
        cached["snapshot"] = cached["snapshot"].replace("Synthetic First", "Corrupted")
        (self.cache / "v2.json").write_text(json.dumps(cached), encoding="utf-8")
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
