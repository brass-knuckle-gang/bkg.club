"""Optional, sanitized QRZ data for the reviewed JSON roster.

Rendering only reads this cache. Refreshing it is a separate, explicit command.
Membership and identity always come from the reviewed roster export, never QRZ.
"""

from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import tempfile
import time


REPO_ROOT = Path(__file__).resolve().parents[1]
CACHE_NAME = "enrichment.json"
CACHE_VERSION = 2
# Version 1 caches hold full-size PNG photos and no lookup times. They remain
# readable so a refresh can upgrade them in place instead of starting over.
READABLE_CACHE_VERSIONS = {1, 2}
MAX_PHOTO_BYTES = 10 * 1024 * 1024
MAX_PHOTO_PIXELS = 50_000_000
# Cards render at most ~250 CSS px wide; 480 px covers 2x displays. Lossy WebP
# at this size averages ~30 KB against ~1 MB for the full-size PNGs.
PHOTO_MAX_SIDE = 480
PHOTO_QUALITY = 82
PHOTO_EXTENSION = ".webp"
MAX_STORED_PHOTO_BYTES = 1024 * 1024
CALLSIGN_RE = re.compile(r"[A-Za-z0-9]+(?:/[A-Za-z0-9]+)*")
GRID_RE = re.compile(r"[A-R]{2}[0-9]{2}(?:[A-X]{2}(?:[0-9]{2})?)?")
PHOTO_EXTENSIONS = {".png", ".jpg", ".gif", ".webp"}
LEGACY_PHOTO_EXTENSIONS = PHOTO_EXTENSIONS - {PHOTO_EXTENSION}
# Bookkeeping kept in the cache for refresh scheduling; never rendered.
REFRESH_FIELDS = ("checked_at", "image_ref")


def validate_enrichment_dir(directory: Path, *, repo_root: Path = REPO_ROOT) -> Path:
    """Resolve symlinks and refuse the source tree, dist, or their ancestors."""
    directory = Path(directory).resolve()
    root = Path(repo_root).resolve()
    if directory == root or directory in root.parents or directory.is_relative_to(root):
        raise ValueError("Enrichment directory must be outside the source repository")
    return directory


def _identities(members: list[dict]) -> dict[str, str]:
    if not isinstance(members, list) or not members:
        raise ValueError("Enrichment requires a validated nonempty active roster")
    identities = {}
    callsigns = set()
    for member in members:
        number, callsign = member.get("number"), member.get("callsign")
        if (not isinstance(number, int) or isinstance(number, bool) or number <= 0
                or not isinstance(callsign, str)
                or not CALLSIGN_RE.fullmatch(callsign)):
            raise ValueError("Enrichment requires reviewed BKG numbers and callsigns")
        if str(number) in identities or callsign.upper() in callsigns:
            raise ValueError("Enrichment roster contains duplicate identities")
        if member.get("status", "active") != "active" or member.get("active", True) is not True:
            raise ValueError("Enrichment requires active roster members only")
        identities[str(number)] = callsign
        callsigns.add(callsign.upper())
    return identities


def photo_filename(number: int, callsign: str, extension: str) -> str:
    """Bind a photo to both stable membership and its reviewed current callsign."""
    if extension not in PHOTO_EXTENSIONS:
        raise ValueError("Unsupported enrichment photo format")
    digest = hashlib.sha256(callsign.encode("ascii")).hexdigest()[:12]
    return f"bkg-{number}-{digest}{extension}"


def location_binding(qth: str) -> str:
    """Invalidate stale geography whenever the reviewed current QTH changes."""
    return hashlib.sha256(qth.encode("utf-8")).hexdigest()


def _coordinates(value: dict) -> tuple[float, float] | None:
    lat, lon = value.get("lat"), value.get("lon")
    if isinstance(lat, bool) or isinstance(lon, bool):
        return None
    try:
        lat, lon = float(lat), float(lon)
    except (TypeError, ValueError, OverflowError):
        return None
    if not (math.isfinite(lat) and math.isfinite(lon)
            and -90 <= lat <= 90 and -180 <= lon <= 180):
        return None
    return round(lat, 2), round(lon, 2)


def _grid(value) -> str | None:
    if isinstance(value, str) and GRID_RE.fullmatch(value.upper()):
        return value.upper()[:4]
    return None


