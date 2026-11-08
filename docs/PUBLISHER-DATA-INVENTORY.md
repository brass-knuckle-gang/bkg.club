# Website data ownership

Audit baseline: `cb1d5f14389ae1ac424f37798bb4adf9c325aebb` (October 5, 2026).
Status updated November 8, 2026.

The administration application owns member data and its enrichment. The website
turns a validated snapshot and its immutable public assets into static pages.
Production has published the v2 snapshot since October 21, 2026. The v1
publication path, with the website's own QRZ enrichment, remains available only
for rollback until its retirement.

## Build-time inputs

| Input | Website use (v1 rollback path, except the first row) | v2 owner |
| --- | --- | --- |
| Anonymous roster JSON | Reviewed active number, callsign, name, QTH, QSO date, sponsor reference, stored OG assignments; in v2 (production) also accepted map locations and photo descriptors | Administration |
| QRZ XML login and callsign lookup | Canonical call for alias detection; public grid/coordinates; country/state agreement; profile image URL | Administration |
| QRZ profile image download | Sanitized public roster photo | Administration |
| `images/photo-overrides.json` and 11 source photos | Explicit number-and-callsign-bound photo replacement, preferred over QRZ | Administration |
| Legacy Sheets CSV | Optional legacy roster source; production already selects JSON | Retired by v2 publication |
| Legacy name/location overrides | Used only by the explicit Sheets path; JSON ignores them | Reconcile against administration before retirement |
| Legacy callsign/mugshot caches | Used only by the explicit Sheets path | Retired by v2 publication |

No other builder script fetches member data. `fetch-roster.py` fetches exactly one
snapshot; photo fetching, refresh, rendering, validation, and artifact retention
use those saved bytes. In v2, `fetch-public-assets.py` downloads only the photos
that snapshot names and verifies them against it. In v1, QRZ enrichment is a
separate command. Rendering itself does not refresh
QRZ. `fit-outbreak-projection.py` uses embedded development data, not a remote
member lookup.

## Enrichment behavior to preserve in administration

The website never changes an exported callsign, name, QTH, sponsor, QSO date,
or OG. Administration's separate scheduled callsign refresh follows QRZ,
retains private callsign history, and preserves sponsor links by stable member
identity (backend [PR #15](https://github.com/brass-knuckle-gang/bkg-automation/pull/15)).
The public contract contains only the current callsign; sponsor display resolves
the exported BKG number against the current snapshot. The website consumes no
history array and performs no V2 sponsor alias matching.
An aliased QRZ response can supply a photo and geography for the same reviewed
identity. Geography is accepted only when QRZ country/state agrees with current
reviewed QTH. Canadian province names are interpreted within Canada; missing QRZ
province is allowed, while a reviewed US state must match.

The v1 cache prefers the center of the public four- or six-character Maidenhead
grid; longer valid grids are trimmed to six. Without a grid, v1 coordinates
are rounded to two decimals. Administration v2 resolves QRZ coordinates to a
six-character grid when no valid grid exists and publishes that grid's center.
The website uses the exported v2 coordinates verbatim. Geography is bound to
the current reviewed QTH, so a move suppresses old coordinates. Photos are bound to both stable BKG number and
reviewed callsign. Removed or changed identities cannot inherit a cached photo.

Failed lookups and sparse successful responses retain last-known-good fields
for matching identities. An explicit reviewed photo override takes precedence
over QRZ. Photos are fully decoded, limited to 480 pixels on their longest side,
and re-encoded as first-frame WebP with metadata removed. The current cache
stores only allowlisted public values plus refresh bookkeeping; raw XML,
credentials, email, addresses, and image source URLs are excluded.

The website's V1 scheduled refresh checks the least-recently-checked slice each hour so the
roster is revisited within 24 hours. New/moved members and explicit `recheck`
callsigns are checked immediately. Unchanged image URLs avoid another download.
Administration's V2 refresh runs in small batches (at most six members every
five minutes at the October 2026 review), so a due time is not a full-roster
completion promise. The initial population finished before the October 21
cutover; the [live review](PUBLIC-ROSTER-V2-REVIEW.md) records its coverage gates.

## Browser requests

Nearby loads OpenStreetMap tiles and optionally asks Nominatim to geocode the
address, place, US state or Canadian province the visitor types (states and
provinces as a structured query, so "WA" is Washington). A typed member
callsign or grid square is resolved in the browser with no lookup. It does not
use browser geolocation. These requests do not enrich or modify member
data. Member maps, downline, outbreak, notes, and roster cards consume static
generated data.

The badge press (`badges.html`) reads this site's published
`data/v2/roster.json` (falling back to `data/v1/roster.json`) for number,
callsign, name, QTH, and OG labels. If neither loads, the roster stays empty
until the visitor pastes or uploads export JSON. It never writes member data.

The recruit form posts applications directly to the membership intake API
(`https://bkg-membership-intake.cool-cake-fac1.workers.dev/v1/applications`),
guarded by a Cloudflare Turnstile challenge. For sponsor lookup it reads this
site's published `data/v2/roster.json`, falling back to `data/v1/roster.json`.
That is an intake workflow, not a backend roster read; see
[the recruit form guide](RECRUIT-FORM.md). The former FormSubmit relay is no
longer used. Google Fonts and the pinned Leaflet CDN provide presentation
assets. QRZ profile, Discord, YouTube, `programmer.bkg.club`, and other outbound
links are navigation.

## Publication boundary

The v2 path must not read legacy overrides/caches, log in to QRZ, download QRZ
images, or select another membership source after a failure. It validates the
entire administration snapshot before accepting its public photos, then verifies
immutable photo bytes against the snapshot before copying them into the static
artifact. Invalid input/assets fail before replacing the previous output.

The repository variable `ROSTER_EXPORT_VERSION=2` selected v2 on October 21,
2026, after backend deployment, enrichment and override reconciliation,
comparison of a saved v2 snapshot, and publication verification. The workflow's
built-in default is also v2; only setting the variable to `1` selects the v1
rollback path.
