# GitHub Pages deployment and rollback

Production publishes the administration-owned **V2** roster. It has done so
since October 21, 2026 (Deploy run
[39615920834](https://github.com/brass-knuckle-gang/bkg.club/actions/runs/39615920834)),
when the [v2 cutover procedure](PUBLIC-ROSTER-V2-CUTOVER.md) was carried out.
The repository Actions variable `ROSTER_EXPORT_VERSION` is set to `2`, and the
`github-pages` environment secret `ROSTER_EXPORT_V2_URL` holds the V2 endpoint.
In V2 the workflow does no QRZ refresh and applies no website photo overrides:
it copies verified immutable administration photos and publishes the saved V2
snapshot.

V1 remains in the workflow only as a rollback path. The workflow's built-in
default is still `1`, so deleting the `ROSTER_EXPORT_VERSION` variable (or
setting it to `1`) switches the next build back to the V1 feed in the
`ROSTER_EXPORT_URL` repository secret, with the website's own QRZ enrichment.
Both paths keep fixed-revision validation and artifact rollback.

GitHub Pages, `www.bkg.club`, and CNAME/DNS are unchanged by either path. Production triggers are pushes to `main`, manual dispatch on `main`, and `23 * * * *` (hourly). A scheduled run that would publish the same site as the last deploy stops after validation; pushes and manual dispatches always deploy, unless a dispatch sets `skip_unchanged`. The `deploy` concurrency group serializes runs without cancelling active deployment. Pull requests run only the offline Node and Python checks, with read-only permissions and no production environment/secrets. Creating or updating a draft PR cannot deploy this site.

The `recheck` dispatch input applies to V1 only. It names callsigns (comma-separated) that the website's QRZ refresh looks up that run regardless of the hourly schedule, for example `gh workflow run deploy.yml -f recheck=VE6TD,VA6DM`. Under V2 a dispatch with `recheck` fails with an explanation, because enrichment rechecks belong in administration ([bkg-automation](https://github.com/brass-knuckle-gang/bkg-automation)).

See [JSON input rules](JSON-INPUT.md) for the reviewed contract, V2 photos, and V1 enrichment. The [v1](PUBLIC-ROSTER-CUTOVER.md) and [v2](PUBLIC-ROSTER-V2-CUTOVER.md) cutover runbooks are historical records; the v2 one also covers rollback to V1.

## Build contract and deployment checks

The build checks out the event's exact `github.sha`, verifies HEAD, installs the pinned image dependency, and runs offline tests. It fetches/validates one anonymous roster response using the secret for the selected version (`ROSTER_EXPORT_V2_URL` for V2, `ROSTER_EXPORT_URL` for V1), saving it under `$RUNNER_TEMP/bkg-build`, outside `dist/` and the checkout. Separate `bkg-roster-v1-*` / `bkg-roster-v2-*` caches support conditional GET: only a schema/hash/endpoint-validated snapshot with a matching ETag can be reused after `304`. Transport, integrity, schema, and reference errors fail before upload. Invalid callsign characters are the sole record-level exception: the original response is retained as `roster-source.json`, and `roster.json` contains a filtered publication projection with its own hash. The fetch step reports excluded BKG numbers and counts in Actions warnings and its step summary; recruits retain null sponsor links and excluded OG holders are not replaced. See [membership rules](JSON-INPUT.md#reviewed-membership-rules). Photo fetching (V2), QRZ refresh (V1), and build consume this same file; none of them selects Sheets, stale membership, or fixtures on failure.

### V2 photos (production)

The V2 build restores the `bkg-public-assets-v2-*` cache into `$RUNNER_TEMP/bkg-build/public-assets`, then `scripts/fetch-public-assets.py` checks every photo the saved snapshot references. It accepts only anonymous HTTPS assets on the snapshot's own origin at `/api/roster/photos/<sha256>.webp`, with no redirects, and verifies content type, byte length, SHA-256, and metadata-free still WebP structure. Accepted bytes are copied into the site without resizing or re-encoding. Cached files are reused only when they still match the snapshot. Map locations come verbatim from the snapshot's `map_location` fields. No QRZ login, enrichment cache, or website override is involved. See [Administration-owned v2 publishing](JSON-INPUT.md#administration-owned-v2-publishing).

### V1 enrichment (rollback only)

When V1 is selected, a separate QRZ refresh restores only sanitized, identity/QTH-bound enrichment into `$RUNNER_TEMP/bkg-build/enrichment`. It filters to active exported identities, validates location matches, sanitizes photos, and applies reviewed custom-photo bindings. Each run looks up only members that are due: new members, members whose reviewed QTH moved, and the least recently checked share of the roster sized so every member is rechecked within 24 hours of hourly runs (`--max-age-hours 24 --interval-hours 1`), using four concurrent lookups. A photo is downloaded again only when QRZ's image URL changes; the cache stores a SHA-256 of that URL, never the URL. Photos are downscaled to 480 px and re-encoded as metadata-free WebP; cached photos are verified structurally (single still image, no EXIF/XMP/ICC/animation/unknown chunks, full decode) on every read. Version 1 caches with full-size PNG photos are re-sanitized on the first refresh. The existing environment-scoped QRZ secrets remain available. A fresh cache must contain accepted geography and photos before the workflow can build; subsequent QRZ outages can reuse matching last-known-good enrichment. No authenticated roster credentials are needed. No raw QRZ data or cache-persistence commits enter this workflow.

### Build, validate, and deploy

The V2 builder runs `--source json --schema-version 2 --roster-json <saved-file> --asset-dir <external-assets> --output-dir dist`; V1 replaces the last two options with `--enrichment-dir <external-cache>`. It stages and validates the full site before replacing output. The explicit public allowlist (`PUBLIC_FILES` in `scripts/site_contract.py`) preserves all nine pages (`index`, `tree`, `nearby`, `outbreak`, `recruit`, `clubs`, `safety`, `badges`, `arcade`), CNAME, favicon/logo, CSS, the roster and recruit scripts, the arcade splash image, the mobile mockup, the whole `game/` directory (its GPL license requires `source/`, `LICENSE`, and `THIRD-PARTY.txt` to travel with the compiled game), member notes, and active photos. JSON mode also publishes the validated envelope at `data/v<version>/roster.json`, which is `data/v2/roster.json` in production. Source page templates retain search, maps, recruitment, downline, application links, and existing social-card assets. Private inputs and raw/cache metadata are excluded.

The validator explicitly uses `--source json`, checks public file/link/roster/date/reference consistency, and validates the public envelope against page identities and stable sponsors. The workflow verifies HEAD again and compares the complete published envelope against its saved input. It fingerprints `dist/` with `scripts/site-fingerprint.py`, which ignores only the build timestamps and the export's `generated_at`, and compares it with the newest `bkg-deployed-v1-*` Actions cache, which the deploy job saves after each successful deploy. That cache name is historical: it holds the last deployed fingerprint for both roster versions. A scheduled run with an identical fingerprint skips the remaining upload and deploy steps but still saves its roster and photo/enrichment caches. Otherwise it retains a separate immutable input archive and uploads the immutable Pages archive. Cache-save failure is nonblocking because the retained input archive is already available. The deploy job depends on successful build completion and publishes the selected artifact without checkout/refetch/rebuild. Build/input/validation/upload failures leave the live deployment in place.

Run the offline checks that pull requests run, with Python 3.10+ and the pinned dependency (Pillow) installed, for example in a virtual environment:

```sh
python3 -m pip install -r requirements-build.txt
node --test tests/recruit.test.cjs
python3 -m unittest discover -s tests -v
```

Tests block live networking and exercise anonymous/optional-auth fetches, snapshot reuse, malformed/hash/reference/empty input, atomic failures, unchanged previous outputs, V2 asset origin/hash/length/metadata checks, active enrichment filtering, stale location rejection, and metadata removal. The Node suite covers the recruit form rules. Local preview commands are in [JSON-INPUT.md](JSON-INPUT.md#administration-owned-v2-publishing) (V2, no credentials) and [JSON-INPUT.md](JSON-INPUT.md#reproduce-an-inspectable-local-preview) (V1, needs QRZ credentials).

## Build input retention and recovery

A build retains `bkg-build-inputs-<run-id>-<build-attempt>` for **30 days**, separate from `github-pages-<run-id>-<build-attempt>`, whenever it uploads a new site or excludes any members. Scheduled runs that change nothing and exclude no one keep no archive. It contains the exact source response (`roster-source.json`), the publication snapshot (`roster.json`), both hashes and any exclusions in `roster-record.json`, `source-sha.txt`, and either the verified `public-assets/` (V2) or the sanitized `enrichment/` cache (V1). No URL, credentials, source photo URLs, or raw QRZ responses are stored. The Actions caches `bkg-public-assets-v2-*` (V2) and `bkg-enrichment-v1-*` (V1) restore the newest successfully validated cache available on the branch. GitHub may evict caches; the immutable archive provides independent recovery. A missing or evicted V2 photo cache costs only a full re-download of the snapshot's photos on the next run.

For V1 enrichment cache recovery, download a matching known-good input artifact with `gh run download RUN_ID --repo brass-knuckle-gang/bkg.club --name INPUT_ARTIFACT_NAME --dir saved-inputs`, verify its hash/source record, and retain `enrichment/` together with its canonical photos. A local rebuild can use that external directory with the newly saved roster; stale/inactive fields are filtered again. Recovery to the Actions cache needs an operator-reviewed cache seed/repair in the normal build environment, or a successful QRZ refresh; it must not upload an unvalidated public artifact or use the retained snapshot as an automatic membership fallback. For longer retention, download both known-good artifacts before expiry.

## Switch the roster back to the classic card wall

The roster on `index.html` has two layouts. `list` (the default) is the searchable list and mugshot wall in `assets/roster.css` and `assets/roster.js`. `classic` is the original one-card-per-member wall. The builder reads the `ROSTER_LAYOUT` environment variable, and the deploy workflow sets it from the repository variable of the same name, so the layout can change without a commit:

1. In GitHub, open **Settings → Secrets and variables → Actions → Variables** and add a repository variable named `ROSTER_LAYOUT` with the value `classic`.
2. Open **Actions → Deploy → Run workflow** on `main`, or wait for the next hourly run. Changing the layout changes the site fingerprint, so the scheduled run deploys it.

To go back to `list`, delete the variable (or set it to `list`) and run the workflow again. Any other value fails the build before upload, so the live site stays as it is. Both layouts pass the same `validate-site.py` checks, and the classic markup is unchanged.

`ROSTER_LAYOUT` only changes how the roster looks. It is independent of the roster data contract version (`--schema-version` / `ROSTER_EXPORT_VERSION`): either layout renders either contract, and neither layout adds photos, places or other data the selected snapshot does not publish. Roll back one without touching the other.

To preview either layout locally, set the variable on the build: `ROSTER_LAYOUT=classic python3 scripts/build-roster.py --source json --roster-json <file> --output-dir dist`.

Re-deploying an older Pages artifact (below) also restores the old roster, but the next hourly run rebuilds from `main` and replaces it. Set `ROSTER_LAYOUT` for anything longer than an hour. The `pre-roster-list` branch marks the last commit before the list layout, in case the change has to be reverted in source.

## Roll back the exact published files

Each build that uploads a site retains its immutable Pages archive for **30 days**, named `github-pages-<run-id>-<build-attempt>`. A known-good artifact is one whose **deploy job succeeded** and whose site was verified. Failed builds cannot replace previous artifacts. The source SHA is recorded by the run; artifact names distinguish full build reruns so deployment cannot select a duplicate artifact name.

The procedure below is for an operator deliberately restoring production. It does not require a new commit, a DNS change, or another roster/photo fetch. It applies to runs using the split build/deploy workflow introduced here, while their artifacts and GitHub's 30-day rerun window remain available.

1. Find a previously verified successful production run. Wait for any active deployment to finish and account for pending runs before restoring:

   ```sh
   gh run list --repo brass-knuckle-gang/bkg.club --workflow deploy.yml --branch main --status success --limit 20
   gh run view RUN_ID --repo brass-knuckle-gang/bkg.club --json headSha,jobs --jq '{source: .headSha, jobs: [.jobs[] | {name, databaseId, conclusion}]}'
   gh api repos/brass-knuckle-gang/bkg.club/actions/runs/RUN_ID/artifacts --jq '.artifacts[] | {name, expired, expires_at}'
   ```

2. Confirm the selected run's latest completed build produced the verified known-good artifact, its corresponding `deploy` job succeeded, and the artifact is unexpired. A full workflow rerun changes that run's upstream build output: deploy-only reruns cannot select an older build attempt's artifact. Choose another verified run if its latest build is unsuitable. Use the deploy job's numeric **databaseId**, not the number embedded in the browser URL. Rerun **only that deploy job**:

   ```sh
   gh run rerun RUN_ID --repo brass-knuckle-gang/bkg.club --job DEPLOY_JOB_DATABASE_ID
   gh run watch RUN_ID --repo brass-knuckle-gang/bkg.club --exit-status
   ```

   GitHub retains the completed upstream build's outputs on a downstream job rerun, so `needs.build.outputs.artifact_name` still names its original archive. Do not rerun the build or all jobs for rollback: those would fetch current roster and photo data. See [GitHub job reruns](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs) and [the CLI job-ID requirement](https://cli.github.com/manual/gh_run_rerun).

3. Verify the homepage, roster/count, `members.txt`, nearby/downline/outbreak pages, and photos at `https://www.bkg.club/`. Record the restored run/artifact and resolve the cause of the bad release.

   The hourly schedule keeps running. The next scheduled build compares its fingerprint with the restored site's, so if the bad roster data or code is still in place it deploys again within the hour. Fix the cause first, or disable the workflow right after the restore (`gh workflow disable deploy.yml --repo brass-knuckle-gang/bkg.club`) and run `gh workflow enable` once it is fixed. Pushes, manual dispatches and the hourly schedule then publish normally.

For longer offline retention, download a verified artifact before expiry with `gh run download RUN_ID --repo brass-knuckle-gang/bkg.club --name ARTIFACT_NAME --dir saved-pages`. Save its `artifact.tar`, run/source SHA, build attempt, and successful deploy record together. An expired artifact cannot be restored by a job rerun; rebuilding an old source revision does not reproduce the old mutable roster and photo input.
