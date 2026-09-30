"""Offline build/validation contract. No request can reach a production service."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import socket
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = Path(__file__).parent / "fixtures"
sys.path.insert(0, str(ROOT / "scripts"))
import site_contract


def load_builder():
    spec = importlib.util.spec_from_file_location("build_roster", ROOT / "scripts/build-roster.py")
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    return builder


class DeploymentTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.workspace = Path(temporary.name)
        self.source = self.workspace / "source"
        self.dist = self.workspace / "dist"
        self.source.mkdir()
        self.builder = load_builder()
        for relative in site_contract.PUBLIC_FILES:
            target = self.source / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        for relative in ("name-overrides.txt", "location-overrides.txt"):
            shutil.copyfile(FIXTURES / relative, self.source / relative)
        (self.source / "qrz-cache.json").write_text('{"W1OLD":"W1NEW"}\n')
        mugshots = self.source / "images/mugshots"
        mugshots.mkdir(parents=True)
        (mugshots / "K2TST.jpg").write_bytes(b"unchanged-photo")
        (mugshots / "K4TST.jpg").write_bytes(b"previous-photo")
        (mugshots / ".sources.json").write_text(json.dumps({
            "K2TST": "https://fixture.invalid/unchanged.jpg",
            "K4TST": "https://fixture.invalid/previous.jpg",
        }))
        overrides = self.source / "images/mugshots-override"
        overrides.mkdir()
        (overrides / "W1NEW.png").write_bytes(b"override-photo")
        (self.source / "private.txt").write_text("must never be published")

        constants = {
            "REPO_ROOT": self.source,
            "INDEX_PATH": self.source / "index.html",
            "TREE_PATH": self.source / "tree.html",
            "NEARBY_PATH": self.source / "nearby.html",
            "OUTBREAK_PATH": self.source / "outbreak.html",
            "NAME_OVERRIDES_PATH": self.source / "name-overrides.txt",
            "LOCATION_OVERRIDES_PATH": self.source / "location-overrides.txt",
            "QRZ_CACHE_PATH": self.source / "qrz-cache.json",
            "MUGSHOT_DIR": mugshots,
            "MUGSHOT_SOURCES_PATH": mugshots / ".sources.json",
            "MUGSHOT_OVERRIDE_DIR": overrides,
        }
        for name, value in constants.items():
            setattr(self.builder, name, value)
        self.csv = (FIXTURES / "roster.csv").read_text()
        self.qrz = json.loads((FIXTURES / "qrz.json").read_text())
        self.downloads = []
        patches = [
            patch.dict(os.environ, {"QRZ_USERNAME": "fixture", "QRZ_PASSWORD": "fixture"}),
            patch.object(self.builder, "fetch_csv", side_effect=lambda: self.csv),
            patch.object(self.builder, "_fetch_with_retry", side_effect=self.fixture_response),
            patch.object(urllib.request, "urlopen", side_effect=AssertionError("Network forbidden in tests")),
            patch.object(socket, "create_connection", side_effect=AssertionError("Network forbidden in tests")),
            patch.object(socket.socket, "connect", side_effect=AssertionError("Network forbidden in tests")),
        ]
        for replacement in patches:
            replacement.start()
            self.addCleanup(replacement.stop)

    def fixture_response(self, request, **_kwargs):
        url = urllib.parse.urlparse(request.full_url)
        if url.hostname == "xmldata.qrz.com":
            parameters = urllib.parse.parse_qs(url.query)
            root = ET.Element("QRZDatabase", xmlns="http://xmldata.qrz.com")
            if "username" in parameters:
                ET.SubElement(ET.SubElement(root, "Session"), "Key").text = "fixture-session"
            else:
                info = self.qrz[parameters["callsign"][0]]
                if info is None:
                    return None
                call = ET.SubElement(root, "Callsign")
                for name, value in info.items():
                    ET.SubElement(call, name).text = value
            return ET.tostring(root)
        self.assertEqual(url.hostname, "fixture.invalid")
        self.downloads.append(request.full_url)
        if url.path == "/new.png":
            return b"downloaded-photo"
        if url.path == "/unavailable.jpg":
            return None
        self.fail(f"Unexpected photo download: {request.full_url}")

    def build(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return self.builder.main(["--output-dir", str(self.dist)])

    def data(self, page, marker):
        text = (self.dist / page).read_text()
        pattern = rf"<!-- {marker}:START -->(.*?)<!-- {marker}:END -->"
        return json.loads(re.search(pattern, text, re.DOTALL)[1])

    def replace_data(self, page, marker, data):
        target = self.dist / page
        target.write_text(self.builder.replace_between(
            target.read_text(), f"<!-- {marker}:START -->", f"<!-- {marker}:END -->", json.dumps(data)
        ))

    def test_full_fixture_build_preserves_enrichment_and_public_site(self):
        source_index = (self.source / "index.html").read_bytes()
        self.assertEqual(self.build(), 0)
        site_contract.validate_dist(self.dist)
        self.assertEqual((self.source / "index.html").read_bytes(), source_index)
        for relative in site_contract.PUBLIC_FILES:
            self.assertTrue((self.dist / relative).is_file(), relative)
        for private in ("qrz-cache.json", "name-overrides.txt", "location-overrides.txt", "private.txt", "scripts", ".github", "images/mugshots/.sources.json"):
            self.assertFalse((self.dist / private).exists(), private)

        index = (self.dist / "index.html").read_text()
        self.assertIn("<!-- MEMBER_COUNT:START -->4<!-- MEMBER_COUNT:END -->", index)
        self.assertIn('class="member-card founder-card"', index)
        self.assertIn("BKG #005", index)  # The existing future-member card.
        self.assertIn('href="https://www.qrz.com/db/W1NEW"', index)
        self.assertIn('<div class="member-name">Scout</div>', index)
        self.assertNotIn('href="https://www.qrz.com/db/W1OLD"', index)

        tree = self.data("tree.html", "DOWNLINE_DATA")
        self.assertEqual([entry["call"] for entry in tree], ["W1NEW", "K2TST", "VE3TST", "K4TST"])
        self.assertEqual([entry["sponsor"] for entry in tree], [None, "W1NEW", "K2TST", "W1NEW"])
        territory = self.data("index.html", "MAP_DATA")
        self.assertEqual(territory["states"]["TX"][0]["call"], "K2TST")
        self.assertEqual(territory["dx"]["Canada"]["members"][0]["call"], "VE3TST")
        geo = self.data("nearby.html", "GEO_DATA")
        self.assertEqual([entry["call"] for entry in geo], ["W1NEW", "VE3TST", "K4TST"])
        self.assertEqual((geo[1]["lat"], geo[1]["lon"]), (43.65, -79.38))
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        self.assertEqual([entry["date"] for entry in outbreak], ["2025-01-02", "2025-01-03", "2025-01-04", "2025-01-05"])
        self.assertEqual(outbreak[1]["state"], "TX")
        self.assertNotIn("lat", outbreak[1])
        self.assertEqual(outbreak[3]["sponsor"], "W1NEW")
        notes = [line for line in (self.dist / "members.txt").read_text().splitlines() if line and not line.startswith("#")]
        self.assertEqual(notes, [
            "K2TST 🤜 Scout BKG #2 (OG, TX OG)",
            "K4TST 🤜 Dana E BKG #4 (CA OG)",
            "VE3TST 🤜 Cora E BKG #3 (Canada OG)",
            "W1NEW 🤜 Ada E BKG #1 (OG, UT OG)",
        ])
        self.assertEqual((self.dist / "images/mugshots/W1NEW.png").read_bytes(), b"override-photo")
        self.assertEqual((self.dist / "images/mugshots/K2TST.jpg").read_bytes(), b"unchanged-photo")
        self.assertEqual((self.dist / "images/mugshots/K4TST.jpg").read_bytes(), b"previous-photo")
        self.assertEqual((self.dist / "images/mugshots/VE3TST.png").read_bytes(), b"downloaded-photo")
        self.assertEqual(self.downloads, ["https://fixture.invalid/new.png", "https://fixture.invalid/unavailable.jpg"])
        self.assertEqual(json.loads((self.source / "qrz-cache.json").read_text())["W1OLD"], "W1NEW")

    def test_qrz_outage_uses_canonical_cache_and_previous_photos(self):
        self.qrz = {callsign: None for callsign in self.qrz}
        self.assertEqual(self.build(), 0)
        site_contract.validate_dist(self.dist)
        self.assertEqual(self.data("tree.html", "DOWNLINE_DATA")[1]["sponsor"], "W1NEW")
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])
        self.assertEqual((self.dist / "images/mugshots/K4TST.jpg").read_bytes(), b"previous-photo")
        self.assertEqual(self.downloads, [])

    def test_join_dates_use_portable_iso_formatting(self):
        # Match Linux's unpadded strftime behavior even when tests run on macOS.
        from datetime import datetime

        class LinuxDatetime(datetime):
            def strftime(self, fmt):
                if fmt == "%Y-%m-%d":
                    return f"{self.year}-{self.month:02d}-{self.day:02d}"
                return super().strftime(fmt)

        cases = {
            "8/20/0026": "2026-08-20",
            "8/20/26": "2026-08-20",
            "0026-09-03": "2026-09-03",
            "0026-09-03T12:34:56Z": "2026-09-03",
            "0001-01-01": "2001-01-01",
            "0068-01-01": "2068-01-01",
            "0069-01-01": "1969-01-01",
            "0024-02-29": "2024-02-29",
            "0999-12-31": "0999-12-31",
            "2026-09-03": "2026-09-03",
            "02/29/2024": "2024-02-29",
            "02/29/2025": None,
            "0000-01-01": None,
            "": None,
        }
        with patch.object(self.builder, "datetime", LinuxDatetime):
            for raw, expected in cases.items():
                with self.subTest(raw=raw):
                    self.assertEqual(self.builder.parse_join_date(raw), expected)

    def test_low_year_sheet_dates_build_and_validate(self):
        self.csv = self.csv.replace("2025-01-02", "8/20/0026").replace("01/03/2025", "0026-09-03")
        self.assertEqual(self.build(), 0)
        site_contract.validate_dist(self.dist)
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        self.assertEqual([entry["date"] for entry in outbreak[:2]], ["2026-08-20", "2026-09-03"])

    def test_validator_still_rejects_malformed_outbreak_dates(self):
        self.assertEqual(self.build(), 0)
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        for invalid in ("26-08-20", "2026-02-30", "20260903"):
            with self.subTest(date=invalid):
                outbreak[0]["date"] = invalid
                self.replace_data("outbreak.html", "OUTBREAK_DATA", outbreak)
                with self.assertRaisesRegex(ValueError, "Invalid outbreak date"):
                    site_contract.validate_dist(self.dist)

    def test_invalid_input_preserves_previous_dist(self):
        self.dist.mkdir()
        sentinel = self.dist / "index.html"
        sentinel.write_text("previous validated build")
        invalid_inputs = [
            "", "not,the,roster\ninvalid,csv,data\n", "#,Callsign,Name\n",
            self.csv + "1,K9TST,Duplicate,2025-01-06,Utah,\n",
            self.csv + "5,w1old,Duplicate,2025-01-06,Utah,\n",
        ]
        for invalid in invalid_inputs:
            with self.subTest(csv=invalid):
                self.csv = invalid
                self.assertNotEqual(self.build(), 0)
                self.assertEqual(sentinel.read_text(), "previous validated build")

    def test_missing_required_template_or_marker_fails_generation(self):
        self.dist.mkdir()
        sentinel = self.dist / "index.html"
        sentinel.write_text("previous validated build")
        for relative, marker in (("tree.html", None), ("nearby.html", "<!-- GEO_DATA:END -->")):
            with self.subTest(relative=relative):
                target = self.source / relative
                original = target.read_text()
                if marker:
                    target.write_text(original.replace(marker, ""))
                else:
                    target.unlink()
                self.assertNotEqual(self.build(), 0)
                self.assertEqual(sentinel.read_text(), "previous validated build")
                target.write_text(original)

    def test_fetch_failure_and_duplicate_canonical_calls_do_not_replace_dist(self):
        self.dist.mkdir()
        sentinel = self.dist / "index.html"
        sentinel.write_text("previous validated build")
        with patch.object(self.builder, "fetch_csv", side_effect=OSError("fixture fetch failure")):
            self.assertNotEqual(self.build(), 0)
        self.assertEqual(sentinel.read_text(), "previous validated build")
        self.qrz["K2TST"]["call"] = "W1NEW"
        self.assertNotEqual(self.build(), 0)
        self.assertEqual(sentinel.read_text(), "previous validated build")

    def test_output_directory_cannot_replace_source_assets(self):
        asset = self.source / "assets/bkg.css"
        original = asset.read_bytes()
        with patch.object(self.builder, "fetch_csv") as fetch, patch.object(self.builder, "annotate_qrz") as enrich:
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                status = self.builder.main(["--output-dir", str(self.source / "assets")])
        self.assertNotEqual(status, 0)
        self.assertEqual(asset.read_bytes(), original)
        fetch.assert_not_called()
        enrich.assert_not_called()

    def test_validator_rejects_missing_assets_photos_and_internal_files(self):
        self.assertEqual(self.build(), 0)
        for relative in (*site_contract.PUBLIC_FILES, "members.txt", "images/mugshots/W1NEW.png"):
            with self.subTest(relative=relative):
                target = self.dist / relative
                original = target.read_bytes()
                target.unlink()
                with self.assertRaises(ValueError):
                    site_contract.validate_dist(self.dist)
                target.write_bytes(original)
        leaked = self.dist / "qrz-cache.json"
        leaked.write_text("{}")
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)

    def test_validator_rejects_sponsor_cycle_consistent_across_pages(self):
        self.assertEqual(self.build(), 0)
        tree = self.data("tree.html", "DOWNLINE_DATA")
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        # K2TST already names W1NEW as sponsor. Both pages agree, but this
        # mutual sponsorship cannot form a valid downline tree.
        tree[0]["sponsor"] = outbreak[0]["sponsor"] = "K2TST"
        self.replace_data("tree.html", "DOWNLINE_DATA", tree)
        self.replace_data("outbreak.html", "OUTBREAK_DATA", outbreak)
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)

    def test_validator_rejects_invalid_us_state_consistent_across_pages(self):
        self.assertEqual(self.build(), 0)
        territory = self.data("index.html", "MAP_DATA")
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        territory["states"]["XX"] = territory["states"].pop("UT")
        outbreak[0]["state"] = "XX"
        self.replace_data("index.html", "MAP_DATA", territory)
        self.replace_data("outbreak.html", "OUTBREAK_DATA", outbreak)
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)

    def test_validator_rejects_malformed_json_and_generated_data_drift(self):
        self.assertEqual(self.build(), 0)
        index = self.dist / "index.html"
        original_index = index.read_text()
        tree = self.dist / "tree.html"
        original_tree = tree.read_text()
        tree.write_text(original_tree.replace('"call":"W1NEW"', '"call":', 1))
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)
        tree.write_text(original_tree)
        index.write_text(original_index.replace("<!-- MEMBER_COUNT:START -->4", "<!-- MEMBER_COUNT:START -->5"))
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)
        index.write_text(original_index)
        notes = self.dist / "members.txt"
        notes.write_text(notes.read_text().replace("BKG #4", "BKG #99"))
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)

    def test_validator_rejects_invalid_coordinates_and_unknown_sponsor(self):
        self.assertEqual(self.build(), 0)
        geo = self.data("nearby.html", "GEO_DATA")
        invalid_geo = [dict(entry) for entry in geo]
        invalid_geo[0]["lat"] = 91
        self.replace_data("nearby.html", "GEO_DATA", invalid_geo)
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)
        self.replace_data("nearby.html", "GEO_DATA", geo)
        tree = self.data("tree.html", "DOWNLINE_DATA")
        tree[1]["sponsor"] = "UNKNOWN"
        self.replace_data("tree.html", "DOWNLINE_DATA", tree)
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)


if __name__ == "__main__":
    unittest.main()
