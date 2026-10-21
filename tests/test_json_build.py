"""Offline source comparisons and publication boundaries for the JSON builder."""

import base64
import contextlib
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import re
import unittest
from unittest.mock import patch

import test_deployment as deployment


ROOT = deployment.ROOT
FIXTURES = deployment.FIXTURES / "public-roster" / "fixtures"
GIF = base64.b64decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==")


def rehash(envelope):
    payload = {key: envelope[key] for key in ("schema_version", "members", "og_assignments")}
    encoded = json.dumps(payload, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()
    envelope["content_hash"] = "sha256:" + hashlib.sha256(encoded).hexdigest()
    return envelope


def qth_hash(qth):
    return hashlib.sha256(qth.encode("utf-8")).hexdigest()


class JsonBuildTests(unittest.TestCase):
    # Reuse the temporary source tree and network guards without inheriting
    # DeploymentTests (which would run that suite a second time in discovery).
    fixture_response = deployment.DeploymentTests.fixture_response

    def setUp(self):
        deployment.DeploymentTests.setUp(self)
        self.envelope = json.loads((FIXTURES / "roster-v1.json").read_text())
        self.roster = self.workspace / "roster.json"
        self.write_roster()
        self.enrichment = self.workspace / "enrichment"
        self.enrichment.mkdir()
        self.matched_geo = {}
        self.source.joinpath("name-overrides.txt").write_text("")
        self.source.joinpath("location-overrides.txt").write_text("")

    def write_roster(self):
        self.roster.write_text(json.dumps(self.envelope, ensure_ascii=False))

    def build_json(self, *, output=None, extra=()):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            with patch.object(self.builder, "fetch_csv", side_effect=AssertionError("JSON must not fetch Sheets")), patch.object(
                self.builder, "annotate_qrz", side_effect=AssertionError("Rendering must not refresh QRZ")
            ):
                return self.builder.main([
                    "--source", "json", "--roster-json", str(self.roster),
                    "--enrichment-dir", str(self.enrichment),
                    "--output-dir", str(output or self.dist), *extra,
                ])

    def data(self, page, marker, *, output=None):
        text = ((output or self.dist) / page).read_text()
        return json.loads(re.search(rf"<!-- {marker}:START -->(.*?)<!-- {marker}:END -->", text, re.DOTALL)[1])

    def notes(self, output=None):
        return [line for line in ((output or self.dist) / "members.txt").read_text().splitlines() if line and not line.startswith("#")]

    def cards(self, output=None, *, omit_og=False):
        text = ((output or self.dist) / "index.html").read_text()
        block = deployment.site_contract.block(text, deployment.site_contract.ROSTER_START, "<!-- ROSTER:END -->")
        if omit_og:
            block = re.sub(r'\s*<div class="state-og-badge"[^>]*>.*?</div>', "", block)
        return block

    def matched_csv(self):
        text = io.StringIO()
        writer = csv.writer(text)
        writer.writerow(["#", "Callsign", "Name", "Join Date", "QTH", "Sponsor"])
        for member in self.envelope["members"]:
            writer.writerow([member["bkg_number"], member["callsign"], member["name"], member["qso_date"], member["qth"], member["sponsor_bkg_number"]])
        return text.getvalue()

    def sheet_qth_only(self, members):
        for member in members:
            member["state"], member["country"] = self.builder.qth_location(member["qth"])
            member.update(state_og=False, country_og=False, grid=None, lat=None, lon=None, mugshot_path=None)
            member.update(self.matched_geo.get(member["number"], {}))
        self.builder.mark_territory_ogs(members)

    def build_matched_sheets(self, output):
        with patch.object(self.builder, "fetch_csv", return_value=self.matched_csv()), patch.object(
            self.builder, "annotate_qrz", side_effect=self.sheet_qth_only
        ), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return self.builder.main(["--output-dir", str(output)])

    def write_enrichment(self, members):
        (self.enrichment / "enrichment.json").write_text(json.dumps({"version": 1, "members": members}))

    def cache_photo(self, number, callsign):
        from roster_enrichment import sanitize_photo

        sanitized, extension = sanitize_photo(GIF)
        self.assertEqual(extension, ".webp")
        filename = f"bkg-{number}-{hashlib.sha256(callsign.encode()).hexdigest()[:12]}{extension}"
        photos = self.enrichment / "photos"
        photos.mkdir(exist_ok=True)
        (photos / filename).write_bytes(sanitized)
        return filename

    def test_synthetic_export_matches_all_existing_sheet_outputs_with_reviewed_differences(self):
        sheets = self.workspace / "sheets-dist"
        self.assertEqual(self.build_matched_sheets(sheets), 0)
        self.assertEqual(self.build_json(), 0)
        deployment.site_contract.validate_dist(sheets)
        deployment.site_contract.validate_dist(self.dist, source="json")

        # Every graph, map, and date consumer receives the same membership
        # projection. Unknown enrichment does not remove anyone from the map.
        for page, marker in (("index.html", "MAP_DATA"), ("tree.html", "DOWNLINE_DATA"), ("nearby.html", "GEO_DATA"), ("outbreak.html", "OUTBREAK_DATA")):
            with self.subTest(output=marker):
                self.assertEqual(self.data(page, marker), self.data(page, marker, output=sheets))
        self.assertEqual(self.cards(omit_og=True), self.cards(sheets, omit_og=True))
        self.assertEqual(len(self.data("tree.html", "DOWNLINE_DATA")), 6)
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])
        territory = self.data("index.html", "MAP_DATA")
        self.assertEqual(set(territory["states"]), {"IL", "NY"})
        self.assertEqual(len(territory["dx"]["Canada"]["members"]), 1)

        # Sheets infers current-territory OGs; JSON preserves reviewed stored
        # assignments, including VT's holder after moving to New York.
        self.assertIn('title="NY OG"', self.cards(sheets))
        self.assertIn('title="VT OG"', self.cards())
        self.assertNotIn('title="NY OG"', self.cards())
        self.assertEqual(self.notes(), [
            "K2SYN88 🤜 Synthetic Moved BKG #88 (VT OG)",
            "N0SYN1 🤜 Synthetic First BKG #1",
            "N0SYN12 🤜 Zoë Example BKG #12",
            "N0SYN40 🤜 Synthetic Orphan BKG #40",
            "N0SYN7 🤜 Synthetic Reviewed BKG #7 (IL OG)",
            "VE3SYN 🤜 Synthetic Canada BKG #21 (Canada OG)",
        ])
        self.assertEqual(self.notes(sheets), [
            "K2SYN88 🤜 Synthetic M BKG #88 (NY OG)",
            "N0SYN1 🤜 Synthetic F BKG #1 (OG, IL OG)",
            "N0SYN12 🤜 Zoë E BKG #12",
            "N0SYN40 🤜 Synthetic O BKG #40",
            "N0SYN7 🤜 Synthetic R BKG #7",
            "VE3SYN 🤜 Synthetic C BKG #21 (Canada OG)",
        ])
        public_contract = json.loads((self.dist / "data/v1/roster.json").read_text())
        self.assertEqual(public_contract, self.envelope)
        self.assertFalse((sheets / "data").exists())
        # Ordinary production validation cannot admit experimental JSON data.
        with self.assertRaises(ValueError):
            deployment.site_contract.validate_dist(self.dist)

    def test_reviewed_identity_sponsor_and_qso_dates_ignore_legacy_overrides(self):
        self.source.joinpath("name-overrides.txt").write_text("N0SYN7 = Unreviewed Nickname\n")
        self.source.joinpath("location-overrides.txt").write_text("K2SYN88 = Vermont\n")
        self.assertEqual(self.build_json(), 0)
        tree = self.data("tree.html", "DOWNLINE_DATA")
        self.assertEqual([entry["name"] for entry in tree], [member["name"] for member in self.envelope["members"]])
        self.assertEqual([entry["sponsor"] for entry in tree], [None, "N0SYN1", None, "N0SYN7", None, "VE3SYN"])
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")
        self.assertEqual([entry["date"] for entry in outbreak], ["2026-08-21", "2026-08-31", None, "2026-08-21", "2026-08-25", "2026-07-02"])
        self.assertEqual(outbreak[-1]["state"], "NY")
        self.assertNotIn("2026-09-29", [entry["date"] for entry in outbreak])
        self.assertEqual(json.loads((self.dist / "data/v1/roster.json").read_text())["members"], self.envelope["members"])

    def test_partial_enrichment_adds_only_coarse_geo_and_active_sanitized_photo(self):
        filename = self.cache_photo(1, "N0SYN1")
        inactive_photo = self.cache_photo(99, "N0INACTIVE")
        self.write_enrichment({
            "1": {"callsign": "N0SYN1", "qth_hash": qth_hash("Illinois"), "grid": "EN61", "photo": filename},
            "7": {"callsign": "N0SYN7", "qth_hash": qth_hash("Illinois"), "lat": 41.8841, "lon": -87.6287},
            "21": {"callsign": "VE3SYN", "qth_hash": qth_hash("Canada"), "grid": "FN03"},
            "99": {"callsign": "N0INACTIVE", "name": "Inactive Member", "grid": "FN31", "photo": inactive_photo},
        })
        self.assertEqual(self.build_json(), 0)
        deployment.site_contract.validate_dist(self.dist, source="json")
        self.assertEqual([entry["num"] for entry in self.data("nearby.html", "GEO_DATA")], [1, 7, 21])
        self.assertEqual((self.data("nearby.html", "GEO_DATA")[1]["lat"], self.data("nearby.html", "GEO_DATA")[1]["lon"]), (41.88, -87.63))
        self.assertEqual([entry["num"] for entry in self.data("tree.html", "DOWNLINE_DATA")], [1, 7, 12, 21, 40, 88])
        photos = [path.relative_to(self.dist).as_posix() for path in self.dist.rglob("*") if path.is_file() and path.suffix == ".webp"]
        self.assertEqual(photos, ["images/mugshots/bkg-1.webp"])
        for path in self.dist.rglob("*"):
            if path.is_file() and path.suffix in {".html", ".txt", ".json"}:
                self.assertNotIn("N0INACTIVE", path.read_text())
        self.assertFalse((self.dist / "enrichment.json").exists())
        self.assertFalse((self.dist / "images/mugshots/.sources.json").exists())

        self.matched_geo = {1: {"grid": "EN61"}, 7: {"lat": 41.8841, "lon": -87.6287}, 21: {"grid": "FN03"}}
        sheets = self.workspace / "enriched-sheets-dist"
        self.assertEqual(self.build_matched_sheets(sheets), 0)
        for page, marker in (("index.html", "MAP_DATA"), ("tree.html", "DOWNLINE_DATA"), ("nearby.html", "GEO_DATA"), ("outbreak.html", "OUTBREAK_DATA")):
            with self.subTest(enriched_output=marker):
                self.assertEqual(self.data(page, marker), self.data(page, marker, output=sheets))

    def test_changed_callsign_keeps_stable_sponsor_and_invalidates_old_enrichment(self):
        self.write_enrichment({"7": {"callsign": "N0SYN7", "qth_hash": qth_hash("Illinois"), "grid": "EN61"}})
        self.envelope["members"][1]["callsign"] = "K7SYN"
        rehash(self.envelope)
        self.write_roster()
        self.assertEqual(self.build_json(), 0)
        tree = self.data("tree.html", "DOWNLINE_DATA")
        self.assertEqual(tree[1]["call"], "K7SYN")
        self.assertEqual(tree[3]["sponsor"], "K7SYN")
        self.assertEqual(tree[1]["sponsor"], "N0SYN1")
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])
        self.assertNotIn("N0SYN7", (self.dist / "members.txt").read_text())
        self.assertIn("K7SYN 🤜 Synthetic Reviewed BKG #7 (IL OG)", self.notes())

    def test_inactive_holder_and_sponsor_remain_suppressed_after_callsign_reuse(self):
        self.envelope["members"] = [member for member in self.envelope["members"] if member["bkg_number"] != 7]
        self.envelope["og_assignments"] = [assignment for assignment in self.envelope["og_assignments"] if assignment["bkg_number"] != 7]
        for member in self.envelope["members"]:
            if member["bkg_number"] == 21:
                member["sponsor_bkg_number"] = None
            if member["bkg_number"] == 40:
                member["callsign"] = "N0SYN7"
        self.write_enrichment({"7": {"callsign": "N0SYN7", "qth_hash": qth_hash("Illinois"), "grid": "EN61"}})
        rehash(self.envelope)
        self.write_roster()
        self.assertEqual(self.build_json(), 0)
        tree = self.data("tree.html", "DOWNLINE_DATA")
        self.assertEqual([entry["num"] for entry in tree], [1, 12, 21, 40, 88])
        self.assertIsNone(next(entry for entry in tree if entry["num"] == 21)["sponsor"])
        self.assertEqual(next(entry for entry in tree if entry["num"] == 40)["call"], "N0SYN7")
        self.assertEqual(self.data("index.html", "MAP_DATA")["states"]["IL"][0]["num"], 1)
        self.assertNotIn('title="IL OG"', self.cards())
        self.assertNotIn("IL OG", "\n".join(self.notes()))
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])

    def test_moved_qth_suppresses_old_coordinates_but_retains_photo_and_stored_og(self):
        filename = self.cache_photo(88, "K2SYN88")
        self.write_enrichment({"88": {
            "callsign": "K2SYN88", "qth_hash": qth_hash("Vermont"), "grid": "FN33", "photo": filename,
        }})
        self.assertEqual(self.build_json(), 0)
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])
        outbreak = self.data("outbreak.html", "OUTBREAK_DATA")[-1]
        self.assertEqual(outbreak["state"], "NY")
        self.assertNotIn("lat", outbreak)
        self.assertTrue((self.dist / "images/mugshots/bkg-88.webp").is_file())
        self.assertIn("K2SYN88 🤜 Synthetic Moved BKG #88 (VT OG)", self.notes())

    def test_canadian_province_qth_files_under_canada_and_keeps_matched_geography(self):
        for member in self.envelope["members"]:
            if member["bkg_number"] == 21:
                member["qth"] = "Alberta"
        rehash(self.envelope)
        self.write_roster()
        self.write_enrichment({"21": {"callsign": "VE3SYN", "qth_hash": qth_hash("Alberta"), "grid": "DO21"}})
        self.assertEqual(self.build_json(), 0)
        deployment.site_contract.validate_dist(self.dist, source="json")
        territory = self.data("index.html", "MAP_DATA")
        self.assertEqual(list(territory["dx"]), ["Canada"])
        self.assertEqual(territory["dx"]["Canada"]["flag"], "🇨🇦")
        self.assertEqual([entry["num"] for entry in territory["dx"]["Canada"]["members"]], [21])
        self.assertEqual([entry["num"] for entry in self.data("nearby.html", "GEO_DATA")], [21])
        outbreak = next(entry for entry in self.data("outbreak.html", "OUTBREAK_DATA") if entry["num"] == 21)
        self.assertEqual((outbreak["state"], outbreak["dx"]), (None, "Canada"))
        self.assertEqual(self.builder.qth_location("Alberta"), ("AB", "Canada"))
        self.assertEqual(self.builder.qth_location("Canada"), (None, "Canada"))
        # Stored OG labels are published as reviewed, never re-derived from the QTH.
        self.assertIn("VE3SYN 🤜 Synthetic Canada BKG #21 (Canada OG)", self.notes())

    def test_reviewed_html_and_script_text_round_trips_without_execution(self):
        payload = '</script><script>alert("synthetic")</script>&'
        qth = payload + '\u2028\u2029Synthetic territory'
        self.envelope["members"][2]["name"] = "Zoë " + payload
        self.envelope["members"][2]["qth"] = qth
        self.envelope["og_assignments"][1]["region_label"] = payload
        rehash(self.envelope)
        self.write_roster()
        self.assertEqual(self.build_json(), 0)
        deployment.site_contract.validate_dist(self.dist, source="json")
        self.assertEqual(self.data("tree.html", "DOWNLINE_DATA")[2]["name"], "Zoë " + payload)
        self.assertIn(qth, self.data("index.html", "MAP_DATA")["dx"])
        self.assertIn("&lt;/script&gt;", self.cards())
        for page in ("index.html", "tree.html", "nearby.html", "outbreak.html"):
            html = (self.dist / page).read_text()
            self.assertNotIn('<script>alert("synthetic")', html)
        self.assertEqual(json.loads((self.dist / "data/v1/roster.json").read_text()), self.envelope)

    def test_missing_or_unreadable_enrichment_cannot_remove_roster_or_create_demo_members(self):
        self.assertEqual(self.build_json(), 0)
        for cache in (None, "not json", '{"version":1,"members":[]}'):
            with self.subTest(cache=cache):
                target = self.enrichment / "enrichment.json"
                if cache is None:
                    target.unlink(missing_ok=True)
                else:
                    target.write_text(cache)
                status = self.build_json()
                # A bad cache may be ignored or fail closed. Either way it
                # cannot replace a previous successful roster with an empty one.
                if status:
                    self.assertTrue((self.dist / "index.html").is_file())
                self.assertEqual([entry["num"] for entry in self.data("tree.html", "DOWNLINE_DATA")], [1, 7, 12, 21, 40, 88])
                self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])
                self.assertEqual(len(self.notes()), 6)

    def test_fingerprint_ignores_build_times_but_not_visible_changes(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location("site_fingerprint", ROOT / "scripts/site-fingerprint.py")
        fingerprint = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fingerprint)
        later = self.workspace / "later"
        self.assertEqual(self.build_json(), 0)
        self.envelope["generated_at"] = "2026-12-31T23:59:59Z"
        self.write_roster()
        with patch.object(self.builder, "datetime", wraps=self.builder.datetime) as clock:
            clock.now.return_value = self.builder.datetime(2030, 1, 2, 3, 4, 5, tzinfo=self.builder.timezone.utc)
            self.assertEqual(self.build_json(output=later), 0)
        self.assertNotEqual((self.dist / "index.html").read_bytes(), (later / "index.html").read_bytes())
        self.assertNotEqual((self.dist / "members.txt").read_bytes(), (later / "members.txt").read_bytes())
        self.assertEqual(fingerprint.fingerprint(self.dist), fingerprint.fingerprint(later))
        self.envelope["members"][0]["name"] = "Renamed Member"
        rehash(self.envelope)
        self.write_roster()
        self.assertEqual(self.build_json(output=later), 0)
        self.assertNotEqual(fingerprint.fingerprint(self.dist), fingerprint.fingerprint(later))

    def test_json_validator_rejects_unreferenced_inactive_photos(self):
        self.assertEqual(self.build_json(), 0)
        filename = self.cache_photo(99, "N0INACTIVE")
        photo = self.dist / "images/mugshots/bkg-99.webp"
        photo.parent.mkdir(parents=True, exist_ok=True)
        photo.write_bytes((self.enrichment / "photos" / filename).read_bytes())
        with self.assertRaises(ValueError):
            deployment.site_contract.validate_dist(self.dist, source="json")

    def test_malformed_or_empty_contract_preserves_previous_output(self):
        self.assertEqual(self.build_json(), 0)
        previous = {path.relative_to(self.dist): path.read_bytes() for path in self.dist.rglob("*") if path.is_file()}
        missing_sponsor = json.loads(json.dumps(self.envelope))
        missing_sponsor["members"][1]["sponsor_bkg_number"] = 99
        rehash(missing_sponsor)
        leaked_private = json.loads(json.dumps(self.envelope))
        leaked_private["members"][0]["email"] = "private@example.invalid"
        rehash(leaked_private)
        wrong_hash = json.loads(json.dumps(self.envelope))
        wrong_hash["members"][0]["name"] = "Fictional Fallback"
        invalid = ["", "{", "{}", json.dumps(missing_sponsor), json.dumps(leaked_private), json.dumps(wrong_hash), (FIXTURES / "empty-v1.json").read_text()]
        for raw in invalid:
            with self.subTest(input=raw):
                self.roster.write_text(raw)
                self.assertNotEqual(self.build_json(), 0)
                self.assertEqual({path.relative_to(self.dist): path.read_bytes() for path in self.dist.rglob("*") if path.is_file()}, previous)

    def test_json_can_build_production_dist_but_cannot_replace_source(self):
        production = self.source / "dist"
        self.assertEqual(self.build_json(output=production), 0)
        deployment.site_contract.validate_dist(production, source="json")
        for output in (self.source / "experimental", self.source, self.source.parent):
            with self.subTest(output=output):
                self.assertNotEqual(self.build_json(output=output), 0)
        self.assertTrue((production / "data/v1/roster.json").is_file())
        self.assertFalse((self.source / "experimental").exists())
        self.assertTrue(self.roster.is_file())
        previous = {p.relative_to(production): p.read_bytes() for p in production.rglob("*") if p.is_file()}
        self.roster.write_text((FIXTURES / "empty-v1.json").read_text())
        self.assertNotEqual(self.build_json(output=production), 0)
        self.assertEqual({p.relative_to(production): p.read_bytes() for p in production.rglob("*") if p.is_file()}, previous)

    def test_explicit_legacy_build_and_production_json_pages_workflow(self):
        sheets = self.workspace / "default-dist"
        with patch.dict(os.environ, {"ROSTER_EXPORT_URL": "https://fixture.invalid/api/roster/export"}):
            self.assertEqual(self.build_matched_sheets(sheets), 0)
        self.assertFalse((sheets / "data/v1/roster.json").exists())
        workflow = (ROOT / ".github/workflows/deploy.yml").read_text()
        self.assertIn("python3 scripts/build-roster.py --source json", workflow)
        self.assertIn("python3 scripts/validate-site.py --source json dist", workflow)
        self.assertIn("path: dist", workflow)
        self.assertIn("cron: '23 * * * *'", workflow)
        self.assertIn("--max-age-hours 24 --interval-hours 1", workflow)
        self.assertIn("python3 scripts/site-fingerprint.py dist", workflow)
        self.assertIn("if: needs.build.outputs.changed == 'true'", workflow)
        self.assertIn("actions/upload-pages-artifact@v5", workflow)
        self.assertIn("actions/deploy-pages@v5", workflow)
        self.assertIn("ROSTER_EXPORT_URL: ${{ secrets.ROSTER_EXPORT_URL }}", workflow)
        self.assertEqual(workflow.count("python3 scripts/fetch-roster.py"), 1)
        self.assertEqual(workflow.count('--roster-json "$RUNNER_TEMP/bkg-build/roster.json"'), 3)
        self.assertIn('ref: ${{ github.sha }}', workflow)
        self.assertIn('retention-days: 30', workflow)
        self.assertIn('--require-usable', workflow)
        self.assertNotIn('Commit refreshed QRZ cache', workflow)
        self.assertNotIn('CF_ACCESS_CLIENT', workflow)
        self.assertNotIn('images/mugshots\n', workflow)
        self.assertEqual((sheets / "CNAME").read_text().strip(), "www.bkg.club")


if __name__ == "__main__":
    unittest.main()
