"""Offline V2 publication and validation contract."""

import contextlib
import copy
import hashlib
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

from PIL import Image

from test_roster_input import FIXTURES, rehash

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import site_contract

SOURCE_URL = "https://admin.fixture.invalid/api/roster/export/v2"


def load_script(filename):
    spec = importlib.util.spec_from_file_location(Path(filename).stem, ROOT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_builder():
    return load_script("build-roster.py")


def photo_fixture():
    output = io.BytesIO()
    Image.new("RGB", (2, 3), (255, 0, 0)).save(output, format="WEBP")
    data = output.getvalue()
    digest = hashlib.sha256(data).hexdigest()
    return data, {
        "url": f"https://admin.fixture.invalid/api/roster/photos/{digest}.webp",
        "sha256": digest, "content_type": "image/webp", "byte_length": len(data),
    }


class CountrySpellingTests(unittest.TestCase):
    def test_countries_differing_only_in_case_share_one_spelling(self):
        builder = load_builder()
        members = [{"country": c} for c in ("Poland", "POLAND", "Poland", "japan", "JAPAN", None, "United States")]
        builder.canonicalize_countries(members)
        self.assertEqual([m["country"] for m in members],
                         ["Poland", "Poland", "Poland", "JAPAN", "JAPAN", None, "United States"])

    def test_mixed_case_wins_a_tie(self):
        builder = load_builder()
        members = [{"country": "POLAND"}, {"country": "Poland"}]
        builder.canonicalize_countries(members)
        self.assertEqual({m["country"] for m in members}, {"Poland"})


class WorkflowTests(unittest.TestCase):
    def test_validates_one_saved_snapshot_before_uploading_the_site(self):
        workflow = (ROOT / ".github/workflows/deploy.yml").read_text()
        snapshot = '"$RUNNER_TEMP/bkg-build/roster.json"'
        steps = (
            "scripts/fetch-roster.py", "scripts/fetch-public-assets.py",
            "scripts/build-roster.py", "scripts/validate-site.py dist",
            "actions/upload-pages-artifact@", "actions/deploy-pages@",
        )
        offsets = [workflow.index(step) for step in steps]
        self.assertEqual(offsets, sorted(offsets))
        self.assertEqual(workflow.count(steps[0]), 1)
        self.assertIn(f"--output {snapshot}", workflow)
        self.assertEqual(workflow.count(f"--roster-json {snapshot}"), 2)
        self.assertIn("if: needs.build.outputs.changed == 'true'", workflow)
        self.assertIn("artifact_name: ${{ needs.build.outputs.artifact_name }}", workflow)


class PublicationBuildFixture(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.workspace = Path(temporary.name)
        self.source = self.workspace / "source"
        self.dist = self.workspace / "dist"
        self.assets = self.workspace / "public-assets"
        self.assets.mkdir()
        self.roster = self.workspace / "roster.json"
        self.builder = load_builder()
        for relative in site_contract.PUBLIC_FILES:
            target = self.source / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        shutil.copytree(ROOT / site_contract.GAME_DIR, self.source / site_contract.GAME_DIR)
        (self.source / "private.txt").write_text("must never be published")

        paths = {
            "REPO_ROOT": self.source,
            "INDEX_PATH": self.source / "index.html",
            "TREE_PATH": self.source / "tree.html",
            "NEARBY_PATH": self.source / "nearby.html",
            "OUTBREAK_PATH": self.source / "outbreak.html",
        }
        for name, value in paths.items():
            setattr(self.builder, name, value)
        for replacement in (
            patch.dict(os.environ, {"ROSTER_LAYOUT": "list"}),
            patch.object(socket.socket, "connect", side_effect=AssertionError("Offline tests only")),
        ):
            replacement.start()
            self.addCleanup(replacement.stop)

        self.envelope = json.loads((FIXTURES / "fixtures/roster-v2.json").read_text())
        self.photo_bytes, self.photo = photo_fixture()
        self.envelope["members"][0]["photo"] = self.photo
        self.asset = self.assets / f"{self.photo['sha256']}.webp"
        self.asset.write_bytes(self.photo_bytes)
        self.write_roster()

    def write_roster(self):
        self.roster.write_text(rehash(self.envelope))

    def build(self, *, output=None, extra=()):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return self.builder.main([
                "--roster-json", str(self.roster), "--asset-dir", str(self.assets),
                "--asset-origin", SOURCE_URL, "--output-dir", str(output or self.dist), *extra,
            ])

    def data(self, page, marker):
        text = (self.dist / page).read_text()
        pattern = rf"<!-- {marker}:START -->(.*?)<!-- {marker}:END -->"
        return json.loads(re.search(pattern, text, re.DOTALL)[1])

    def replace_data(self, page, marker, data):
        target = self.dist / page
        target.write_text(self.builder.replace_between(
            target.read_text(), f"<!-- {marker}:START -->", f"<!-- {marker}:END -->", json.dumps(data)
        ))

    def output_files(self, directory):
        return {file.relative_to(directory): file.read_bytes() for file in directory.rglob("*") if file.is_file()}


class DeploymentTests(PublicationBuildFixture):
    def test_build_preserves_the_offline_static_publication_contract(self):
        source_index = (self.source / "index.html").read_bytes()
        self.assertEqual(self.build(), 0)
        site_contract.validate_dist(self.dist)
        self.assertEqual((self.source / "index.html").read_bytes(), source_index)
        for relative in (*site_contract.PUBLIC_FILES, *site_contract.GAME_REQUIRED):
            self.assertTrue((self.dist / relative).is_file(), relative)
        for private in ("private.txt", "roster.json", "scripts", ".github"):
            self.assertFalse((self.dist / private).exists(), private)

        index = (self.dist / "index.html").read_text()
        self.assertIn("<!-- MEMBER_COUNT:START -->6<!-- MEMBER_COUNT:END -->", index)
        self.assertIn('<div class="rl" data-view="list">', index)
        self.assertNotIn('class="member-card', index)
        self.assertIn('id="VE3SYN" data-n="21" data-joined="2026-08-21" data-place="Canada"', index)
        self.assertIn('data-sponsor="N0SYN7"', index)
        self.assertIn('<link rel="stylesheet" href="assets/roster.css">', index)
        self.assertIn('<script src="assets/roster.js" defer></script>', index)
        self.assertIn('title="VT OG">VT OG</div>', index)
        self.assertIn('href="https://www.qrz.com/db/N0SYN1"', index)
        territory = self.data("index.html", "MAP_DATA")
        self.assertEqual(territory["states"]["NY"][0]["call"], "K2SYN88")
        self.assertEqual(territory["dx"]["Canada"]["members"][0]["call"], "VE3SYN")
        tree = self.data("tree.html", "DOWNLINE_DATA")
        self.assertEqual([entry["sponsor"] for entry in tree], [None, "N0SYN1", None, "N0SYN7", None, "VE3SYN"])
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        self.assertEqual([entry["date"] for entry in outbreak],
                         ["2026-08-21", "2026-08-31", None, "2026-08-21", "2026-08-25", "2026-07-02"])
        self.assertIn("BKG #88 (VT OG)", (self.dist / "members.txt").read_text())

    def test_roster_layout_switch_restores_classic_card_wall(self):
        with patch.dict(os.environ, {"ROSTER_LAYOUT": "classic"}):
            self.assertEqual(self.build(), 0)
        site_contract.validate_dist(self.dist)
        index = (self.dist / "index.html").read_text()
        self.assertIn('class="member-card founder-card"', index)
        self.assertIn('href="https://www.qrz.com/db/N0SYN1"', index)
        self.assertNotIn('class="rl"', index)
        # The list assets still ship, and roster.js does nothing without .rl.
        self.assertTrue((self.dist / "assets/roster.js").is_file())

    def test_unknown_roster_layout_fails_and_keeps_previous_dist(self):
        self.assertEqual(self.build(), 0)
        previous = (self.dist / "index.html").read_bytes()
        with patch.dict(os.environ, {"ROSTER_LAYOUT": "cards"}):
            self.assertEqual(self.build(), 1)
        self.assertEqual((self.dist / "index.html").read_bytes(), previous)

    def test_blank_roster_layout_variable_uses_list(self):
        # An unset repository variable can reach the build as an empty string.
        with patch.dict(os.environ, {"ROSTER_LAYOUT": ""}):
            self.assertEqual(self.build(), 0)
        self.assertIn('class="rl"', (self.dist / "index.html").read_text())

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

    def test_low_year_roster_dates_build_and_validate(self):
        first, second = self.envelope["members"][:2]
        first["qso_date"], second["qso_date"] = "8/20/0026", "0026-09-03"
        self.write_roster()
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

    def test_failed_output_swap_restores_previous_build(self):
        self.dist.mkdir()
        sentinel = self.dist / "index.html"
        sentinel.write_text("previous validated build")
        rename = Path.rename

        def fail_publish(path, target):
            if path.name == "public" and Path(target).resolve() == self.dist.resolve():
                raise OSError("fixture final rename failure")
            return rename(path, target)

        with patch.object(Path, "rename", autospec=True, side_effect=fail_publish):
            self.assertNotEqual(self.build(), 0)
        self.assertEqual(sentinel.read_text(), "previous validated build")

    def test_validator_rejects_missing_assets_photos_and_internal_files(self):
        self.assertEqual(self.build(), 0)
        for relative in (*site_contract.PUBLIC_FILES, *site_contract.GAME_REQUIRED,
                         "members.txt", "data/v2/roster.json", f"images/mugshots/{self.asset.name}"):
            with self.subTest(relative=relative):
                target = self.dist / relative
                original = target.read_bytes()
                target.unlink()
                with self.assertRaises(ValueError):
                    site_contract.validate_dist(self.dist)
                target.write_bytes(original)
        leaked = self.dist / "private.txt"
        leaked.write_text("{}")
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)

    def test_validator_rejects_sponsor_drift_even_when_generated_pages_agree(self):
        self.assertEqual(self.build(), 0)
        tree = self.data("tree.html", "DOWNLINE_DATA")
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        tree[1]["sponsor"] = outbreak[1]["sponsor"] = None
        self.replace_data("tree.html", "DOWNLINE_DATA", tree)
        self.replace_data("outbreak.html", "OUTBREAK_DATA", outbreak)
        with self.assertRaisesRegex(ValueError, "sponsors differ from pages"):
            site_contract.validate_dist(self.dist)

    def test_validator_rejects_invalid_us_state_consistent_across_pages(self):
        self.assertEqual(self.build(), 0)
        territory = self.data("index.html", "MAP_DATA")
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        territory["states"]["XX"] = territory["states"].pop("IL")
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
        tree.write_text(original_tree.replace('"call":"N0SYN1"', '"call":', 1))
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)
        tree.write_text(original_tree)
        index.write_text(original_index.replace("<!-- MEMBER_COUNT:START -->6", "<!-- MEMBER_COUNT:START -->7"))
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)
        index.write_text(original_index)
        notes = self.dist / "members.txt"
        notes.write_text(notes.read_text().replace("BKG #88", "BKG #99"))
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

    def test_malformed_or_empty_contract_preserves_previous_output(self):
        self.assertEqual(self.build(), 0)
        previous = self.output_files(self.dist)
        missing_sponsor = copy.deepcopy(self.envelope)
        missing_sponsor["members"][1]["sponsor_bkg_number"] = 99
        leaked_private = copy.deepcopy(self.envelope)
        leaked_private["members"][0]["email"] = "private@example.invalid"
        wrong_hash = copy.deepcopy(self.envelope)
        wrong_hash["members"][0]["name"] = "Fictional Fallback"
        invalid = {
            "missing file": None,
            "truncated JSON": "{",
            "unsupported envelope": "{}",
            "missing sponsor": rehash(missing_sponsor),
            "private field": rehash(leaked_private),
            "mismatched hash": json.dumps(wrong_hash),
            "empty roster": (FIXTURES / "fixtures/empty-v2.json").read_text(),
        }
        for case, raw in invalid.items():
            with self.subTest(case=case):
                if raw is None:
                    self.roster.unlink()
                else:
                    self.roster.write_text(raw)
                self.assertNotEqual(self.build(), 0)
                self.assertEqual(self.output_files(self.dist), previous)

    def test_production_dist_can_build_without_replacing_source_or_inputs(self):
        production = self.source / "dist"
        self.assertEqual(self.build(output=production), 0)
        site_contract.validate_dist(production)
        previous = self.output_files(production)
        source_asset = self.source / "assets/bkg.css"
        original = source_asset.read_bytes()
        for output in (self.source / "experimental", self.source / "assets", self.source, self.source.parent):
            with self.subTest(output=output):
                self.assertNotEqual(self.build(output=output), 0)
        self.assertEqual(source_asset.read_bytes(), original)
        self.assertFalse((self.source / "experimental").exists())
        self.assertTrue(self.roster.is_file())
        self.assertEqual(self.output_files(production), previous)

    def test_fingerprint_ignores_build_times_but_not_visible_changes(self):
        fingerprint = load_script("site-fingerprint.py")
        later = self.workspace / "later"
        self.assertEqual(self.build(), 0)
        self.envelope["generated_at"] = "2026-12-31T23:59:59Z"
        self.write_roster()
        with patch.object(self.builder, "datetime", wraps=self.builder.datetime) as clock:
            clock.now.return_value = self.builder.datetime(2030, 1, 2, 3, 4, 5, tzinfo=self.builder.timezone.utc)
            self.assertEqual(self.build(output=later), 0)
        self.assertNotEqual((self.dist / "index.html").read_bytes(), (later / "index.html").read_bytes())
        self.assertNotEqual((self.dist / "members.txt").read_bytes(), (later / "members.txt").read_bytes())
        self.assertEqual(fingerprint.fingerprint(self.dist), fingerprint.fingerprint(later))
        self.envelope["members"][0]["name"] = "Renamed Member"
        self.write_roster()
        self.assertEqual(self.build(output=later), 0)
        self.assertNotEqual(fingerprint.fingerprint(self.dist), fingerprint.fingerprint(later))

    def test_reviewed_html_and_script_text_round_trips_without_execution(self):
        payload = '</script><script>alert("synthetic")</script>&'
        qth = payload + '\u2028\u2029Synthetic territory'
        self.envelope["members"][2]["name"] = "Zoë " + payload
        self.envelope["members"][2]["qth"] = qth
        self.envelope["og_assignments"][1]["region_label"] = payload
        self.write_roster()
        self.assertEqual(self.build(), 0)
        site_contract.validate_dist(self.dist)
        self.assertEqual(self.data("tree.html", "DOWNLINE_DATA")[2]["name"], "Zoë " + payload)
        self.assertIn(qth, self.data("index.html", "MAP_DATA")["dx"])
        self.assertIn("&lt;/script&gt;", (self.dist / "index.html").read_text())
        for page in ("index.html", "tree.html", "nearby.html", "outbreak.html"):
            html = (self.dist / page).read_text()
            self.assertNotIn('<script>alert("synthetic")', html)
        self.assertEqual(json.loads((self.dist / "data/v2/roster.json").read_text()), self.envelope)

    def test_validator_rejects_unreferenced_photos(self):
        self.assertEqual(self.build(), 0)
        (self.dist / "images/mugshots" / ("a" * 64 + ".webp")).write_bytes(self.photo_bytes)
        with self.assertRaises(ValueError):
            site_contract.validate_dist(self.dist)


if __name__ == "__main__":
    unittest.main()
