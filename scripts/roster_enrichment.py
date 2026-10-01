"""Optional, sanitized QRZ data for the JSON roster experiment.

Rendering only reads this cache. Refreshing it is a separate, explicit command.
Membership and identity always come from the reviewed roster export, never QRZ.
"""

import hashlib
import json
import math
from pathlib import Path
import re
import tempfile


REPO_ROOT = Path(__file__).resolve().parents[1]
CACHE_NAME = "enrichment.json"
CACHE_VERSION = 1
MAX_PHOTO_BYTES = 10 * 1024 * 1024
MAX_PHOTO_PIXELS = 50_000_000
CALLSIGN_RE = re.compile(r"[A-Za-z0-9]+(?:/[A-Za-z0-9]+)*")
GRID_RE = re.compile(r"[A-R]{2}[0-9]{2}(?:[A-X]{2}(?:[0-9]{2})?)?")
PHOTO_EXTENSIONS = {".png", ".jpg", ".gif", ".webp"}


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
    """Fully decode an image and re-encode its first frame without metadata.

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
                # A fresh image has no EXIF, GPS, comments, source URLs, ICC,
                # animation metadata, or encoder options inherited from QRZ.
                clean = Image.frombytes(mode, decoded.size, decoded.tobytes())
                output = io.BytesIO()
                clean.save(output, format="PNG", compress_level=6)
                result = output.getvalue()
    except Exception as exc:
        raise ValueError("Unusable enrichment photo") from exc
    if len(result) > MAX_PHOTO_BYTES:
        raise ValueError("Sanitized enrichment photo exceeds the cache limit")
    return result, ".png"


def _read_photo(directory: Path, number: str, callsign: str, value) -> tuple[str, bytes] | None:
    if not isinstance(value, str):
        return None
    extension = Path(value).suffix
    if extension not in PHOTO_EXTENSIONS or value != photo_filename(int(number), callsign, extension):
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
        sanitized, actual_extension = sanitize_photo(data)
    except (OSError, ValueError):
        return None
    if actual_extension != extension or sanitized != data:
        return None
    return value, data


def read_enrichment(directory: Path, members: list[dict], *, repo_root: Path = REPO_ROOT) -> dict[str, dict]:
    """Read only allowlisted data bound to current, active reviewed identities."""
    directory = validate_enrichment_dir(directory, repo_root=repo_root)
    identities = _identities(members)
    try:
        raw = json.loads((directory / CACHE_NAME).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    if not isinstance(raw, dict) or raw.get("version") != CACHE_VERSION or not isinstance(raw.get("members"), dict):
        return {}
    records = {}
    qths = {str(member["number"]): location_binding(member.get("qth") or "") for member in members}
    for number, callsign in identities.items():
        value = raw["members"].get(number)
        if not isinstance(value, dict) or value.get("callsign") != callsign:
            continue
        record = {"callsign": callsign}
        grid = _grid(value.get("grid"))
        coordinates = _coordinates(value)
        if value.get("qth_hash") != qths[number]:
            grid = coordinates = None
        if grid:
            record["grid"] = grid
        elif coordinates:
            record["lat"], record["lon"] = coordinates
        if grid or coordinates:
            record["qth_hash"] = qths[number]
        photo = _read_photo(directory, number, callsign, value.get("photo"))
        if photo:
            record["photo"] = photo[0]
        records[number] = record
    return records


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


def refresh_enrichment(members: list[dict], directory: Path, *, username: str,
                       password: str, login, lookup, photo_fetch,
                       repo_root: Path = REPO_ROOT) -> dict[str, int]:
    """Explicit refresh with atomic cache replacement and per-field LKG fallback.

    Callbacks are injected so tests are offline and the renderer never imports
    QRZ networking. No returned QRZ identity, address, session, URL, or raw XML
    is persisted. QRZ aliases cannot change a reviewed roster identity.
    """
    directory = validate_enrichment_dir(directory, repo_root=repo_root)
    identities = _identities(members)  # Validate before login, mkdir, or pruning.
    if directory.exists():
        if not directory.is_dir() or any(child.name not in {CACHE_NAME, "photos"} for child in directory.iterdir()):
            raise ValueError("Enrichment refresh requires a dedicated cache directory")
        photo_dir = directory / "photos"
        if photo_dir.is_symlink() or (photo_dir.exists() and not photo_dir.is_dir()):
            raise ValueError("Enrichment photos must be a directory inside the dedicated cache")
    records = read_enrichment(directory, members, repo_root=repo_root)
    photos = {}
    for number, record in records.items():
        if record.get("photo"):
            photo = _read_photo(directory, number, identities[number], record["photo"])
            if photo:
                photos[photo[0]] = photo[1]
    summary = {"active_members": len(identities), "lookups_updated": 0,
               "lookups_failed": 0, "photos_updated": 0, "photos_retained": 0}
    qths = {str(member["number"]): location_binding(member.get("qth") or "") for member in members}
    try:
        session = login(username, password) if username and password else None
    except Exception:
        session = None
    for number, callsign in identities.items():
        record = records.setdefault(number, {"callsign": callsign})
        try:
            info = lookup(session, callsign) if session else None
        except Exception:
            info = None
        current_call = info.get("current_call") if isinstance(info, dict) else None
        if not isinstance(current_call, str) or current_call.upper() != callsign.upper():
            summary["lookups_failed"] += 1
            continue
        _merge_location(record, info)
        if record.get("grid") is not None or _coordinates(record) is not None:
            record["qth_hash"] = qths[number]
        summary["lookups_updated"] += 1
        image = info.get("image")
        if isinstance(image, str):
            # Reject local files, credentials, non-HTTP schemes, and absurd URLs.
            from urllib.parse import urlsplit
            try:
                parsed = urlsplit(image)
                usable = (len(image) <= 2048 and parsed.scheme in {"http", "https"}
                          and parsed.hostname and not parsed.username and not parsed.password)
                if usable:
                    data, extension = sanitize_photo(photo_fetch(image))
                    filename = photo_filename(int(number), callsign, extension)
                    record["photo"] = filename
                    photos[filename] = data
                    summary["photos_updated"] += 1
            except Exception:
                pass
    keep_photos = {record["photo"] for record in records.values() if record.get("photo")}
    summary["photos_retained"] = len(keep_photos) - summary["photos_updated"]
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
