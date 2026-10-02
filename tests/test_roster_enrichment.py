"""Offline tests for the optional JSON cache; no live QRZ or roster endpoint."""

import contextlib
import copy
import importlib.util
import io
import json
from pathlib import Path
import socket
import struct
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch
import urllib.request
import zlib


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import roster_enrichment as enrichment


def png_chunk(kind, payload):
    return (len(payload).to_bytes(4, "big") + kind + payload
            + zlib.crc32(kind + payload).to_bytes(4, "big"))


def png(*, metadata=False):
    parts = [b"\x89PNG\r\n\x1a\n", png_chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))]
    if metadata:
        description = b"private-gps-coordinates\x00"
        exif = (b"II*\x00\x08\x00\x00\x00" + struct.pack("<H", 1)
                + struct.pack("<HHII", 0x010E, 2, len(description), 26)
                + b"\x00\x00\x00\x00" + description)
        parts += [png_chunk(b"tEXt", b"Comment\x00private-home-address"),
                  png_chunk(b"eXIf", exif)]
    return b"".join(parts + [png_chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00")), png_chunk(b"IEND", b"")])


GIF = bytes.fromhex("47494638396101000100800000000000ffffff2c00000000010001000002024401003b")


class EnrichmentTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.workspace = Path(temporary.name)
        self.root = self.workspace / "source"
        self.root.mkdir()
        self.cache = self.workspace / "enrichment"
        self.members = [self.member(1, "K1TST"), self.member(2, "K2TST")]
        self.login = Mock(return_value="private-session")
        self.lookup = Mock(side_effect=lambda _session, call: {"current_call": call})
        self.photo_fetch = Mock(return_value=png(metadata=True))
        for replacement in (
            patch.object(urllib.request, "urlopen", side_effect=AssertionError("Network forbidden")),
            patch.object(socket, "create_connection", side_effect=AssertionError("Network forbidden")),
            patch.object(socket.socket, "connect", side_effect=AssertionError("Network forbidden")),
        ):
            replacement.start()
            self.addCleanup(replacement.stop)

    def member(self, number, callsign):
        return {"number": number, "callsign": callsign, "name": "Reviewed identity",
                "qth": "Reviewed QTH", "join_date": "2026-01-01", "sponsor_bkg_number": None,
                "og_regions": [{"kind": "state", "value": "UT"}]}

    def write_cache(self, records):
        self.cache.mkdir(parents=True, exist_ok=True)
        (self.cache / "photos").mkdir(exist_ok=True)
        (self.cache / enrichment.CACHE_NAME).write_text(json.dumps({"version": 1, "members": records}))

    def seed_cache(self):
        filename = enrichment.photo_filename(1, "K1TST", ".png")
        self.write_cache({
            "1": {"callsign": "K1TST", "grid": "FN31", "photo": filename,
                  "qth_hash": enrichment.location_binding("Reviewed QTH")},
            "2": {"callsign": "K2TST", "lat": 40.123456, "lon": -110.654321,
                  "qth_hash": enrichment.location_binding("Reviewed QTH")},
            "99": {"callsign": "K9OLD", "grid": "EM12", "photo": "inactive.png"},
        })
        (self.cache / "photos" / filename).write_bytes(png())
        (self.cache / "photos" / "inactive.png").write_bytes(png())
        return filename

    def read(self, members=None):
        return enrichment.read_enrichment(self.cache, members or self.members, repo_root=self.root)

    def refresh(self, members=None):
        return enrichment.refresh_enrichment(
            members if members is not None else self.members, self.cache, username="private-user",
            password="private-password", login=self.login, lookup=self.lookup,
            photo_fetch=self.photo_fetch, repo_root=self.root,
        )

    def test_render_reads_only_active_reviewed_binding_and_preserves_contract(self):
        filename = self.seed_cache()
        original = copy.deepcopy(self.members)
        before = (self.cache / enrichment.CACHE_NAME).read_bytes()
        photos = enrichment.annotate_json_enrichment(self.members, self.cache, repo_root=self.root)
        self.assertEqual(photos, {"images/mugshots/bkg-1.png": (self.cache / "photos" / filename).resolve()})
        self.assertEqual(self.members[0]["grid"], "FN31")
        self.assertEqual((self.members[1]["lat"], self.members[1]["lon"]), (40.12, -110.65))
        for old, member in zip(original, self.members):
            self.assertEqual({key: member[key] for key in old}, old)
        self.assertEqual((self.cache / enrichment.CACHE_NAME).read_bytes(), before)
        self.login.assert_not_called()

    def test_no_cache_and_corrupt_cache_keep_members_without_enrichment(self):
        for directory in (None, self.cache):
            members = copy.deepcopy(self.members)
            self.assertEqual(enrichment.annotate_json_enrichment(members, directory, repo_root=self.root), {})
            self.assertEqual(len(members), 2)
            self.assertTrue(all(m["mugshot_path"] is None and m["grid"] is None for m in members))
        self.write_cache({})
        (self.cache / enrichment.CACHE_NAME).write_text("{broken")
        self.assertEqual(self.read(), {})

    def test_changed_callsign_rejects_previous_member_binding_without_rewriting_identity(self):
        self.seed_cache()
        members = [self.member(1, "n0Changed/P"), self.member(2, "K2TST")]
        self.assertNotIn("1", self.read(members))
        self.lookup.side_effect = lambda _session, call: {"current_call": call.upper(), "grid": "EN61AB"}
        self.refresh(members)
        self.assertEqual(self.read(members)["1"], {"callsign": "n0Changed/P", "grid": "EN61",
                                                "qth_hash": enrichment.location_binding("Reviewed QTH")})
        self.assertEqual(members[0]["callsign"], "n0Changed/P")
        self.assertNotIn("photo", self.read(members)["1"])

    def test_reviewed_qth_move_suppresses_stale_coordinates_but_retains_photo(self):
        filename = self.seed_cache()
        members = copy.deepcopy(self.members)
        members[0]["qth"] = "Reviewed new QTH"
        record = self.read(members)["1"]
        self.assertEqual(record, {"callsign": "K1TST", "photo": filename})
        self.login.return_value = None
        self.refresh(members)
        self.assertEqual(self.read(members)["1"], record)
        self.login.return_value = "session"
        self.lookup.side_effect = lambda _session, call: {"current_call": call, "grid": "EN61AB"}
        self.refresh(members)
        self.assertEqual(self.read(members)["1"]["qth_hash"], enrichment.location_binding("Reviewed new QTH"))

    def test_unbound_old_cache_geography_is_ignored(self):
        self.write_cache({"1": {"callsign": "K1TST", "grid": "EN61AB"}})
        self.assertEqual(self.read(), {"1": {"callsign": "K1TST"}})

    def test_failed_login_lookup_and_sparse_success_retain_last_good_location_and_photo(self):
        filename = self.seed_cache()
        expected = self.read()
        for failure in (None, RuntimeError("private-password")):
            with self.subTest(login=failure):
                self.login.side_effect = failure if isinstance(failure, Exception) else None
                self.login.return_value = failure
                self.refresh()
                self.assertEqual(self.read(), expected)
                self.assertEqual((self.cache / "photos" / filename).read_bytes(), png())
        self.login.side_effect = None
        self.login.return_value = "private-session"
        for info in (None, RuntimeError("raw XML private-address"), {},
                     {"current_call": "K9RECYCLED", "grid": "EM12"},
                     {"current_call": "K1TST", "grid": None, "lat": None, "lon": None, "image": None}):
            with self.subTest(lookup=info):
                self.lookup.side_effect = info if isinstance(info, Exception) else None
                self.lookup.return_value = info
                self.refresh()
                self.assertEqual(self.read(), expected)

    def test_malformed_fields_and_failed_photo_download_retain_last_good(self):
        self.seed_cache()
        expected = self.read()
        self.lookup.side_effect = lambda _session, call: {
            "current_call": call, "grid": "private-address", "lat": float("nan"), "lon": 1000,
            "image": "https://fixture.invalid/new.png", "name": "Unreviewed identity",
        }
        bad_png = png()[:33] + png_chunk(b"IDAT", b"not-zlib-data") + png_chunk(b"IEND", b"")
        bad_webp = b"RIFF\x0e\x00\x00\x00WEBPVP8 \x01\x00\x00\x00x\x00"
        for result in (None, b"<html>error</html>", b"\x89PNG\r\n\x1a\ntruncated",
                       bad_png, bad_webp, RuntimeError("private-token")):
            with self.subTest(photo=result):
                self.photo_fetch.side_effect = result if isinstance(result, Exception) else None
                self.photo_fetch.return_value = result
                self.refresh()
                self.assertEqual(self.read(), expected)

    def test_refresh_persists_allowlist_only_coarse_location_and_metadata_free_photos(self):
        self.lookup.side_effect = lambda _session, call: {
            "current_call": call, "grid": "EN61AB12" if call == "K1TST" else None,
            "lat": "43.654321", "lon": "-79.387654", "image": "https://fixture.invalid/private-token.png",
            "address": "private-home-address", "email": "private-email", "session": "private-session",
            "name": "Unreviewed name", "qth": "Unreviewed QTH", "sponsor_bkg_number": 999,
        }
        before = copy.deepcopy(self.members)
        result = self.refresh()
        raw = json.loads((self.cache / enrichment.CACHE_NAME).read_text())
        self.assertEqual(set(raw), {"version", "members"})
        self.assertEqual(set(raw["members"]), {"1", "2"})
        self.assertEqual(set(raw["members"]["1"]), {"callsign", "grid", "photo", "qth_hash"})
        self.assertEqual(raw["members"]["1"]["grid"], "EN61")
        self.assertEqual((raw["members"]["2"]["lat"], raw["members"]["2"]["lon"]), (43.65, -79.39))
        self.assertEqual(result["photos_updated"], 2)
        for photo in (self.cache / "photos").iterdir():
            self.assertEqual(photo.read_bytes(), png())
            self.assertNotIn(b"private", photo.read_bytes())
        self.assertEqual(self.members, before)
        self.assertNotIn("private", (self.cache / enrichment.CACHE_NAME).read_text())

    def test_valid_roster_prunes_inactive_cache_and_photos_even_during_qrz_outage(self):
        filename = self.seed_cache()
        self.login.return_value = None
        result = self.refresh([self.members[0]])
        self.assertEqual(result["active_members"], 1)
        raw = json.loads((self.cache / enrichment.CACHE_NAME).read_text())
        self.assertEqual(set(raw["members"]), {"1"})
        self.assertEqual([p.name for p in (self.cache / "photos").iterdir()], [filename])

    def test_bad_members_cannot_login_or_prune_existing_cache(self):
        self.seed_cache()
        before = {p.relative_to(self.cache): p.read_bytes() for p in self.cache.rglob("*") if p.is_file()}
        for members in ([], [self.members[0], self.members[0]], [self.member(True, "K1TST")],
                        [self.member(1, "../K1TST")], [{**self.members[0], "status": "inactive"}]):
            with self.subTest(members=members), self.assertRaises(ValueError):
                self.refresh(members)
        self.login.assert_not_called()
        self.assertEqual({p.relative_to(self.cache): p.read_bytes() for p in self.cache.rglob("*") if p.is_file()}, before)

    def test_production_paths_and_symlinks_rejected_before_side_effects(self):
        link = self.workspace / "source-link"
        link.symlink_to(self.root)
        for path in (self.root, self.root / "dist", self.root / "images/mugshots", self.workspace, link / "cache"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                enrichment.refresh_enrichment(self.members, path, username="x", password="y",
                                              login=self.login, lookup=self.lookup, photo_fetch=self.photo_fetch,
                                              repo_root=self.root)
        self.login.assert_not_called()
        self.assertFalse((self.root / "dist").exists())

    def test_refresh_refuses_unrelated_files_and_photo_symlink_directory(self):
        self.cache.mkdir()
        unrelated = self.cache / "important.txt"
        unrelated.write_text("Keep me")
        with self.assertRaises(ValueError):
            self.refresh()
        self.assertEqual(unrelated.read_text(), "Keep me")
        unrelated.unlink()
        (self.cache / "photos").symlink_to(self.root)
        with self.assertRaises(ValueError):
            self.refresh()
        self.login.assert_not_called()

    def test_cache_private_fields_and_unsafe_photos_never_annotate(self):
        filename = enrichment.photo_filename(1, "K1TST", ".png")
        self.write_cache({"1": {"callsign": "K1TST", "grid": "EN61AB", "lat": 12.34567, "lon": 23.45678,
                                "qth_hash": enrichment.location_binding("Reviewed QTH"),
                                "photo": filename, "address": "private-home", "session": "private-session"},
                          "2": {"callsign": "K2TST", "grid": "invalid", "lat": True, "lon": -100,
                                "photo": "../../source/private.png"}})
        (self.cache / "photos" / filename).write_bytes(png(metadata=True))
        self.assertEqual(self.read(), {"1": {"callsign": "K1TST", "grid": "EN61",
                                            "qth_hash": enrichment.location_binding("Reviewed QTH")},
                                      "2": {"callsign": "K2TST"}})

    def test_photo_formats_strip_metadata_and_reject_html_truncation_and_corruption(self):
        self.assertEqual(enrichment.sanitize_photo(png(metadata=True)), (png(), ".png"))
        gif_png = enrichment.sanitize_photo(GIF)
        self.assertEqual(gif_png[1], ".png")
        self.assertEqual(enrichment.sanitize_photo(gif_png[0]), gif_png)
        gif_with_comment = GIF[:-1] + b"\x21\xfe\x07private\x00;"
        self.assertEqual(enrichment.sanitize_photo(gif_with_comment), gif_png)
        self.assertEqual(enrichment.sanitize_photo(png() + b"private-address"), (png(), ".png"))
        for data in (b"<html>not photo</html>", GIF[:25], png()[:45], png()[:40] + b"bad" + png()[43:],
                     b"\xff\xd8\xff\xe1\x00\x02\xff\xd9", b"RIFF\x00\x00\x00\x00WEBP",
                     png()[:33] + png_chunk(b"IDAT", b"not-zlib-data") + png_chunk(b"IEND", b""),
                     b"RIFF\x0e\x00\x00\x00WEBPVP8 \x01\x00\x00\x00x\x00"):
            with self.subTest(data=data[:16]), self.assertRaises(ValueError):
                enrichment.sanitize_photo(data)

    def test_jpeg_arbitrary_app_metadata_is_removed(self):
        original = (ROOT / "logo.jpg").read_bytes()
        private = b"SYNTHETIC-GPS-PRIVATE-METADATA"
        app14 = b"\xff\xee" + (len(private) + 2).to_bytes(2, "big") + private
        clean, extension = enrichment.sanitize_photo(original[:2] + app14 + original[2:])
        self.assertEqual(extension, ".png")
        self.assertNotIn(private, clean)
        self.assertEqual(clean, enrichment.sanitize_photo(original)[0])

    def test_failed_atomic_replacement_restores_last_good_directory(self):
        self.seed_cache()
        before = (self.cache / enrichment.CACHE_NAME).read_bytes()
        rename = Path.rename
        def fail_publish(path, target):
            if path.name == "cache":
                raise OSError("simulated failed replacement")
            return rename(path, target)
        with patch.object(Path, "rename", fail_publish), self.assertRaises(OSError):
            self.refresh()
        self.assertEqual((self.cache / enrichment.CACHE_NAME).read_bytes(), before)

    def test_custom_photos_require_both_number_and_callsign_and_are_sanitized(self):
        photos = self.root / "images/mugshots-override"
        photos.mkdir(parents=True)
        (photos / "K1TST.png").write_bytes(png(metadata=True))
        manifest = self.root / "photo-overrides.json"
        manifest.write_text(json.dumps({"version": 1, "members": {
            "1": {"callsign": "K1TST", "photo": "K1TST.png"},
            "99": {"callsign": "K9OLD", "photo": "missing-inactive.png"},
        }}))
        selected = enrichment.read_photo_overrides(manifest, self.members, repo_root=self.root)
        self.assertEqual(selected, {"1": (enrichment.photo_filename(1, "K1TST", ".png"), png())})
        reused_call = [self.member(40, "K1TST"), self.member(1, "K1NEW")]
        self.assertEqual(enrichment.read_photo_overrides(manifest, reused_call, repo_root=self.root), {})
        self.login.return_value = None
        enrichment.refresh_enrichment(self.members, self.cache, username="x", password="y",
                                      login=self.login, lookup=self.lookup, photo_fetch=self.photo_fetch,
                                      photo_overrides=selected, repo_root=self.root)
        self.assertEqual((self.cache / "photos" / selected["1"][0]).read_bytes(), png())

    def test_empty_bootstrap_fails_before_cache_replacement_but_usable_outage_passes(self):
        self.seed_cache()
        before = (self.cache / enrichment.CACHE_NAME).read_bytes()
        self.login.return_value = None
        with self.assertRaisesRegex(ValueError, "bootstrap"):
            enrichment.refresh_enrichment([self.member(3, "K3NEW")], self.cache, username="x", password="y",
                                          login=self.login, lookup=self.lookup, photo_fetch=self.photo_fetch,
                                          require_usable=True, repo_root=self.root)
        self.assertEqual((self.cache / enrichment.CACHE_NAME).read_bytes(), before)
        result = enrichment.refresh_enrichment(self.members, self.cache, username="x", password="y",
                                              login=self.login, lookup=self.lookup, photo_fetch=self.photo_fetch,
                                              require_usable=True, repo_root=self.root)
        self.assertEqual(result["located_members"], 2)
        self.assertEqual(result["photos_retained"], 1)


class RefreshCommandTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("refresh_qrz", ROOT / "scripts/refresh-qrz.py")
        self.command = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.command)

    def test_malformed_roster_does_not_load_client_or_touch_cache(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary) / "cache"
            cache.mkdir()
            marker = cache / "enrichment.json"
            marker.write_text("last-good")
            with patch.object(self.command, "read_roster_json", return_value="{malformed"), \
                    patch.object(self.command, "load_qrz_client") as load, \
                    contextlib.redirect_stderr(io.StringIO()) as errors:
                result = self.command.main(["--roster-json", "/fixture.json", "--enrichment-dir", str(cache)])
            self.assertEqual(result, 1)
            load.assert_not_called()
            self.assertEqual(marker.read_text(), "last-good")
            self.assertNotIn("malformed", errors.getvalue())

    def test_transport_diagnostics_are_suppressed(self):
        def unsafe():
            print("private-token and raw XML", file=sys.stderr)
            print("private-password")
            return "session"
        with contextlib.redirect_stderr(io.StringIO()) as errors, contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(self.command._quiet(unsafe)(), "session")
        self.assertEqual(errors.getvalue(), "")
        self.assertEqual(output.getvalue(), "")

    def test_input_inside_cache_is_rejected_before_read_or_refresh(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary) / "cache"
            cache.mkdir()
            roster = cache / "roster.json"
            roster.write_text("keep-input")
            with patch.object(self.command, "read_roster_json") as read, \
                    patch.object(self.command, "load_qrz_client") as load, \
                    contextlib.redirect_stderr(io.StringIO()):
                result = self.command.main(["--roster-json", str(roster), "--enrichment-dir", str(cache)])
            self.assertEqual(result, 1)
            read.assert_not_called()
            load.assert_not_called()
            self.assertEqual(roster.read_text(), "keep-input")

    def test_refresh_rejects_qrz_geography_that_disagrees_with_reviewed_location(self):
        client = Mock()
        client.qth_location.return_value = ("NY", "United States")
        info = {"current_call": "K1TST", "state": "VT", "country": "United States",
                "grid": "FN31", "lat": 43, "lon": -72, "image": "https://fixture.invalid/photo.png"}
        client.qrz_fetch_callsign.return_value = info
        lookup = self.command.reviewed_lookup(client, [{"callsign": "K1TST", "qth": "New York"}])
        stale = lookup("session", "K1TST")
        self.assertFalse({"grid", "lat", "lon"} & stale.keys())
        self.assertEqual(stale["image"], info["image"])
        self.assertIn("grid", info)  # Does not mutate the QRZ client's result.
        info.update(state="NY", country="United States of America")
        self.assertEqual(lookup("session", "K1TST")["grid"], "FN31")


if __name__ == "__main__":
    unittest.main()
