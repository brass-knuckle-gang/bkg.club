# Website data ownership

Administration owns approved member data, map locations, and photos. The site
renders an anonymous V2 roster snapshot and its verified public photos into
static files.

## Build-time inputs

`fetch-roster.py` fetches and validates one snapshot. `fetch-public-assets.py`
downloads only the photos named by that snapshot and verifies their bytes.
Rendering and validation use those saved inputs. `fit-outbreak-projection.py`
uses embedded development data.

## Enrichment behavior to preserve in administration

The website never changes an exported callsign, name, QTH, sponsor, QSO date,
or OG. Administration's separate scheduled callsign refresh follows QRZ,
retains private callsign history, and preserves sponsor links by stable member
identity (backend [PR #15](https://github.com/brass-knuckle-gang/bkg-automation/pull/15)).
Sponsor display resolves the exported BKG number to the current callsign in
the same snapshot.
An aliased QRZ response can supply a photo and geography for the same reviewed
identity. Geography is accepted only when QRZ country/state agrees with current
reviewed QTH. Canadian province names are interpreted within Canada; missing QRZ
province is allowed, while a reviewed US state must match.

The website uses the exported V2 coordinates verbatim. A move suppresses
coordinates bound to the previous QTH. Photos are bound to the stable BKG
number and reviewed callsign.

Failed or sparse lookups retain accepted enrichment for matching identities.
A reviewed photo override takes precedence over QRZ. Administration publishes
metadata-free WebP images, and the website copies only verified public bytes.

Administration's V2 refresh runs in small batches (at most six members every
five minutes at the October 2026 review), so a due time is not a full-roster
completion promise. The initial population finished before the October 21
cutover; the [live review](PUBLIC-ROSTER-V2-REVIEW.md) records its coverage gates.

## Browser requests

Nearby loads OpenStreetMap tiles and optionally asks Nominatim to geocode the
address, place, US state or Canadian province the visitor types (states and
provinces as a structured query, so "WA" is Washington). Member lookups by
callsign or explicit BKG number and grid searches stay in the browser. Member
maps, downline, outbreak, notes, and roster cards consume static generated data.

The badge press (`badges.html`) reads this site's published
`data/v2/roster.json` for number, callsign, name, QTH, and OG labels. Members
with multiple OG titles have a badge option for each assignment, and the bulk
ZIP includes every assignment. Visitors can also paste or upload a V2 export.

The recruit form posts applications directly to the membership intake API
(`https://bkg-membership-intake.cool-cake-fac1.workers.dev/v1/applications`),
guarded by a Cloudflare Turnstile challenge. For sponsor lookup it reads this
site's published `data/v2/roster.json`.
That is an intake workflow, not a backend roster read; see
[the recruit form guide](RECRUIT-FORM.md). The former FormSubmit relay is no
longer used. Google Fonts and the pinned Leaflet CDN provide presentation
assets. QRZ profile, Discord, YouTube, `programmer.bkg.club`, and other outbound
links are navigation.

## Publication boundary

The build validates the entire administration snapshot before accepting its
public photos. It verifies each photo's bytes against the snapshot before
copying it into the static artifact. Invalid inputs or assets stop the build
before it replaces the previous output.

The [V2 cutover record](PUBLIC-ROSTER-V2-CUTOVER.md) documents the October 2026
activation. The [deployment runbook](DEPLOYMENT.md) describes current operation
and Pages artifact rollback.
