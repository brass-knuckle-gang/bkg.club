# GitHub Pages deployment and rollback

Production publishes the administration-owned **V2** roster from the
`ROSTER_EXPORT_V2_URL` secret in the `github-pages` environment. The
[October 21 cutover](PUBLIC-ROSTER-V2-CUTOVER.md) records the transition.
Administration handles QRZ refresh and contributed photos.

Pushes to `main`, manual dispatch on `main`, and the hourly schedule
`23 * * * *` trigger publication to GitHub Pages at `www.bkg.club`. GitHub
runs scheduled workflows on a best-effort basis, so new approvals can take
several hours to appear. Scheduled runs skip upload when the site would be
identical. Pushes and manual dispatches always deploy unless a dispatch sets
`skip_unchanged`. The `deploy` concurrency group serializes runs without
cancelling active deployment. Pull requests run offline checks with
read-only permissions outside the production environment.

## Build contract and deployment checks

The build checks out the event's exact `github.sha`, verifies HEAD, and runs the offline checks. It fetches and validates one roster response and saves it under `$RUNNER_TEMP/bkg-build`. The `bkg-roster-v2-*` cache supports conditional GET. Reusing a cached snapshot requires a matching ETag and a validated schema, hash, and endpoint. Transport, integrity, schema, and reference errors stop the build before upload. Invalid callsigns are the sole record-level exception. The original response is retained as `roster-source.json`, and `roster.json` contains the publication projection with its own hash. The fetch step reports excluded numbers and counts. Recruits of excluded sponsors retain null sponsor links, and excluded OG holders are never replaced. See the [membership rules](JSON-INPUT.md#reviewed-membership-rules).

### V2 photos (production)

The V2 build restores the `bkg-public-assets-v2-*` cache into `$RUNNER_TEMP/bkg-build/public-assets`, then `scripts/fetch-public-assets.py` checks every photo the saved snapshot references. It accepts only anonymous HTTPS assets on the snapshot's own origin at `/api/roster/photos/<sha256>.webp`, with no redirects, and verifies content type, byte length, SHA-256, and metadata-free still WebP structure. Accepted bytes are copied into the site without resizing or re-encoding. Cached files are reused only when they still match the snapshot. Map locations come verbatim from the snapshot's `map_location` fields. No QRZ login, enrichment cache, or website override is involved. See [Administration-owned v2 publishing](JSON-INPUT.md#administration-owned-v2-publishing).

### Build, validate, and deploy

Run `scripts/build-roster.py --roster-json <saved-file> --asset-dir <external-assets> --output-dir dist`. The builder stages and validates the site before replacing output. The public allowlist in `scripts/site_contract.py` covers the nine pages, shared assets, the game and its source, `members.txt`, verified photos, and `data/v2/roster.json`.

The workflow runs `scripts/validate-site.py dist`, verifies HEAD again, and compares the published roster envelope with its saved input. It fingerprints the output using `scripts/site-fingerprint.py`, then compares the digest with the newest `bkg-deployed-v1-*` Actions cache. The cache name predates V2 and still stores the fingerprint of the last successful deploy. An unchanged scheduled run skips artifact upload and deploy and still saves the roster and photo caches. For a changed build, the deploy job publishes the validated Pages artifact. A failed build or upload leaves the current site in place.

Run the offline checks that pull requests run, with Python 3.10+ and the pinned dependency (Pillow) installed, for example in a virtual environment:

```sh
python3 -m pip install -r requirements-build.txt
node --test tests/*.test.cjs
python3 -m unittest discover -s tests -v
```

Offline checks cover roster validation, conditional snapshot reuse, asset integrity,
atomic build failures, and browser behavior. Use the
[V2 preview commands](JSON-INPUT.md#administration-owned-v2-publishing) to
reproduce publication locally.

## Build input retention and recovery

A build retains `bkg-build-inputs-<run-id>-<build-attempt>` for **30 days**,
separate from the Pages archive, whenever it uploads a new site or excludes a
member. The input archive contains the exact source response
(`roster-source.json`), the publication snapshot (`roster.json`), hashes and
exclusions (`roster-record.json`), the source commit (`source-sha.txt`), and
verified `public-assets/`. An evicted V2 photo cache causes the next build to
download the snapshot's photos again.

## Switch the roster back to the classic card wall

The roster on `index.html` has two layouts. `list` (the default) is the searchable list and mugshot wall in `assets/roster.css` and `assets/roster.js`. `classic` is the original one-card-per-member wall. The builder reads the `ROSTER_LAYOUT` environment variable, and the deploy workflow sets it from the repository variable of the same name, so the layout can change without a commit:

1. In GitHub, open **Settings → Secrets and variables → Actions → Variables** and add a repository variable named `ROSTER_LAYOUT` with the value `classic`.
2. Open **Actions → Deploy → Run workflow** on `main`, or wait for the next scheduled run. Changing the layout changes the site fingerprint, so the scheduled run deploys it.

To go back to `list`, delete the variable (or set it to `list`) and run the workflow again. Any other value fails the build before upload, so the live site stays as it is. Both layouts pass the same `validate-site.py` checks, and the classic markup is unchanged.

To preview the classic layout, set `ROSTER_LAYOUT=classic` when running
`build-roster.py` in the [local preview recipe](JSON-INPUT.md#administration-owned-v2-publishing).

Re-deploying an older Pages artifact (below) also restores the old roster, but the next run that deploys rebuilds from `main` and replaces it, usually within a few hours. Set `ROSTER_LAYOUT` for anything longer-lived. The `pre-roster-list` branch marks the last commit before the list layout, in case the change has to be reverted in source.

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

   The schedule keeps running. The next scheduled build compares its fingerprint with the restored site's, so if the bad roster data or code is still in place it deploys again, typically within a few hours and possibly within minutes. Fix the cause first, or disable the workflow right after the restore (`gh workflow disable deploy.yml --repo brass-knuckle-gang/bkg.club`) and run `gh workflow enable` once it is fixed. Pushes, manual dispatches and the schedule then publish normally.

For longer offline retention, download a verified artifact before expiry with `gh run download RUN_ID --repo brass-knuckle-gang/bkg.club --name ARTIFACT_NAME --dir saved-pages`. Save its `artifact.tar`, run/source SHA, build attempt, and successful deploy record together. An expired artifact cannot be restored by a job rerun; rebuilding an old source revision does not reproduce the old mutable roster and photo input.
