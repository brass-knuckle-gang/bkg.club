# Administration-owned member publication

Backend [PR #13](https://github.com/brass-knuckle-gang/bkg-automation/pull/13)
is merged and deployed. The anonymous V2 endpoint is:

`https://bkg-public-roster.cool-cake-fac1.workers.dev/api/roster/export/v2`

The [live review](PUBLIC-ROSTER-V2-REVIEW.md) includes the October 21 recheck:
the original population and QRZ photo-host blockers are resolved. The final
preflight verifies all 882 members, 880 sponsor links, 61 OG assignments,
871 locations, and 687 photos against the current feeds. Administration also carries WA7SPY's newer contribution
before activation, preserving all eleven contributed portraits.

This is an explicit production cutover. The website PR prepares v2
consumption; it does not deploy the backend, change GitHub configuration, import
member data, or activate the new publication source. Missing configuration fails
closed. The current v1 endpoint and production behavior remain intact.

## Prepare and compare

1. Verify the deployed contract against backend main's
   [schema/fixtures](https://github.com/brass-knuckle-gang/bkg-automation/tree/main/docs/public-roster)
   and [publication rules](https://github.com/brass-knuckle-gang/bkg-automation/blob/main/docs/member-publication.md).
   Complete or review the bounded population pass/backfill and reconcile the
   contributed photos in `public/member-overrides/photos` (eleven at cutover). Investigate
   coverage loss through administration's publication status, modes, and
   identity/QTH bindings. Public nulls do not distinguish hidden, stale,
   not-found, or never-checked records; they remain authoritative in every case.
   Verify any photo-host or population failures recorded in the live review
   are resolved before cutover.
   Future photo contributions and overrides belong in administration.
2. Save one v2 snapshot outside the website checkout using `fetch-roster.py
   --schema-version 2`. Fetch its assets with `fetch-public-assets.py` from that
   same configured origin. Build an external preview with `--source json
   --schema-version 2 --asset-dir`, then run `validate-site.py --source json`.
   These operations publish nothing and need no website QRZ credentials.
   Use `--asset-origin "$ROSTER_EXPORT_URL"` when rendering a saved snapshot;
   the builder also defaults to that environment variable. The trusted origin
   is checked again before copying cached assets into the site.
3. Compare the preview against a known-good saved v1 build: roster identities,
   names, numbers, dates, sponsors, stored OGs, territory buckets, notes, Nearby
   and outbreak coordinates, and roster photos. Review intentional missing or
   hidden locations/photos and contributions rather than restoring them from a
   cache. Confirm the published v2 envelope equals the exact saved input and
   photo hashes/sizes match the accepted bytes.
   Preserve all six V1 member fields and stored OG assignments. Backend
   [PR #15](https://github.com/brass-knuckle-gang/bkg-automation/pull/15)
   owns callsign changes/history and sponsor resolution: show the exported
   current callsign and resolve `sponsor_bkg_number` through the same snapshot.
   There is no public callsign-history array and no website alias matching in V2.
4. Preserve the current successful Pages/input artifacts and deployment record
   for rollback. Verify current v2 photo references return anonymous metadata-free
   WebP without redirects. Hiding a photo or changing the selected identity may
   make an older URL return 404; retain the verified bytes with each saved
   snapshot instead of relying on future endpoint access. The asset cache must
   not be the only surviving copy of a photo.

## Activate once

After population, photo refresh, backfill review, and comparison are verified,
configure the **`github-pages`
environment** Actions secret **`ROSTER_EXPORT_V2_URL`** to
`https://bkg-public-roster.cool-cake-fac1.workers.dev/api/roster/export/v2`, then set
the repository variable **`ROSTER_EXPORT_VERSION`** to **`2`**. An unset variable
defaults to `1`; a missing v2 secret never falls back to the v1 URL. The existing
`ROSTER_EXPORT_URL` secret remains the v1 rollback source.

For the initial cutover, the existing main workflow still selects V1 directly
and ignores `ROSTER_EXPORT_VERSION`. Set the URL secret and version variable
after preflight but **before merging PR #46**, so the merge's first production
build uses V2 without performing a website QRZ refresh. Then merge the reviewed
PR and monitor its Deploy run. A manual dispatch is needed only if the merge
run did not start or a later retry is required.

In `brass-knuckle-gang/bkg.club`, use Settings → Environments → `github-pages`
→ Environment secrets for the URL and Settings → Secrets and variables →
Actions → Variables for `ROSTER_EXPORT_VERSION`. The public endpoint needs no
Access token or authentication headers. Equivalent operator commands:

```sh
# At the prompt, paste the public V2 endpoint above.
gh secret set ROSTER_EXPORT_V2_URL --env github-pages --repo brass-knuckle-gang/bkg.club
gh variable set ROSTER_EXPORT_VERSION --body 2 --repo brass-knuckle-gang/bkg.club
# Merge PR #46, then monitor its push-triggered Deploy run.
# If a manual run is needed after merge:
gh workflow run deploy.yml --ref main --repo brass-knuckle-gang/bkg.club
```

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

The separate `bkg-roster-v1-*` and `bkg-roster-v2-*` caches store endpoint-bound
snapshots and ETags. Restored JSON is validated for the selected schema and
content hash before sending `If-None-Match`. A matching `304` reuses those exact
validated bytes; an unsolicited or mismatched `304` fails. A changed endpoint
or corrupt/wrong-version cache causes an unconditional fetch. Transport errors
never serve stale cached membership. Caches are saved only after the full build
validates; cache failures cannot authorize a deployment.

The isolated `bkg-public-assets-v2-*` cache contains only hash-named public WebP
files. `bkg-build-inputs-*` retains the saved v2 snapshot, hash/timestamp/source
record, and verified `public-assets/` directory for 30 days. The existing
fingerprint comparison ignores export generation time for both schema versions
and still observes visible changes in locations, photos, and membership. Its
shared deployed-site fingerprint records the last successful deployment across
both versions; it is not a membership or enrichment cache.

## Rollback and retirement

For immediate rollback, restore the exact known-good Pages archive using the
[existing deployment rollback procedure](DEPLOYMENT.md#roll-back-the-exact-published-files).
Set `ROSTER_EXPORT_VERSION` back to `1` before later publication runs if continued
v1 publishing is required. Account for serialized pending runs before restoration.
Changing the version alone causes a new mutable-input build, so it is not an
exact reproduction of a historical artifact.

```sh
gh variable set ROSTER_EXPORT_VERSION --body 1 --repo brass-knuckle-gang/bkg.club
```

Verify `/data/v1/roster.json`, V1 enrichment, and the selected known-good Pages
artifact when rolling back. Neither V2's roster cache nor its public-asset cache
can replace the V1 snapshot/enrichment cache. The live review also validates a
V1 rebuild from the saved production input/cache with the updated consumer.

Keep the v1 feed, website QRZ secrets, legacy cache, and website contributed-photo
manifest until the v2 publication and rollback checkpoint is verified. Remove
those compatibility paths in a follow-up retirement change. Retiring them is
separate from the recruitment form's direct-intake migration and from static
presentation dependencies such as Leaflet, map tiles, and location geocoding.
