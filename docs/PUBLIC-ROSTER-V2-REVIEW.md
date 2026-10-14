# Deployed V2 consumer review — October 13, 2026

**Recommendation: the website implementation is ready for review; hold production
activation and keep PR #46 draft until population/backfill and QRZ photo refresh
are resolved.** Public nulls are honored throughout the preview.

## Inputs and provenance

- Website main: `cb1d5f14389ae1ac424f37798bb4adf9c325aebb`. PR #46 already
  descended from that revision; no main changes needed reconciliation.
- Backend main: `5e9322d92488ec08cc50b97462255afff2eac3fd`, merged
  [PR #13](https://github.com/brass-knuckle-gang/bkg-automation/pull/13), including
  [PR #15](https://github.com/brass-knuckle-gang/bkg-automation/pull/15).
  Both schemas and all four shared fixtures match byte-for-byte.
- Known-good production baseline:
  [run 38454205437, attempt 1](https://github.com/brass-knuckle-gang/bkg.club/actions/runs/38454205437),
  successful build/deploy, with both `github-pages-38454205437-1` and
  `bkg-build-inputs-38454205437-1` downloaded. Its V1 snapshot was generated
  `2026-10-13T19:21:52.057Z`, hash
  `sha256:a2a902a08f13bd90eeb6bb873ab30cc215ab1d308cbe3e82d1fc4b508a8939f8`.
- Credential-free V2 snapshot: generated `2026-10-13T20:03:07.469Z`, hash
  `sha256:da33bcb4c96d561be790f43a998272449cfd4a26c7683d15ee159628aafae81e`,
  from `https://bkg-public-roster.cool-cake-fac1.workers.dev/api/roster/export/v2`.
  The saved input and copied photo bytes are retained with the local preview.

These numbers describe fixed snapshots. Population continues to change coverage;
repeat the comparison with a fresh snapshot before activation.

## Comparison

| Measure | Published V1 baseline | V2 preview | Result |
| --- | ---: | ---: | --- |
| Active identities / roster cards / notes | 732 | 734 | Adds BKG733/734; removes none |
| Sponsor graph edges | 719 | 721 | Existing edges unchanged; two new edges |
| Stored OG assignments | 61 | 61 | Exactly equal |
| Nearby and outbreak member coordinates | 728 | 36 | 692 existing locations become null |
| Six-character published grids | 728 | 36 | All common grids exactly equal |
| Member photos | 589 | 10 | Only the ten contributed photos currently publish |

For all 732 existing members, `bkg_number`, `callsign`, `name`, `qth`,
`qso_date`, and `sponsor_bkg_number` are identical. The live V1 and V2 feeds
also agree on every V1 member field and OG assignment. Territory buckets and
QSO-date projections retain the same existing members; both new members appear
across roster, notes, territories, downline, and outbreak. All 734 QSO dates parse.

Nearby and outbreak use each non-null exported `{grid, lat, lon}` exactly.
V1 rounded grid centers to four decimal places; V2 keeps the backend's full
numeric precision (largest common coordinate difference about 0.000033333°).
The upstream fixture additionally checks four-character grid preservation.
V2's 698 null locations and 724 null photos remain suppressed, with no QRZ,
geocoding, cache, or website override recovery.

All ten photo descriptors resolve anonymously on the roster origin. The
publisher verified `image/webp`, exact lengths totaling **288,002 bytes**, every
SHA-256, metadata-free still WebP structure, full decode, and dimensions no
larger than 480×480. A second fetch reused those verified bytes with zero
downloads. Static copies preserve the accepted bytes without transformation.
All ten existing contributions have matching current BKG/callsign bindings.

## Verification

- **104 offline tests pass.** Shared V1/V2 fixtures, schema/hash/reference
  rejection, current-callsign sponsor display, authoritative nulls, asset
  origin/hash/length/metadata, malformed inputs, and atomic failures are covered.
  Corrupt roster/photo validation preserves the previous complete build.
- Version-specific, endpoint-bound roster caches were tested with fresh `200`,
  changed snapshots, matching `304`, corrupt/wrong-version caches, changed
  endpoints, mismatched/unsolicited `304`, and failed requests. A live conditional
  request returned **304** with the snapshot's matching weak ETag at 20:03 UTC.
- The live V2 preview and `validate-site.py --source json` pass. Its published
  envelope equals the saved validated input; page identities, stable sponsors,
  map coordinates, selected photos, and notes agree with that envelope.
- Explicit V1 rollback build from the saved production input/enrichment also
  passes, preserving **732 members, 728 locations, and 589 photos**. It does not
  use the V2 caches or public assets.
- `actionlint` with ShellCheck passes for both workflows. Upload/deploy remain
  downstream of snapshot, photo, and full-site validation. PR checks have no
  production environment or secrets.
- Browser review at **1440×1000** and **390×844** covers the homepage/roster,
  Nearby, downline, and outbreak. Founder/contributed photos load; other cards
  show the required placeholders. Nearby Leaflet/OpenStreetMap tiles and a
  `DN40` grid search work; a `Salt Lake City, UT` address search returns seven
  nearby members through Nominatim. Visitor location services are unchanged;
  photo alt text and the map attribution now accommodate administrator choices.
  Roster mobile
  width remains 396 px at a 390 px viewport in both baseline and V2, an existing
  six-pixel overflow. Downline likewise retains its existing 424 px scroll width
  at 390 px. The sparse dots/photos reflect feed coverage.
- CNAME, favicon/logo, clubs, recruitment, safety, shared styles, and the mobile
  mockup retain their baseline files. No production settings were changed.

## Population and backfill blockers

Read-only production D1 queries at approximately 20:03 UTC explain the large
location loss: **698 active members had never been checked**; 36 had accepted
grids. Publication checks began at `19:35:39.786Z` and the latest completed batch
was at `20:00:50.541Z`. There were **zero hidden location/photo selections** and
**zero imported grids/photos**. Coverage progressed from 24 to 36 locations
during this review, consistent with six-member five-minute batches. A full
734-member first pass takes roughly ten hours at that rate without failures;
do not infer completion from one cron run or the six-hour refresh due target.

There is also a confirmed QRZ photo blocker: **27 image candidates use
`https://cdn-xml.qrz.com`**, and all 27 records report photo-refresh failure.
Backend main's [photo URL guard](https://github.com/brass-knuckle-gang/bkg-automation/blob/5e9322d92488ec08cc50b97462255afff2eac3fd/src/integrations/member-photos.ts)
accepts only `https://files.qrz.com`. It therefore rejects the observed candidate
host before fetching. There are zero accepted automatic QRZ photos; the ten
Git contributions are already synchronized and usable. Backend maintainers
must resolve the allowlist/redirect policy and verify successful sanitized
photo population. Waiting for batches alone cannot resolve this mismatch.

The downloaded V1 enrichment offers **728 exact number/callsign/QTH-bound grid
candidates** and **587 similarly bound photo candidates** for a reviewed
backfill. Four cache records lack a QTH binding and must not be rebound. These
are locally eligible candidates, not an authenticated backend dry-run or a
completed import. Use the backend's
[backfill procedure](https://github.com/brass-knuckle-gang/bkg-automation/blob/main/docs/member-publication-backfill.md)
with a current authorized administrator session, preserving existing automatic,
manual, hidden, and repository selections and skipping changed identities.
No backend writes or imports were performed in this review.

Before switching, complete the pass or reviewed backfill, account for remaining
not-found/QTH/callsign conflicts and intentional hidden values in administration,
verify QRZ photos now publish, and rerun a fresh saved-snapshot preview. Do not
make website fallback enrichment mask those gaps. The
[cutover runbook](PUBLIC-ROSTER-V2-CUTOVER.md#activate-once) specifies the
`github-pages` environment secret `ROSTER_EXPORT_V2_URL` and repository Actions
variable `ROSTER_EXPORT_VERSION=2`, followed by dispatch and public verification.
