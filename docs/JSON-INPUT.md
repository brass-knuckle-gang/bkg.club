# Reviewed public JSON roster

The website builds from the administration-owned **V2** snapshot. See the
[data ownership inventory](PUBLISHER-DATA-INVENTORY.md) for the publishing
boundary and the [cutover record](PUBLIC-ROSTER-V2-CUTOVER.md) for its history.

## One snapshot per run

`fetch-roster.py` makes one anonymous HTTPS GET and validates the complete source contract and content hash. It saves the exact response as `$RUNNER_TEMP/bkg-build/roster-source.json` and the publication roster as `roster.json`, outside the checkout and published output. These match unless callsigns are excluded as described below. Asset fetching, build, and validation consume only the publication roster. Consumers independently revalidate the saved bytes; they never refetch the URL. The final validation compares the public JSON envelope with that saved publication input.

HTTPS, redirect rejection, the 30-second transport timeout, 8 MiB input limit, sanitized errors, schema/hash/reference checks, and empty-roster rejection are preserved. Optional Access headers are supported by the reader for other endpoints only when both credential fields are valid; an incomplete or empty pair fails before requesting. The deployed public feed needs neither header nor API token. Missing configuration or invalid input stops the workflow before artifact upload; the last published site remains available.

Build to `dist/` or an external preview directory. The snapshot and verified photo cache must be separate from the output, and the cache must be outside the checkout. The build publishes the website, selected photos, `members.txt`, and the validated envelope at `data/v2/roster.json`.

## Reviewed membership rules

`callsign`, `name`, and `qth` are authoritative reviewed values. The build uses the reviewed values for the site and member notes.

`sponsor_bkg_number` resolves through the stable exported membership number to the sponsor's current reviewed callsign. Null references stay null. Historical callsigns, stale cache records, or a later member reusing a callsign cannot recreate a suppressed sponsor edge.

`qso_date` supplies the Join Date. The original text remains intact in the public contract. Outbreak uses the builder's date parser and leaves empty or unreadable dates null.

OG badges and note labels come only from stored `og_assignments`. A holder who moves from Vermont to New York keeps `VT OG`; current QTH places that member on the New York map. JSON does not choose a new holder by current state, member number, or enrichment. Inactive members, inactive sponsors, and assignments with inactive holders are suppressed upstream; the builder validates all remaining references and never restores them from a cache.

Validation rejects unsupported schema versions, additional/private fields, duplicate identities or JSON keys, unsorted arrays, invalid sponsor/OG references, sponsor cycles, unsafe text, and mismatched content hashes. Empty active rosters are rejected even though the upstream empty fixture is valid API output. HTML and embedded JSON escape reviewed text before publication. Generation and validation complete in a temporary directory before the previous output can be replaced.

Publication tolerates one narrowly defined record error: a printable callsign
containing characters other than ASCII letters and digits, including slashes.
After validating the entire source and its original hash, it excludes those
members and creates a projection with its own valid content hash. No callsign is
guessed or rewritten. Recruits of an excluded member remain in the roster with a
null sponsor; OG assignments held by excluded members are omitted without
reassignment. Counts, roster rows, maps, downline, notes, photos, and public JSON
all consume the same projection. If every member would be excluded, publication
fails and preserves the previous site. All other validation remains strict.

The fetch step emits Actions warnings and a step summary with excluded BKG
numbers and reasons, never untrusted field values. Its JSON record contains
source/publication counts and hashes plus the exclusions. The 30-day input
artifact retains the exact source and publication files and this record even
when the accepted site is unchanged. Conditional GET caches always retain the
source bytes and source ETag; they never send a projection hash upstream.

## Administration-owned v2 publishing

Each member has nullable `map_location` and `photo` fields. A location
contains the accepted four-, six-, or eight-character public grid and its center
coordinates. Coordinates must match the computed center or its representation
rounded to four decimal places.
A null location suppresses the member's dot. Nearby and outbreak
use the exact exported coordinates, and reviewed QTH supplies territory labels.

A photo contains an anonymous HTTPS administration asset URL, its SHA-256,
`image/webp` content type, and byte length (at most 1 MiB). Assets must be on the
same HTTPS origin as the saved export's configured source and at the exact
`/api/roster/photos/<sha256>.webp` path. Redirects are rejected. The publisher
checks response type, size, hash, bounded WebP structure, and absence of metadata,
then copies those accepted bytes without resizing or re-encoding. A null photo
uses the placeholder. The content hash covers location and photo metadata along
with membership and OG fields.

To reproduce the production build locally, no credentials are needed:

```sh
preview_dir="$(mktemp -d "${TMPDIR:-/tmp}/bkg-public-roster.XXXXXX")"
python3 -m venv "$preview_dir/venv"
. "$preview_dir/venv/bin/activate"
python3 -m pip install -r requirements-build.txt
export ROSTER_EXPORT_URL=https://bkg-public-roster.cool-cake-fac1.workers.dev/api/roster/export/v2
python3 scripts/fetch-roster.py \
  --cache-dir "$preview_dir/roster-cache-v2" --output "$preview_dir/roster.json"
python3 scripts/fetch-public-assets.py \
  --roster-json "$preview_dir/roster.json" --asset-dir "$preview_dir/public-assets"
python3 scripts/build-roster.py \
  --roster-json "$preview_dir/roster.json" --asset-dir "$preview_dir/public-assets" \
  --asset-origin "$ROSTER_EXPORT_URL" \
  --output-dir "$preview_dir/site"
python3 scripts/validate-site.py "$preview_dir/site"
python3 -m http.server 8765 --bind 127.0.0.1 --directory "$preview_dir/site"
```

The asset directory must be outside the repository and separate from snapshot
and output. The build reuses cached photos only when their bytes match the
snapshot. Missing or invalid assets fail the build and retain the last
published site. Removing or hiding a member or photo prunes the next artifact.

`--asset-origin` defaults to `ROSTER_EXPORT_URL`; provide a trusted source URL
when rendering saved V2 photos offline. The snapshot cache is bound to the
endpoint and can reuse the saved JSON only after a matching `304`. The
[deployment runbook](DEPLOYMENT.md#roll-back-the-exact-published-files) covers
restoring a verified Pages artifact.