def sanitize_photo(data: bytes) -> tuple[bytes, str]:
    """Fully decode an image and re-encode its first frame, downscaled, as
    metadata-free WebP.

    Pillow is deliberately imported only for photo handling; an ordinary JSON
    render with no cached photos has no enrichment or image dependency.
    """
    if not isinstance(data, bytes) or not data or len(data) > MAX_PHOTO_BYTES:
        raise ValueError("Unusable enrichment photo")
    import io
    import warnings
    from PIL import Image, ImageOps

    try:
        with warnings.catch_warnings():
            # Reject malformed metadata as well as oversized images without
            # leaking Pillow diagnostics derived from untrusted QRZ content.
            warnings.simplefilter("error")
            with Image.open(io.BytesIO(data)) as image:
                if image.format not in {"PNG", "JPEG", "GIF", "WEBP"}:
                    raise ValueError("Unsupported enrichment photo")
                if image.width * image.height > MAX_PHOTO_PIXELS:
                    raise ValueError("Unusable photo dimensions")
                image.verify()
            with Image.open(io.BytesIO(data)) as image:
                image.seek(0)
                image.load()  # verify() alone does not validate compressed pixels.
                mode = "RGBA" if "A" in image.getbands() or "transparency" in image.info else "RGB"
                decoded = ImageOps.exif_transpose(image).convert(mode)
                decoded.thumbnail((PHOTO_MAX_SIDE, PHOTO_MAX_SIDE), Image.Resampling.LANCZOS)
                # A fresh image has no EXIF, GPS, comments, source URLs, ICC,
                # animation metadata, or encoder options inherited from QRZ.
                clean = Image.frombytes(mode, decoded.size, decoded.tobytes())
                output = io.BytesIO()
                clean.save(output, format="WEBP", quality=PHOTO_QUALITY)
                result = output.getvalue()
        verify_photo(result)
    except Exception as exc:
        raise ValueError("Unusable enrichment photo") from exc
    return result, PHOTO_EXTENSION


def _webp_chunks(data: bytes) -> list[tuple[bytes, bytes]]:
    if len(data) < 20 or data[:4] != b"RIFF" or data[8:12] != b"WEBP":
        raise ValueError("Not a WebP container")
    if struct.unpack("<I", data[4:8])[0] + 8 != len(data):
        raise ValueError("WebP size mismatch or trailing data")
    chunks, offset = [], 12
    while offset < len(data):
        if offset + 8 > len(data):
            raise ValueError("Truncated WebP chunk")
        kind, size = data[offset:offset + 4], struct.unpack("<I", data[offset + 4:offset + 8])[0]
        end = offset + 8 + size
        if end > len(data):
            raise ValueError("Truncated WebP chunk")
        chunks.append((kind, data[offset + 8:end]))
        offset = end + (size & 1)
    return chunks


def verify_photo(data: bytes) -> None:
    """Accept only the exact shape sanitize_photo() writes.

    Lossy WebP does not re-encode to identical bytes, so a cached photo is
    checked structurally instead: a single still image with no EXIF, XMP, ICC,
    animation, or unknown chunks, within size limits, that fully decodes.
    """
    try:
        _verify_photo(data)
    except Exception as exc:
        # Any surprise from a cached file means "unusable", never a crash.
        raise ValueError("Unusable stored photo") from exc


def _verify_photo(data: bytes) -> None:
    if not isinstance(data, bytes) or not data or len(data) > MAX_STORED_PHOTO_BYTES:
        raise ValueError("Unusable stored photo")
    chunks = _webp_chunks(data)
    kinds = [kind for kind, _ in chunks]
    if kinds not in ([b"VP8 "], [b"VP8X", b"VP8 "], [b"VP8X", b"ALPH", b"VP8 "]):
        raise ValueError("Unexpected WebP chunks")
    # Only the alpha flag may be set; ICC, EXIF, XMP, animation are refused.
    if kinds[0] == b"VP8X" and (len(chunks[0][1]) != 10 or chunks[0][1][0] & ~0x10):
        raise ValueError("WebP declares metadata or animation")
    import io
    import warnings
    from PIL import Image

    with warnings.catch_warnings():
        warnings.simplefilter("error")
        with Image.open(io.BytesIO(data)) as image:
            if (image.format != "WEBP" or getattr(image, "n_frames", 1) != 1
                    or max(image.size) > PHOTO_MAX_SIDE):
                raise ValueError("Unexpected stored photo")
            image.load()


