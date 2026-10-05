"""Copy immutable administration-owned photos; never enrich member data."""

from concurrent.futures import ThreadPoolExecutor
import hashlib
import http.client
import io
from pathlib import Path
import re
import struct
import tempfile
import urllib.parse
import urllib.request
import warnings

from roster_input import MAX_PUBLIC_PHOTO_BYTES


REPO_ROOT = Path(__file__).resolve().parents[1]
ASSET_NAME = re.compile(r"[0-9a-f]{64}\.webp\Z")


def asset_directory(directory, *, repo_root=REPO_ROOT):
    directory = Path(directory).resolve()
    root = Path(repo_root).resolve()
    if directory == root or directory in root.parents or directory.is_relative_to(root):
        raise ValueError("Public asset directory must be outside the source repository")
    return directory


def _origin(url):
    try:
        parsed = urllib.parse.urlsplit(url)
        if (parsed.scheme != "https" or not parsed.hostname or parsed.port == 0
                or parsed.username is not None or parsed.password is not None
                or parsed.query or parsed.fragment
                or any(char.isspace() or ord(char) < 32 or ord(char) == 127 for char in url)):
            raise ValueError()
        return parsed.scheme, parsed.hostname.lower(), parsed.port or 443
    except (TypeError, ValueError):
        raise ValueError("Invalid public snapshot origin") from None


def verify_asset(data, photo):
    if (not isinstance(data, bytes) or len(data) != photo["byte_length"]
            or len(data) > MAX_PUBLIC_PHOTO_BYTES
            or hashlib.sha256(data).hexdigest() != photo["sha256"]):
        raise ValueError("Public photo bytes do not match the snapshot")
    # Decode and verify metadata-free, bounded WebP without rewriting pixels.
    # Administration may encode either lossy VP8 or lossless VP8L.
    try:
        _verify_webp(data)
    except Exception:
        raise ValueError("Unusable public WebP photo") from None


def _verify_webp(data):
    if (len(data) < 20 or data[:4] != b"RIFF" or data[8:12] != b"WEBP"
            or struct.unpack("<I", data[4:8])[0] != len(data) - 8):
        raise ValueError("Invalid WebP envelope")
    kinds = []
    offset = 12
    while offset < len(data):
        if offset + 8 > len(data):
            raise ValueError("Truncated WebP chunk")
        kind = data[offset:offset + 4]
        size = struct.unpack("<I", data[offset + 4:offset + 8])[0]
        end = offset + 8 + size
        if end + (size & 1) > len(data):
            raise ValueError("Truncated WebP payload")
        if kind == b"VP8X" and (size != 10 or data[offset + 8] & ~0x10 or any(data[offset + 9:offset + 12])):
            raise ValueError("WebP declares metadata or animation")
        kinds.append(kind)
        offset = end + (size & 1)
    if kinds not in ([b"VP8 "], [b"VP8L"], [b"VP8X", b"VP8 "],
                     [b"VP8X", b"VP8L"], [b"VP8X", b"ALPH", b"VP8 "]):
        raise ValueError("Unexpected WebP chunks")
    from PIL import Image

    with warnings.catch_warnings():
        warnings.simplefilter("error")
        with Image.open(io.BytesIO(data)) as image:
            if (image.format != "WEBP" or getattr(image, "n_frames", 1) != 1 or max(image.size) > 480):
                raise ValueError("Unexpected public photo dimensions or animation")
            image.load()


def _photos(members):
    photos = {}
    for member in members:
        photo = member["photo"]
        if photo is not None:
            digest = photo["sha256"]
            if digest in photos and photos[digest] != photo:
                raise ValueError("Inconsistent immutable photo metadata")
            photos[digest] = photo
    return photos


class _NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def _download(photo):
    request = urllib.request.Request(photo["url"], headers={
        "Accept": "image/webp", "User-Agent": "BKG-Public-Asset-Publisher/2.0",
    })
    try:
        with urllib.request.build_opener(_NoRedirects()).open(request, timeout=30) as response:
            if response.getcode() != 200 or response.headers.get_content_type() != "image/webp":
                raise ValueError("Public photo response is invalid")
            data = response.read(MAX_PUBLIC_PHOTO_BYTES + 1)
        verify_asset(data, photo)
        return data
    except (OSError, ValueError, http.client.HTTPException):
        raise OSError("Unable to fetch a validated public photo") from None


def fetch_assets(members, directory, *, source_url, workers=4, repo_root=REPO_ROOT):
    """Validate every photo origin before requests; atomically save exact bytes."""
    directory = asset_directory(directory, repo_root=repo_root)
    origin = _origin(source_url)
    photos = _photos(members)
    for photo in photos.values():
        if (_origin(photo["url"]) != origin or urllib.parse.urlsplit(photo["url"]).path
                != f"/api/roster/photos/{photo['sha256']}.webp"):
            raise ValueError("Public photo URL must share the snapshot origin")
    if directory.exists() and (not directory.is_dir() or any(
            child.is_symlink() or not child.is_file() or not ASSET_NAME.fullmatch(child.name)
            for child in directory.iterdir())):
        raise ValueError("Public asset cache must contain only immutable WebP files")
    if type(workers) is not int or not 1 <= workers <= 8:
        raise ValueError("Invalid public asset download concurrency")

    def read_or_fetch(item):
        digest, photo = item
        existing = directory / f"{digest}.webp"
        try:
            if existing.stat().st_size != photo["byte_length"]:
                raise ValueError("Cached photo length mismatch")
            data = existing.read_bytes()
            verify_asset(data, photo)
            return digest, data, False
        except (OSError, ValueError):
            return digest, _download(photo), True

    with ThreadPoolExecutor(max_workers=workers) as pool:
        assets = list(pool.map(read_or_fetch, photos.items()))
    directory.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{directory.name}-", dir=directory.parent) as temporary:
        staged = Path(temporary) / "assets"
        staged.mkdir()
        for digest, data, _downloaded in assets:
            (staged / f"{digest}.webp").write_bytes(data)
        previous = Path(temporary) / "previous"
        if directory.exists():
            directory.rename(previous)
        try:
            staged.rename(directory)
        except Exception:
            if previous.exists():
                previous.rename(directory)
            raise
    return {"photos": len(assets), "downloaded": sum(item[2] for item in assets),
            "byte_length": sum(len(item[1]) for item in assets)}


def annotate_public_assets(members, directory, *, repo_root=REPO_ROOT):
    """Read only photo bytes named by the validated v2 snapshot, offline."""
    photos = _photos(members)
    if photos and directory is None:
        raise ValueError("The v2 snapshot requires its public asset directory")
    directory = asset_directory(directory, repo_root=repo_root) if directory is not None else None
    selected = {}
    for member in members:
        member["mugshot_path"] = None
        if member["photo"] is None:
            continue
        photo = member["photo"]
        path = directory / f"{photo['sha256']}.webp"
        if path.is_symlink() or not path.is_file():
            raise ValueError("A v2 public photo is missing or unsafe")
        if path.stat().st_size != photo["byte_length"]:
            raise ValueError("Public photo bytes do not match the snapshot")
        verify_asset(path.read_bytes(), photo)
        relative = f"images/mugshots/{photo['sha256']}.webp"
        member["mugshot_path"] = relative
        selected[relative] = path
    return selected
