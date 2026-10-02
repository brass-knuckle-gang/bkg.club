"""Exercise the production snapshot boundary without any live network access."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import socket
import tempfile
import unittest
from unittest.mock import patch

from test_deployment import load_builder
from test_roster_enrichment import png
from test_roster_input import FakeResponse, FIXTURES, roster_input
from site_contract import validate_dist

ROOT = Path(__file__).resolve().parents[1]


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("fetch_roster", ROOT / "scripts/fetch-roster.py")
        self.command = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.command)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.workspace = Path(temporary.name)
        self.snapshot = self.workspace / "input/roster.json"
        self.body = (FIXTURES / "fixtures/roster-v1.json").read_bytes()
        for replacement in (
            patch.dict(roster_input.os.environ, {"ROSTER_EXPORT_URL": "https://private.fixture.invalid/export"}, clear=True),
            patch.object(socket.socket, "connect", side_effect=AssertionError("Offline tests only")),
        ):
            replacement.start()
            self.addCleanup(replacement.stop)

    def fetch(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as errors:
            status = self.command.main(["--output", str(self.snapshot)])
        self.assertNotIn("private.fixture.invalid", errors.getvalue())
        return status

    def test_one_anonymous_fetch_builds_all_artifacts_from_saved_file(self):
        with patch.object(roster_input.urllib.request, "build_opener") as opener:
            opener.return_value.open.return_value = FakeResponse(self.body)
            self.assertEqual(self.fetch(), 0)
            opener.return_value.open.assert_called_once()
            self.assertEqual(self.snapshot.read_bytes(), self.body)
        builder = load_builder()
        site = self.workspace / "site"
        cache = self.workspace / "enrichment"
        spec = importlib.util.spec_from_file_location("refresh_qrz", ROOT / "scripts/refresh-qrz.py")
        refresh = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(refresh)
        client = load_builder()

        def reviewed_qrz(_session, callsign):
            return {"current_call": callsign, "country": "United States", "state": "IL",
                    "grid": "EN61AB", "image": "https://fixture.invalid/photo.png"}

        # Every subsequent consumer must work with networking forbidden.
        with patch.object(roster_input.urllib.request, "build_opener", side_effect=AssertionError("No second fetch")), \
                patch.object(builder, "fetch_csv", side_effect=AssertionError("No Sheets fallback")), \
                patch.object(builder, "annotate_qrz", side_effect=AssertionError("No render enrichment refresh")), \
                patch.dict(roster_input.os.environ, {"QRZ_USERNAME": "fixture", "QRZ_PASSWORD": "fixture"}), \
                patch.object(refresh, "load_qrz_client", return_value=client), \
                patch.object(client, "qrz_login", return_value="fixture-session"), \
                patch.object(client, "qrz_fetch_callsign", side_effect=reviewed_qrz), \
                patch.object(refresh, "fetch_photo", return_value=png(metadata=True)), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(refresh.main(["--roster-json", str(self.snapshot),
                                           "--enrichment-dir", str(cache), "--require-usable"]), 0)
            self.assertEqual(builder.main(["--source", "json", "--roster-json", str(self.snapshot),
                                           "--enrichment-dir", str(cache),
                                           "--output-dir", str(site)]), 0)
            validate_dist(site, source="json")
        self.assertEqual(json.loads((site / "data/v1/roster.json").read_text()), json.loads(self.body))
        self.assertFalse((site / "input").exists())
        self.assertFalse((site / "roster.json").exists())
        self.assertNotIn("private", (cache / "enrichment.json").read_text())
        self.assertEqual(len(list((site / "images/mugshots").iterdir())), 6)

    def test_transport_invalid_empty_and_hash_failures_preserve_saved_snapshot(self):
        self.snapshot.parent.mkdir()
        self.snapshot.write_bytes(self.body)
        for body in (b"{", b"{}", self.body.replace(b"Synthetic First", b"Unreviewed Name"),
                     (FIXTURES / "fixtures/empty-v1.json").read_bytes()):
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

    def test_snapshot_inside_checkout_rejected_before_network(self):
        with patch.object(self.command, "read_roster_json") as read, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(self.command.main(["--output", str(ROOT / "dist/roster-input.json")]), 1)
        read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