def _read_photo(directory: Path, number: str, callsign: str, value, *,
                upgrade: bool = False) -> tuple[str, bytes] | None:
    """Return a verified canonical photo for this identity.

    With ``upgrade`` (refresh only), a version 1 photo bound to the same
    identity is treated as untrusted input and fully re-sanitized.
    """
    if not isinstance(value, str):
        return None
    extension = Path(value).suffix
    if extension not in PHOTO_EXTENSIONS or value != photo_filename(int(number), callsign, extension):
        return None
    if extension != PHOTO_EXTENSION and not upgrade:
        return None
    photo_dir = directory / "photos"
    if photo_dir.is_symlink():
        return None
    photo_root = photo_dir.resolve()
    photo = directory / "photos" / value
    if not photo.resolve().is_relative_to(photo_root) or photo.is_symlink():
        return None
    try:
        if photo.stat().st_size > MAX_PHOTO_BYTES:
            return None
        data = photo.read_bytes()
        if extension == PHOTO_EXTENSION:
            verify_photo(data)
            return value, data
        clean, clean_extension = sanitize_photo(data)
    except (OSError, ValueError):
        return None
    return photo_filename(int(number), callsign, clean_extension), clean


def read_enrichment(directory: Path, members: list[dict], *, repo_root: Path = REPO_ROOT) -> dict[str, dict]:
    """Read only allowlisted data bound to current, active reviewed identities."""
    records, _photos, _moved = _load(directory, members, repo_root=repo_root)
    for record in records.values():
        for field in REFRESH_FIELDS:
            record.pop(field, None)
    return records


def _load(directory: Path, members: list[dict], *, repo_root: Path = REPO_ROOT,
          upgrade: bool = False) -> tuple[dict[str, dict], dict[str, bytes], set[str]]:
    """Return (records, verified photo bytes by filename, numbers whose QTH moved)."""
    directory = validate_enrichment_dir(directory, repo_root=repo_root)
    identities = _identities(members)
    try:
        raw = json.loads((directory / CACHE_NAME).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}, {}, set()
    if (not isinstance(raw, dict) or raw.get("version") not in READABLE_CACHE_VERSIONS
            or not isinstance(raw.get("members"), dict)):
        return {}, {}, set()
    records, photos, moved = {}, {}, set()
    qths = {str(member["number"]): location_binding(member.get("qth") or "") for member in members}
    for number, callsign in identities.items():
        value = raw["members"].get(number)
        if not isinstance(value, dict) or value.get("callsign") != callsign:
            continue
        record = {"callsign": callsign}
        grid = _grid(value.get("grid"))
        coordinates = _coordinates(value)
        if value.get("qth_hash") != qths[number]:
            if grid or coordinates:
                moved.add(number)
            grid = coordinates = None
        if grid:
            record["grid"] = grid
        elif coordinates:
            record["lat"], record["lon"] = coordinates
        if grid or coordinates:
            record["qth_hash"] = qths[number]
        photo = _read_photo(directory, number, callsign, value.get("photo"), upgrade=upgrade)
        if photo:
            record["photo"] = photo[0]
            photos[photo[0]] = photo[1]
        checked_at = value.get("checked_at")
        if type(checked_at) is int and checked_at >= 0:
            record["checked_at"] = checked_at
        image_ref = value.get("image_ref")
        if photo and isinstance(image_ref, str) and re.fullmatch(r"[0-9a-f]{64}", image_ref):
            record["image_ref"] = image_ref
        records[number] = record
    return records, photos, moved


def annotate_json_enrichment(members: list[dict], directory: Path | None, *, repo_root: Path = REPO_ROOT) -> dict[str, Path]:
    """Annotate rendering fields without network calls or cache/source writes."""
    _identities(members)
    if directory is not None:
        directory = validate_enrichment_dir(directory, repo_root=repo_root)
    records = read_enrichment(directory, members, repo_root=repo_root) if directory is not None else {}
    photos = {}
    for member in members:
        member.update(grid=None, lat=None, lon=None, mugshot_path=None)
        record = records.get(str(member["number"]), {})
        for field in ("grid", "lat", "lon"):
            if field in record:
                member[field] = record[field]
        if record.get("photo"):
            relative = f"images/mugshots/bkg-{member['number']}{Path(record['photo']).suffix}"
            member["mugshot_path"] = relative
            photos[relative] = directory / "photos" / record["photo"]
    return photos


def _merge_location(record: dict, info: dict) -> None:
    # Missing/invalid QRZ fields never remove last-known-good enrichment.
    if info.get("grid") is not None:
        grid = _grid(info["grid"])
        if not grid:
            return
        record["grid"] = grid
        record.pop("lat", None)
        record.pop("lon", None)
    elif (coordinates := _coordinates(info)) is not None:
        record.pop("grid", None)
        record["lat"], record["lon"] = coordinates


