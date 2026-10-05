# Administration-owned member publication

Backend dependency: [administration PR #13](https://github.com/brass-knuckle-gang/bkg-automation/pull/13).
Keep this consumer change in draft until that implementation is published.

This is an explicit future production cutover. The website PR prepares v2
consumption; it does not deploy the backend, change GitHub configuration, import
member data, or activate the new publication source. Missing configuration fails
closed. The current v1 endpoint and production behavior remain intact.

## Prepare and compare

1. Deploy the administration implementation with the additive v2 export at
   `/api/roster/export/v2` and immutable public photo delivery at
   `/api/roster/photos/<sha256>.webp`. Keep `/api/roster/export` unchanged for v1.
   Configure the administration enrichment/assets runtime, complete its reviewed
   backfill, and reconcile the 10 existing contributed photo overrides in
   `public/member-overrides/photos`. Preserve explicit hidden/manual choices and the
   number/callsign/QTH identity safeguards. Future contributed photo PRs belong
   in the administration repository.
2. Save one v2 snapshot outside the website checkout using `fetch-roster.py
   --schema-version 2`. Fetch its assets with `fetch-public-assets.py` from that
   same configured origin. Build an external preview with `--source json
   --schema-version 2 --asset-dir`, then run `validate-site.py --source json`.
   These operations publish nothing and need no website QRZ credentials.
3. Compare the preview against a known-good saved v1 build: roster identities,
   names, numbers, dates, sponsors, stored OGs, territory buckets, notes, Nearby
   and outbreak coordinates, and roster photos. Review intentional missing or
   hidden locations/photos and contributions rather than restoring them from a
   cache. Confirm the published v2 envelope equals the exact saved input and
   photo hashes/sizes match the accepted bytes.
4. Preserve the current successful Pages/input artifacts and deployment record
   for rollback. Verify current v2 photo references return anonymous metadata-free
   WebP without redirects. Hiding a photo or changing the selected identity may
   make an older URL return 404; retain the verified bytes with each saved
   snapshot instead of relying on future endpoint access. The asset cache must
   not be the only surviving copy of a photo.

## Activate once

After the backend and comparison are verified, configure the website Actions
secret **`ROSTER_EXPORT_V2_URL`** with the complete v2 HTTPS export URL, then set
the repository variable **`ROSTER_EXPORT_VERSION`** to **`2`**. An unset variable
defaults to `1`; a missing v2 secret never falls back to the v1 URL. The existing
`ROSTER_EXPORT_URL` secret remains the v1 rollback source.

Dispatch the Deploy workflow without `recheck`. The v2 build fetches one snapshot,
verifies its immutable administration photos, renders offline, validates the
artifact, and deploys through the existing serialized GitHub Pages workflow.
The v1 QRZ refresh/cache steps and their credentials are skipped in v2. Enrichment
rechecks now belong in administration; a v2 dispatch with `recheck` fails with an
explanation instead of silently ignoring the request.

Verify the public roster/count, notes, territory map, Nearby, downline, outbreak,
photo replacements/placeholders, and `/data/v2/roster.json`. Check that snapshot
identities, accepted coordinates, and selected photo bytes agree across every
output. No webpage reads D1 or contacts QRZ for membership enrichment.

The isolated `bkg-public-assets-v2-*` cache contains only hash-named public WebP
files. `bkg-build-inputs-*` retains the saved v2 snapshot, hash/timestamp/source
record, and verified `public-assets/` directory for 30 days. The existing
fingerprint comparison ignores export generation time for both schema versions
and still observes visible changes in locations, photos, and membership.

## Rollback and retirement

For immediate rollback, restore the exact known-good Pages archive using the
[existing deployment rollback procedure](DEPLOYMENT.md#roll-back-the-exact-published-files).
Set `ROSTER_EXPORT_VERSION` back to `1` before later publication runs if continued
v1 publishing is required. Account for serialized pending runs before restoration.
Changing the version alone causes a new mutable-input build, so it is not an
exact reproduction of a historical artifact.

Keep the v1 feed, website QRZ secrets, legacy cache, and website contributed-photo
manifest until the v2 publication and rollback checkpoint is verified. Remove
those compatibility paths in a follow-up retirement change. Retiring them is
separate from the recruitment form's direct-intake migration and from static
presentation dependencies such as Leaflet, map tiles, and location geocoding.
