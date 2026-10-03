# GitHub Pages deployment and rollback

Production builds reviewed JSON from the public roster feed. GitHub Pages, `www.bkg.club`, and CNAME/DNS remain unchanged. Production triggers are pushes to `main`, manual dispatch on `main`, and `23 * * * *` (hourly). A scheduled run that would publish the same site as the last deploy stops after validation; pushes and manual dispatches always deploy. The `deploy` concurrency group serializes runs without cancelling active deployment. Pull requests run only offline fixture checks with read-only permissions and no production environment/secrets. Creating or updating a draft PR cannot deploy this site.

See [cutover instructions](PUBLIC-ROSTER-CUTOVER.md) for operator configuration, and [JSON input rules](JSON-INPUT.md) for the reviewed contract and sanitized enrichment.

## Build contract

The build checks out the event's exact `github.sha`, verifies HEAD, installs the pinned image dependency, and runs offline tests. It fetches/validates one anonymous roster response using the Actions secret `ROSTER_EXPORT_URL`, saving it under `$RUNNER_TEMP/bkg-build`, outside `dist/` and the checkout. Missing/invalid input fails before upload. Refresh and build consume this same file; neither selects Sheets or fixtures on failure.

A separate QRZ refresh restores only sanitized, identity/QTH-bound enrichment into `$RUNNER_TEMP/bkg-build/enrichment`. It filters to active exported identities, validates location matches, sanitizes photos, and applies reviewed custom-photo bindings. Each run looks up only members that are due: new members, members whose reviewed QTH moved, and the least recently checked share of the roster sized so every member is rechecked within 24 hours of hourly runs (`--max-age-hours 24 --interval-hours 1`), using four concurrent lookups. A photo is downloaded again only when QRZ's image URL changes; the cache stores a SHA-256 of that URL, never the URL. Photos are downscaled to 480 px and re-encoded as metadata-free WebP; cached photos are verified structurally (single still image, no EXIF/XMP/ICC/animation/unknown chunks, full decode) on every read. Version 1 caches with full-size PNG photos are re-sanitized on the first refresh. The existing environment-scoped QRZ secrets remain available. A fresh cache must contain accepted geography and photos before the workflow can build; subsequent QRZ outages can reuse matching last-known-good enrichment. No authenticated roster credentials are needed. No raw QRZ data or cache-persistence commits enter this workflow.

The builder explicitly uses `--source json --roster-json <saved-file> --enrichment-dir <external-cache> --output-dir dist`. It stages and validates the full site before replacing output. The explicit public allowlist preserves all eight pages, CNAME, favicon/logo, CSS, the mobile mockup, member notes, and active sanitized photos; JSON mode also publishes `data/v1/roster.json`. Source page templates retain search, maps, recruitment, downline, application links, and existing social-card assets. Private inputs and raw/cache metadata are excluded.

The validator explicitly uses `--source json`, checks public file/link/roster/date/reference consistency, and validates the public envelope against page identities and stable sponsors. The workflow verifies HEAD again and compares the complete published envelope against its saved input. It fingerprints `dist/` with `scripts/site-fingerprint.py`, which ignores only the build timestamps and the export's `generated_at`, and compares it with the newest `bkg-deployed-v1-*` Actions cache, which the deploy job saves after each successful deploy. A scheduled run with an identical fingerprint skips the remaining upload and deploy steps but still saves the refreshed enrichment cache. Otherwise it retains a separate immutable input/enrichment archive and uploads the immutable Pages archive. Cache-save failure is nonblocking because the retained input archive is already available. The deploy job depends on successful build completion and publishes the selected artifact without checkout/refetch/rebuild. Build/input/validation/upload failures leave the live deployment in place.

Run offline checks with the dependency installed:

```sh
python3 -m unittest discover -s tests -v
```

Tests block live networking and exercise anonymous/optional-auth fetches, snapshot reuse, malformed/hash/reference/empty input, atomic failures, unchanged previous outputs, active enrichment filtering, stale location rejection, and metadata removal. Local preview commands are in [JSON-INPUT.md](JSON-INPUT.md#reproduce-an-inspectable-local-preview).

## Enrichment retention and recovery

Each successful build retains `bkg-build-inputs-<run-id>-<build-attempt>` for **30 days**, separate from `github-pages-<run-id>-<build-attempt>`. It contains the exact reviewed snapshot, aggregate hash/timestamp record, `source-sha.txt`, and sanitized `enrichment/` cache/photos. No URL, credentials, source photo URLs, or raw QRZ responses are stored. The Actions cache key `bkg-enrichment-v1-<run-id>-<attempt>` restores the newest successfully validated cache available on the branch. GitHub may evict caches; the immutable archive provides independent recovery.

For cache recovery, download a matching known-good input artifact with `gh run download RUN_ID --repo brass-knuckle-gang/bkg.club --name INPUT_ARTIFACT_NAME --dir saved-inputs`, verify its hash/source record, and retain `enrichment/` together with its canonical photos. A local rebuild can use that external directory with the newly saved roster; stale/inactive fields are filtered again. Recovery to the Actions cache needs an operator-reviewed cache seed/repair in the normal build environment, or a successful QRZ refresh; it must not upload an unvalidated public artifact or use the retained snapshot as an automatic membership fallback. For longer retention, download both known-good artifacts before expiry.

## Roll back the exact published files

Each build retains its immutable Pages archive for **30 days**, named `github-pages-<run-id>-<build-attempt>`. A known-good artifact is one whose **deploy job succeeded** and whose site was verified. Failed builds cannot replace previous artifacts. The source SHA is recorded by the run; artifact names distinguish full build reruns so deployment cannot select a duplicate artifact name.

The procedure below is for an operator deliberately restoring production. It does not require a new commit, a DNS change, or another Sheets/QRZ fetch. It applies to runs using the split build/deploy workflow introduced here, while their artifacts and GitHub's 30-day rerun window remain available.

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

   GitHub retains the completed upstream build's outputs on a downstream job rerun, so `needs.build.outputs.artifact_name` still names its original archive. Do not rerun the build or all jobs for rollback: those would fetch current roster/QRZ data. See [GitHub job reruns](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs) and [the CLI job-ID requirement](https://cli.github.com/manual/gh_run_rerun).

3. Verify the homepage, roster/count, `members.txt`, nearby/downline/outbreak pages, and photos at `https://www.bkg.club/`. Record the restored run/artifact and resolve the cause of the bad release; the unchanged push/manual/six-hour triggers can publish another build afterward.

For longer offline retention, download a verified artifact before expiry with `gh run download RUN_ID --repo brass-knuckle-gang/bkg.club --name ARTIFACT_NAME --dir saved-pages`. Save its `artifact.tar`, run/source SHA, build attempt, and successful deploy record together. An expired artifact cannot be restored by a job rerun; rebuilding an old source revision does not reproduce the old mutable roster/QRZ input.