def read_photo_overrides(manifest: Path, members: list[dict], *, repo_root: Path = REPO_ROOT) -> dict[str, tuple[str, bytes]]:
    """Select custom photos by reviewed number AND callsign; sanitize pixels."""
    identities = _identities(members)
    raw = json.loads(Path(manifest).read_text(encoding="utf-8"))
    if not isinstance(raw, dict) or raw.get("version") != 1 or not isinstance(raw.get("members"), dict):
        raise ValueError("Invalid photo override manifest")
    photo_root = Path(repo_root) / "images/mugshots-override"
    if photo_root.is_symlink() or not photo_root.resolve().is_relative_to(Path(repo_root).resolve()):
        raise ValueError("Photo overrides must be inside the source repository")
    selected = {}
    for number, callsign in identities.items():
        value = raw["members"].get(number)
        if not isinstance(value, dict) or value.get("callsign") != callsign:
            continue
        filename = value.get("photo")
        if (not isinstance(filename, str) or Path(filename).name != filename
                or Path(filename).suffix.lower() not in PHOTO_EXTENSIONS | {".jpeg"}):
            raise ValueError("Invalid photo override path")
        source = photo_root / filename
        if (source.is_symlink() or not source.resolve().is_relative_to(photo_root.resolve())
                or source.stat().st_size > MAX_PHOTO_BYTES):
            raise ValueError("Invalid photo override file")
        pixels, extension = sanitize_photo(source.read_bytes())
        selected[number] = (photo_filename(int(number), callsign, extension), pixels)
    return selected


