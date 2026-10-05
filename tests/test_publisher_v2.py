"""Admin-owned v2 publication, with all network access replaced by fixtures."""

import contextlib
import copy
from email.message import Message
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import test_json_build as json_build
from test_roster_enrichment import png
from test_roster_input import FakeResponse, roster_input
import roster_assets
import roster_enrichment
from site_contract import validate_dist


SOURCE_URL = "https://admin.fixture.invalid/api/roster/export/v2"


def photo_fixture():
    data, _extension = roster_enrichment.sanitize_photo(png(metadata=True))
    digest = hashlib.sha256(data).hexdigest()
    return data, {"url": f"https://admin.fixture.invalid/api/roster/photos/{digest}.webp",
                  "sha256": digest, "content_type": "image/webp", "byte_length": len(data)}


class PublisherV2Tests(unittest.TestCase):
    fixture_response = json_build.JsonBuildTests.fixture_response
    write_roster = json_build.JsonBuildTests.write_roster
    data = json_build.JsonBuildTests.data

    def setUp(self):
        json_build.JsonBuildTests.setUp(self)
        self.envelope["schema_version"] = "2"
        for member in self.envelope["members"]:
            member.update(map_location=None, photo=None)
        self.envelope["members"][0]["map_location"] = {"grid": "EN61AB", "lat": 41.0625, "lon": -87.9583}
        self.assets = self.workspace / "public-assets"
        self.assets.mkdir()
        self.photo_bytes, self.photo = photo_fixture()
        self.envelope["members"][0]["photo"] = self.photo
        (self.assets / f"{self.photo['sha256']}.webp").write_bytes(self.photo_bytes)
        json_build.rehash(self.envelope)
        self.write_roster()

    def build(self, extra=()):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()), \
                patch.object(self.builder, "fetch_csv", side_effect=AssertionError("No Sheets")), \
                patch.object(self.builder, "annotate_qrz", side_effect=AssertionError("No QRZ")), \
                patch.object(self.builder, "load_name_overrides", side_effect=AssertionError("No name overrides")), \
                patch.object(self.builder, "load_location_overrides", side_effect=AssertionError("No location overrides")), \
                patch.object(roster_enrichment, "annotate_json_enrichment", side_effect=AssertionError("No enrichment")), \
                patch.object(roster_enrichment, "sanitize_photo", side_effect=AssertionError("No photo transformation")):
            return self.builder.main(["--source", "json", "--schema-version", "2",
                                      "--roster-json", str(self.roster), "--asset-dir", str(self.assets),
                                      "--output-dir", str(self.dist), *extra])

    def test_v2_publishes_exact_admin_locations_photos_and_snapshot_offline(self):
        self.assertEqual(self.build(), 0)
        validate_dist(self.dist, source="json")
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [
            {"call": "N0SYN1", "name": "Synthetic First", "num": 1,
             "grid": "EN61AB", "lat": 41.0625, "lon": -87.9583},
        ])
        self.assertEqual(json.loads((self.dist / "data/v2/roster.json").read_text()), self.envelope)
        self.assertFalse((self.dist / "data/v1").exists())
        public_photo = self.dist / f"images/mugshots/{self.photo['sha256']}.webp"
        self.assertEqual(public_photo.read_bytes(), self.photo_bytes)
        self.assertEqual(len(list((self.dist / "images/mugshots").iterdir())), 1)
        self.assertFalse((self.dist / "images/mugshots-override").exists())

    def test_upstream_v2_fixture_hash_and_full_precision_centers_publish_unchanged(self):
        fixtures = json_build.ROOT / "tests/fixtures/public-roster/fixtures"
        text = (fixtures / "roster-v2.json").read_text()
        envelope, members = roster_input.parse_roster_json(text, expected_version="2")
        self.assertEqual(envelope["content_hash"], "sha256:8857d8d9cfd7b57c22918780e189fea1139257817456607fbff6de089490e6a7")
        self.envelope = envelope
        self.write_roster()
        self.assertEqual(self.build(), 0)
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [
            dict(call=member["callsign"], name=member["name"], num=member["number"], **member["map_location"])
            for member in members if member["map_location"] is not None
        ])
        with self.assertRaisesRegex(ValueError, "no active members"):
            roster_input.parse_roster_json((fixtures / "empty-v2.json").read_text(), expected_version="2")

    def test_null_locations_never_recover_from_stale_enrichment(self):
        self.envelope["members"][0]["map_location"] = None
        json_build.rehash(self.envelope)
        self.write_roster()
        self.assertEqual(self.build(), 0)
        self.assertEqual(self.data("nearby.html", "GEO_DATA"), [])
        self.assertTrue(all("lat" not in row for row in self.data("outbreak.html", "OUTBREAK_DATA")))
        self.assertNotEqual(self.build(["--enrichment-dir", str(self.enrichment)]), 0)

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
        original = nearby.read_text()
        nearby.write_text(original.replace('"grid":"EN61AB"', '"grid":"EN61AC"'))
        with self.assertRaisesRegex(ValueError, "administration map locations"):
            validate_dist(self.dist, source="json")
        nearby.write_text(original)
        photo = self.dist / f"images/mugshots/{self.photo['sha256']}.webp"
        photo.write_bytes(self.photo_bytes + b"extra")
        with self.assertRaisesRegex(ValueError, "photo bytes"):
            validate_dist(self.dist, source="json")

    def test_v2_is_explicit_and_contract_rejects_private_malformed_values(self):
        text = json.dumps(self.envelope)
        with self.assertRaisesRegex(ValueError, "Unsupported"):
            roster_input.parse_roster_json(text)
        envelope, members = roster_input.parse_roster_json(text, expected_version="2")
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
                    roster_input.parse_roster_json(json.dumps(json_build.rehash(invalid)), expected_version="2")
        invalid = copy.deepcopy(self.envelope)
        invalid["members"][0]["photo"]["source_url"] = "private"
        with self.assertRaisesRegex(ValueError, "additional"):
            roster_input.parse_roster_json(json.dumps(json_build.rehash(invalid)), expected_version="2")
        invalid = copy.deepcopy(self.envelope)
        invalid["members"][0]["photo"]["byte_length"] += 1
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            roster_input.parse_roster_json(json.dumps(invalid), expected_version="2")


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
        chunk = b"XMP " + len(metadata).to_bytes(4, "little") + metadata + b"\x00"
        bad = self.data[:4] + (len(self.data) - 8 + len(chunk)).to_bytes(4, "little") + self.data[8:] + chunk
        photo = dict(self.photo, byte_length=len(bad), sha256=hashlib.sha256(bad).hexdigest())
        with self.assertRaises(ValueError):
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
