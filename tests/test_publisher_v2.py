"""Admin-owned v2 publication, with all network access replaced by fixtures."""

import copy
from email.message import Message
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from test_deployment import PublicationBuildFixture, SOURCE_URL, photo_fixture
from test_roster_input import FIXTURES, FakeResponse, rehash, roster_input
import roster_assets
from site_contract import validate_dist


class PublisherV2Tests(PublicationBuildFixture):
    def setUp(self):
        super().setUp()
        for member in self.envelope["members"]:
            member["map_location"] = None
        self.envelope["members"][0]["map_location"] = {"grid": "EN61AB", "lat": 41.0625, "lon": -87.9583}
        self.write_roster()

    def test_v2_publishes_exact_admin_locations_photos_and_snapshot_offline(self):
        self.assertEqual(self.build(), 0)
        validate_dist(self.dist)
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [
            {"call": "N0SYN1", "name": "Synthetic First", "num": 1,
             "grid": "EN61AB", "lat": 41.0625, "lon": -87.9583},
        ])
        self.assertEqual(self.data("nearby.html", "UNMAPPED_MEMBERS"), [
            {"call": row["callsign"], "num": row["bkg_number"]}
            for row in self.envelope["members"] if row["map_location"] is None
        ])
        self.assertEqual(json.loads((self.dist / "data/v2/roster.json").read_text(encoding="utf-8")), self.envelope)
        self.assertFalse((self.dist / "data/v1").exists())
        public_photo = self.dist / f"images/mugshots/{self.photo['sha256']}.webp"
        self.assertEqual(public_photo.read_bytes(), self.photo_bytes)
        self.assertEqual(len(list((self.dist / "images/mugshots").iterdir())), 1)
        self.assertFalse((self.dist / "images/mugshots-override").exists())

    def test_invalid_callsign_preserves_the_previous_site(self):
        self.assertEqual(self.build(), 0)
        previous = self.output_files(self.dist)
        self.envelope["members"][0]["callsign"] = "KCØDWZ"
        self.write_roster()
        self.assertEqual(self.build(), 1)
        validate_dist(self.dist)
        self.assertEqual(self.output_files(self.dist), previous)

    def test_upstream_v2_fixture_hash_and_full_precision_centers_publish_unchanged(self):
        fixtures = FIXTURES / "fixtures"
        text = (fixtures / "roster-v2.json").read_text(encoding="utf-8")
        envelope, members = roster_input.parse_roster_json(text)
        self.assertEqual(envelope["content_hash"], "sha256:8857d8d9cfd7b57c22918780e189fea1139257817456607fbff6de089490e6a7")
        self.envelope = envelope
        self.write_roster()
        self.assertEqual(self.build(), 0)
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [
            dict(call=member["callsign"], name=member["name"], num=member["number"], **member["map_location"])
            for member in members if member["map_location"] is not None
        ])
        with self.assertRaisesRegex(ValueError, "no active members"):
            roster_input.parse_roster_json((fixtures / "empty-v2.json").read_text(encoding="utf-8"))

    def test_null_location_removes_previous_coordinates(self):
        self.assertEqual(self.build(), 0)
        self.envelope["members"][0]["map_location"] = None
        self.write_roster()
        self.assertEqual(self.build(), 0)
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])
        self.assertTrue(all("lat" not in row for row in self.data("outbreak.html", "OUTBREAK_DATA")))

    def test_current_callsign_sponsor_ids_and_stored_ogs_survive_a_rename(self):
        self.envelope["members"][0]["callsign"] = "N0CURRENT"
        self.envelope["members"][0]["photo"] = None
        self.envelope["members"][0]["map_location"] = None
        self.write_roster()
        self.assertEqual(self.build(), 0)
        tree = self.data("tree.html", "DOWNLINE_DATA")
        self.assertEqual(next(m for m in tree if m["num"] == 7)["sponsor"], "N0CURRENT")
        self.assertEqual(json.loads((self.dist / "data/v2/roster.json").read_text(encoding="utf-8"))["og_assignments"],
                         self.envelope["og_assignments"])

    def test_reused_callsign_does_not_restore_inactive_sponsorship_or_og(self):
        self.envelope["members"] = [row for row in self.envelope["members"] if row["bkg_number"] != 7]
        self.envelope["og_assignments"] = [row for row in self.envelope["og_assignments"] if row["bkg_number"] != 7]
        members = {row["bkg_number"]: row for row in self.envelope["members"]}
        members[21]["sponsor_bkg_number"] = None
        members[40]["callsign"] = "N0SYN7"
        self.write_roster()
        self.assertEqual(self.build(), 0)
        tree = {entry["num"]: entry for entry in self.data("tree.html", "DOWNLINE_DATA")}
        self.assertNotIn(7, tree)
        self.assertIsNone(tree[21]["sponsor"])
        self.assertEqual(tree[40]["call"], "N0SYN7")
        self.assertNotIn("IL OG", (self.dist / "members.txt").read_text(encoding="utf-8"))

    def test_canadian_province_groups_under_canada_and_keeps_admin_coordinates(self):
        member = next(row for row in self.envelope["members"] if row["bkg_number"] == 21)
        member["qth"] = "Ontario"
        member["map_location"] = {"grid": "FN03", "lat": 43.5, "lon": -79}
        self.write_roster()
        self.assertEqual(self.build(), 0)
        canada = self.data("index.html", "MAP_DATA")["dx"]["Canada"]
        self.assertEqual([entry["num"] for entry in canada["members"]], [21])
        located = next(entry for entry in self.data("nearby.html", "GEO_DATA") if entry["num"] == 21)
        self.assertEqual((located["grid"], located["lat"], located["lon"]), ("FN03", 43.5, -79))
        outbreak = next(entry for entry in self.data("outbreak.html", "OUTBREAK_DATA") if entry["num"] == 21)
        self.assertEqual((outbreak["state"], outbreak["dx"]), (None, "Canada"))
        self.assertIn("BKG #21 (Canada OG)", (self.dist / "members.txt").read_text(encoding="utf-8"))

    def test_null_photo_removes_old_assets_and_offline_origin_is_checked(self):
        self.assertEqual(self.build(), 0)
        previous = (self.dist / "index.html").read_bytes()
        self.assertNotEqual(self.build(extra=("--asset-origin", "https://other.fixture.invalid/export")), 0)
        self.assertEqual((self.dist / "index.html").read_bytes(), previous)
        self.envelope["members"][0]["photo"] = None
        self.write_roster()
        self.assertEqual(self.build(), 0)
        self.assertFalse(list((self.dist / "images").rglob("*.webp")))

    def test_photo_integrity_failures_preserve_the_previous_build(self):
        self.assertEqual(self.build(), 0)
        previous = (self.dist / "index.html").read_bytes()
        asset = self.assets / f"{self.photo['sha256']}.webp"
        for bad in (b"", b"<html>unavailable</html>", self.photo_bytes + b"extra"):
            with self.subTest(bad=bad[:10]):
                asset.write_bytes(bad)
                self.assertNotEqual(self.build(), 0)
                self.assertEqual((self.dist / "index.html").read_bytes(), previous)
        asset.unlink()
        self.assertNotEqual(self.build(), 0)
        asset.symlink_to(self.roster)
        self.assertNotEqual(self.build(), 0)

    def test_validator_checks_location_and_photo_against_admin_snapshot(self):
        self.assertEqual(self.build(), 0)
        nearby = self.dist / "nearby.html"
        original = nearby.read_text(encoding="utf-8")
        nearby.write_text(original.replace('"grid":"EN61AB"', '"grid":"EN61AC"'), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "administration map locations"):
            validate_dist(self.dist)
        nearby.write_text(original, encoding="utf-8")
        photo = self.dist / f"images/mugshots/{self.photo['sha256']}.webp"
        photo.write_bytes(self.photo_bytes + b"extra")
        with self.assertRaisesRegex(ValueError, "photo bytes"):
            validate_dist(self.dist)

    def test_contract_rejects_invalid_or_unpublished_location_and_photo_metadata(self):
        text = json.dumps(self.envelope)
        envelope, members = roster_input.parse_roster_json(text)
        self.assertEqual(envelope, self.envelope)
        self.assertIsNone(members[1]["map_location"])
        for target, key, value in (
            ("map_location", "grid", "en61ab"), ("map_location", "grid", "EN61AB12"),
            ("map_location", "lat", True), ("map_location", "lon", 181),
            ("photo", "byte_length", True), ("photo", "byte_length", 1024 * 1024 + 1),
            ("photo", "sha256", "bad"), ("photo", "content_type", "image/png"),
            ("photo", "url", "https://qrz.fixture.invalid/image.jpg"),
            ("photo", "url", self.photo["url"] + "?token=private"),
            ("photo", "url", self.photo["url"] + "#fragment"),
            ("photo", "url", self.photo["url"].replace("https://", "http://")),
        ):
            with self.subTest(target=target, key=key, value=value):
                invalid = copy.deepcopy(self.envelope)
                invalid["members"][0][target][key] = value
                with self.assertRaises(ValueError):
                    roster_input.parse_roster_json(rehash(invalid))
        invalid = copy.deepcopy(self.envelope)
        invalid["members"][0]["photo"]["source_url"] = "private"
        with self.assertRaisesRegex(ValueError, "additional"):
            roster_input.parse_roster_json(rehash(invalid))
        invalid = copy.deepcopy(self.envelope)
        invalid["members"][0]["callsign_history"] = ["N0FORMER"]
        with self.assertRaisesRegex(ValueError, "additional"):
            roster_input.parse_roster_json(rehash(invalid))
        invalid = copy.deepcopy(self.envelope)
        invalid["members"][0]["photo"]["byte_length"] += 1
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            roster_input.parse_roster_json(json.dumps(invalid))


class PublicAssetTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.assets = self.root / "assets"
        self.data, self.photo = photo_fixture()
        self.members = [{"photo": self.photo}]

    def fetch(self):
        return roster_assets.fetch_assets(self.members, self.assets, source_url=SOURCE_URL, repo_root=self.repo)

    def test_fetch_verifies_exact_bytes_and_reuses_only_matching_immutable_photos(self):
        with patch.object(roster_assets, "_download", return_value=self.data) as download:
            result = self.fetch()
            self.assertEqual(result["downloaded"], 1)
            self.assertEqual((self.assets / f"{self.photo['sha256']}.webp").read_bytes(), self.data)
            self.assertEqual(self.fetch()["downloaded"], 0)
            download.assert_called_once()
            (self.assets / ("a" * 64 + ".webp")).write_bytes(self.data)
            self.fetch()
            self.assertFalse((self.assets / ("a" * 64 + ".webp")).exists())

    def test_all_origins_are_validated_before_any_fetch_or_cache_reuse(self):
        self.members.append({"photo": dict(self.photo, sha256="a" * 64,
                                           url=f"https://other.fixture.invalid/api/roster/photos/{'a' * 64}.webp")})
        with patch.object(roster_assets, "_download") as download:
            with self.assertRaisesRegex(ValueError, "snapshot origin"):
                self.fetch()
            download.assert_not_called()
        self.assertFalse(self.assets.exists())

    def test_transport_redirect_and_wrong_type_fail_without_exposing_urls(self):
        headers = Message()
        headers["Content-Type"] = "image/webp"
        response = FakeResponse(self.data)
        response.headers = headers
        with patch.object(roster_assets.urllib.request, "build_opener") as opener:
            opener.return_value.open.return_value = response
            self.assertEqual(roster_assets._download(self.photo), self.data)
            handler = opener.call_args.args[0]
            self.assertIsNone(handler.redirect_request(None, None, 302, "Found", {}, "https://other.invalid"))
            headers.replace_header("Content-Type", "text/html")
            with self.assertRaisesRegex(OSError, "validated public photo") as caught:
                roster_assets._download(self.photo)
            self.assertNotIn("fixture.invalid", str(caught.exception))

    def test_verified_digest_still_rejects_untrusted_webp_metadata(self):
        metadata = b"private metadata"
        pad = b"\x00" * (len(metadata) % 2)
        chunk = b"XMP " + len(metadata).to_bytes(4, "little") + metadata + pad
        bad = self.data[:4] + (len(self.data) - 8 + len(chunk)).to_bytes(4, "little") + self.data[8:] + chunk
        photo = dict(self.photo, byte_length=len(bad), sha256=hashlib.sha256(bad).hexdigest())
        with self.assertRaisesRegex(ValueError, "Unusable public WebP photo"):
            roster_assets.verify_asset(bad, photo)

    def test_lossless_admin_webp_is_accepted_without_reencoding(self):
        from PIL import Image
        output = io.BytesIO()
        Image.new("RGBA", (2, 3), (255, 0, 0, 64)).save(output, format="WEBP", lossless=True)
        data = output.getvalue()
        self.assertIn(b"VP8L", data)
        photo = dict(self.photo, byte_length=len(data), sha256=hashlib.sha256(data).hexdigest())
        roster_assets.verify_asset(data, photo)

    def test_failed_download_and_final_swap_preserve_existing_cache(self):
        with patch.object(roster_assets, "_download", return_value=self.data):
            self.fetch()
        existing = self.assets / f"{self.photo['sha256']}.webp"
        existing.write_bytes(b"invalid cached bytes")
        with patch.object(roster_assets, "_download", side_effect=OSError("private URL")):
            with self.assertRaises(OSError):
                self.fetch()
        self.assertEqual(existing.read_bytes(), b"invalid cached bytes")
        existing.write_bytes(self.data)
        original_rename = Path.rename

        def fail_publish(path, target):
            if path.name == "assets" and path.parent.name.startswith(".assets-"):
                raise OSError("fixture publish failure")
            return original_rename(path, target)

        with patch.object(Path, "rename", autospec=True, side_effect=fail_publish):
            with self.assertRaises(OSError):
                self.fetch()
        self.assertEqual(existing.read_bytes(), self.data)


if __name__ == "__main__":
    unittest.main()