def refresh_enrichment(members: list[dict], directory: Path, *, username: str,
                       password: str, login, lookup, photo_fetch,
                       photo_overrides: dict | None = None, require_usable: bool = False,
                       max_age_seconds: int | None = None, interval_seconds: int | None = None,
                       workers: int = 1, now: int | None = None, recheck: frozenset[str] = frozenset(),
                       repo_root: Path = REPO_ROOT) -> dict[str, int]:
    """Explicit refresh with atomic cache replacement and per-field LKG fallback.

    Callbacks are injected so tests are offline and the renderer never imports
    QRZ networking. No returned QRZ identity, address, session, URL, or raw XML
    is persisted. QRZ aliases cannot change a reviewed roster identity.

    Without ``max_age_seconds`` every member is looked up. With it, only due
    members are: anyone new or whose reviewed QTH moved, plus the
    least-recently-checked slice sized so that runs every ``interval_seconds``
    revisit the whole roster within ``max_age_seconds``. Per-run work then
    tracks roster growth divided by the number of runs per cycle, and a photo
    is only downloaded again when QRZ's image URL changes. Reviewed callsigns
    in ``recheck`` are looked up this run regardless of that schedule. ``lookup``
    and ``photo_fetch`` may run on ``workers`` threads; all merging stays here.
    """
    directory = validate_enrichment_dir(directory, repo_root=repo_root)
    identities = _identities(members)  # Validate before login, mkdir, or pruning.
    if directory.exists():
        if not directory.is_dir() or any(child.name not in {CACHE_NAME, "photos"} for child in directory.iterdir()):
            raise ValueError("Enrichment refresh requires a dedicated cache directory")
        photo_dir = directory / "photos"
        if photo_dir.is_symlink() or (photo_dir.exists() and not photo_dir.is_dir()):
            raise ValueError("Enrichment photos must be a directory inside the dedicated cache")
    if max_age_seconds is not None and (not interval_seconds or not 0 < interval_seconds < max_age_seconds):
        raise ValueError("Refresh interval must be positive and shorter than the maximum age")
    now = int(time.time()) if now is None else now
    records, photos, moved = _load(directory, members, repo_root=repo_root, upgrade=True)
    photo_overrides = photo_overrides or {}
    summary = {"active_members": len(identities), "lookups_updated": 0,
               "lookups_aliased": 0, "lookups_failed": 0, "lookups_skipped": 0,
               "photos_updated": 0, "photos_retained": 0, "photos_overridden": 0}
    qths = {str(member["number"]): location_binding(member.get("qth") or "") for member in members}

    due = list(identities)
    if max_age_seconds is not None:
        forced = {callsign.strip().upper() for callsign in recheck}
        urgent = [n for n in identities if n not in records or n in moved or identities[n].upper() in forced]
        rest = sorted((n for n in identities if n not in urgent),
                      key=lambda n: (records[n].get("checked_at", 0), int(n)))
        # Finish a full pass one run early so a single missed run stays in bounds.
        runs_per_cycle = max(1, max_age_seconds // interval_seconds - 1)
        budget = -(-len(identities) // runs_per_cycle)
        due = urgent + [n for n in rest[:budget]
                        if now - records[n].get("checked_at", 0) >= interval_seconds]
        summary["lookups_skipped"] = len(identities) - len(due)

    try:
        session = login(username, password) if username and password and due else None
    except Exception:
        session = None

    def fetch(number):
        """Network and decoding only; returns (info, image_ref, photo or None)."""
        try:
            info = lookup(session, identities[number])
        except Exception:
            return None, None, None
        image = info.get("image") if isinstance(info, dict) else None
        if not isinstance(image, str) or number in photo_overrides:
            return info, None, None
        image_ref = hashlib.sha256(image.encode("utf-8", "surrogatepass")).hexdigest()
        record = records.get(number, {})
        if record.get("photo") in photos and record.get("image_ref") == image_ref:
            return info, image_ref, None
        # Reject local files, credentials, non-HTTP schemes, and absurd URLs.
        from urllib.parse import urlsplit
        try:
            parsed = urlsplit(image)
            if not (len(image) <= 2048 and parsed.scheme in {"http", "https"}
                    and parsed.hostname and not parsed.username and not parsed.password):
                return info, None, None
            return info, image_ref, sanitize_photo(photo_fetch(image))
        except Exception:
            return info, None, None

    if session:
        with ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
            results = list(pool.map(fetch, due))
    else:
        results = []
        summary["lookups_failed"] = len(due)
    for number, (info, image_ref, photo) in zip(due, results):
        callsign = identities[number]
        record = records.setdefault(number, {"callsign": callsign})
        # Count an attempt as a check either way, so a member QRZ cannot
        # answer does not hold the oldest slot and starve everyone else.
        record["checked_at"] = now
        # QRZ answers a retired or vanity-replaced call with the operator's
        # current record (<call> differs from the reviewed callsign). Use that
        # record's geography and photo, exactly as the Sheets build did, but
        # keep the reviewed callsign: QRZ never renames a member here, and the
        # current call is not persisted. A missing <call> is "not found".
        current_call = info.get("current_call") if isinstance(info, dict) else None
        if not isinstance(current_call, str) or not current_call.strip():
            summary["lookups_failed"] += 1
            continue
        if current_call.strip().upper() != callsign.upper():
            summary["lookups_aliased"] += 1
        _merge_location(record, info)
        if record.get("grid") is not None or _coordinates(record) is not None:
            record["qth_hash"] = qths[number]
        summary["lookups_updated"] += 1
        if photo is not None:
            data, extension = photo
            filename = photo_filename(int(number), callsign, extension)
            record["photo"] = filename
            record["image_ref"] = image_ref
            photos[filename] = data
            summary["photos_updated"] += 1
    for number, (filename, pixels) in photo_overrides.items():
        if number not in identities or filename != photo_filename(int(number), identities[number], PHOTO_EXTENSION):
            raise ValueError("Photo override must match an active reviewed identity")
        verify_photo(pixels)
        record = records.setdefault(number, {"callsign": identities[number]})
        record["photo"] = filename
        record.pop("image_ref", None)
        photos[filename] = pixels
        summary["photos_overridden"] += 1
    keep_photos = {record["photo"] for record in records.values() if record.get("photo")}
    summary["photos_retained"] = len(keep_photos) - summary["photos_updated"] - summary["photos_overridden"]
    summary["located_members"] = sum(bool(record.get("grid") or _coordinates(record)) for record in records.values())
    if require_usable and (not keep_photos or not summary["located_members"]):
        raise ValueError("Enrichment bootstrap requires usable geography and photos")
    payload = {"version": CACHE_VERSION, "members": records}
    directory.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{directory.name}-refresh-", dir=directory.parent) as temporary:
        staged = Path(temporary) / "cache"
        (staged / "photos").mkdir(parents=True)
        for filename in sorted(keep_photos):
            (staged / "photos" / filename).write_bytes(photos[filename])
        (staged / CACHE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        # Swap only after every sanitized artifact is durable in staging. Keep
        # the old directory available for recovery if the final rename fails.
        previous = Path(temporary) / "previous"
        if directory.exists():
            directory.rename(previous)
        try:
            staged.rename(directory)
        except Exception:
            if previous.exists():
                previous.rename(directory)
            raise
    return summary
