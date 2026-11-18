"""Exercise the production snapshot boundary without any live network access."""

import contextlib
import io
import json
from pathlib import Path
import socket
import tempfile
import unittest
from unittest.mock import patch

from test_deployment import SOURCE_URL, load_builder, load_script, photo_fixture
from test_roster_input import FakeResponse, FIXTURES, rehash, roster_input
from site_contract import PUBLIC_ROSTER_PATH, validate_dist

ROOT = Path(__file__).resolve().parents[1]


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.command = load_script("fetch-roster.py")
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.workspace = Path(temporary.name)
        self.snapshot = self.workspace / "input/roster.json"
        self.body = (FIXTURES / "fixtures/roster-v2.json").read_bytes()
        for replacement in (
            patch.dict(roster_input.os.environ, {"ROSTER_EXPORT_URL": SOURCE_URL}, clear=True),
            patch.object(socket.socket, "connect", side_effect=AssertionError("Offline tests only")),
        ):
            replacement.start()
            self.addCleanup(replacement.stop)

    def fetch(self):
        with contextlib.redirect_stdout(io.StringIO()) as record, contextlib.redirect_stderr(io.StringIO()) as errors:
            status = self.command.main(["--output", str(self.snapshot)])
        self.record = record.getvalue()
        self.errors = errors.getvalue()
        self.assertNotIn(SOURCE_URL, errors.getvalue())
        return status

    def test_one_anonymous_fetch_builds_all_artifacts_from_saved_file(self):
        photo_bytes, photo = photo_fixture()
        source = json.loads(self.body)
        source["members"][0]["photo"] = photo
        self.body = rehash(source).encode()
        with patch.object(roster_input.urllib.request, "build_opener") as opener:
            opener.return_value.open.return_value = FakeResponse(self.body)
            self.assertEqual(self.fetch(), 0)
            opener.return_value.open.assert_called_once()
            request = opener.return_value.open.call_args.args[0]
            self.assertEqual(request.full_url, SOURCE_URL)
            self.assertFalse(any("access" in field.lower() for field in request.headers))
            self.assertEqual(self.snapshot.read_bytes(), self.body)
            self.assertEqual(list(self.snapshot.parent.iterdir()), [self.snapshot])
            self.assertEqual(json.loads(self.record), {
                "active_members": len(source["members"]),
                "content_hash": source["content_hash"],
                "generated_at": source["generated_at"],
                "schema_version": source["schema_version"],
            })

        assets = self.workspace / "assets"
        fetch_assets = load_script("fetch-public-assets.py")
        with patch.object(roster_input.urllib.request, "build_opener") as opener, \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            response = FakeResponse(photo_bytes)
            response.headers["Content-Type"] = "image/webp"
            opener.return_value.open.return_value = response
            self.assertEqual(fetch_assets.main(["--roster-json", str(self.snapshot), "--asset-dir", str(assets)]), 0)
            opener.return_value.open.assert_called_once()
            self.assertEqual(opener.return_value.open.call_args.args[0].full_url, photo["url"])

        site = self.workspace / "site"
        builder = load_builder()
        with patch.object(roster_input.urllib.request, "build_opener", side_effect=AssertionError("No refetch")), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(builder.main(["--roster-json", str(self.snapshot), "--asset-dir", str(assets),
                                           "--output-dir", str(site)]), 0)
            validate_dist(site)
        self.assertEqual(json.loads((site / PUBLIC_ROSTER_PATH).read_text(encoding="utf-8")), source)
        self.assertEqual(list((site / PUBLIC_ROSTER_PATH).parent.iterdir()), [site / PUBLIC_ROSTER_PATH])
        self.assertFalse((site / "input").exists())
        self.assertFalse((site / "roster.json").exists())
        self.assertEqual((site / "images/mugshots" / f"{photo['sha256']}.webp").read_bytes(), photo_bytes)

    def test_transport_invalid_empty_and_hash_failures_preserve_saved_snapshot(self):
        self.snapshot.parent.mkdir()
        self.snapshot.write_bytes(self.body)
        for body in (b"{", b"{}",
                     self.body.replace(b"Synthetic First", b"Unreviewed Name"),
                     (FIXTURES / "fixtures/empty-v2.json").read_bytes()):
            with self.subTest(body=body[:20]), patch.object(roster_input.urllib.request, "build_opener") as opener:
                opener.return_value.open.return_value = FakeResponse(body)
                self.assertEqual(self.fetch(), 1)
                self.assertEqual(self.snapshot.read_bytes(), self.body)
        with patch.object(self.command, "read_roster_json", side_effect=OSError("private URL/body/token")):
            self.assertEqual(self.fetch(), 1)
            self.assertEqual(self.snapshot.read_bytes(), self.body)
        with patch.dict(roster_input.os.environ, {}, clear=True):
            self.assertEqual(self.fetch(), 1)
            self.assertEqual(self.snapshot.read_bytes(), self.body)

    def test_invalid_callsign_identifies_the_member_and_preserves_the_saved_snapshot(self):
        self.snapshot.parent.mkdir()
        self.snapshot.write_bytes(self.body)
        invalid = json.loads(self.body)
        member = invalid["members"][0]
        member["callsign"] = "KCØDWZ"
        with patch.object(roster_input.urllib.request, "build_opener") as opener:
            opener.return_value.open.return_value = FakeResponse(rehash(invalid).encode())
            self.assertEqual(self.fetch(), 1)
        self.assertEqual(self.snapshot.read_bytes(), self.body)
        self.assertIn(f"Invalid member callsign (BKG #{member['bkg_number']})", self.errors)
        self.assertNotIn(member["callsign"], self.errors)

    def test_snapshot_inside_checkout_rejected_before_network(self):
        with patch.object(self.command, "read_roster_json") as read, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(self.command.main(["--output", str(ROOT / "dist/roster-input.json")]), 1)
        read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
