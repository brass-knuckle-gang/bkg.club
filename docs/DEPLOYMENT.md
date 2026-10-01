# GitHub Pages deployment and rollback

This implements only the deployment-safety portion of [#30](https://github.com/jsvana/bkg.club/issues/30). GitHub Pages, `www.bkg.club`, the Google Sheets CSV URL, and the existing production triggers remain unchanged: pushes to `main`, manual dispatch on `main`, and `0 */6 * * *` (every six hours). The `deploy` concurrency group still serializes runs without cancelling an active deployment. Pull requests run only offline fixture checks with read-only permissions and no production environment or secrets.

The optional [JSON input experiment](JSON-INPUT.md) builds outside the repository and uses a separate enrichment cache. It is not selected by this workflow. Its `/data/v1/roster.json` is rejected by the default production validator; a later source switch requires the checklist in that document.

## Build contract

`python3 scripts/build-roster.py` writes `dist/`. The explicit allowlist in `scripts/site_contract.py` preserves all seven public pages, `CNAME`, favicon/logo, shared CSS, the existing public mobile mockup, `members.txt`, and supported photo files. Repository scripts, QRZ cache, name/location override inputs, and photo-source metadata are excluded. Source page templates are unchanged; only their existing generated sections are replaced in the public copies.

The build job checks out the event's exact `github.sha`, runs fixtures, and fetches the existing Google Sheets input once. Existing QRZ callsign/location enrichment, local photo overrides, unchanged-photo download avoidance, failed-lookup/download photo reuse, and mugshot Actions caching are preserved. The build retains the `github-pages` environment so environment-scoped QRZ secrets remain available. Sponsor resolution still follows canonical QRZ callsign updates.

Generation rejects empty or duplicate rosters and missing templates/markers. Validation requires every public file and local page link, matching roster cards/counts/notes, valid embedded JSON, consistent member identities/sponsors/territories, and finite coordinates. A QRZ outage retains the existing cached-callsign/photo behavior; nearby coordinates may be absent, as before.

Only a successful generation and validation can upload a Pages artifact. The separate deploy job depends on successful build completion and publishes that artifact without checking out or rebuilding source. Fetch, generation, validation, or upload failure leaves the current live deployment in place.

After upload, QRZ-cache persistence uses a separate clean Git worktree at the original source revision. Only that worktree commits, fetches/rebases against current `main`, and pushes the cache delta. It cannot change the build checkout or uploaded files. Persistence conflicts/failures are reported but do not block deployment of the validated artifact. Bot-token pushes and `[skip ci]` retain the existing loop prevention.

Install the pinned photo-validation dependency in an isolated environment
(see [JSON setup](JSON-INPUT.md#render-a-local-export-outside-the-production-artifact)),
then run the checks without credentials or network access:

```sh
python3 -m unittest discover -s tests -v
```

The tests use synthetic CSV/QRZ responses and temporary files, and block network connections. They never submit applications, send mail, or write production data. For an intentionally requested live-input local build, run the builder followed by `python3 scripts/validate-site.py dist`; local builds do not deploy.

## Roll back the exact published files

Each build retains its immutable Pages archive for **30 days**, named `github-pages-<run-id>-<build-attempt>`. A known-good artifact is one whose **deploy job succeeded** and whose site was verified. Failed builds cannot replace previous artifacts. The source SHA is recorded by the run; artifact names distinguish full build reruns so deployment cannot select a duplicate artifact name.

The procedure below is for an operator deliberately restoring production. It does not require a new commit, a DNS change, or another Sheets/QRZ fetch. It applies to runs using the split build/deploy workflow introduced here, while their artifacts and GitHub's 30-day rerun window remain available.

1. Find a previously verified successful production run. Wait for any active deployment to finish and account for pending runs before restoring:

   ```sh
   gh run list --repo jsvana/bkg.club --workflow deploy.yml --branch main --status success --limit 20
   gh run view RUN_ID --repo jsvana/bkg.club --json headSha,jobs --jq '{source: .headSha, jobs: [.jobs[] | {name, databaseId, conclusion}]}'
   gh api repos/jsvana/bkg.club/actions/runs/RUN_ID/artifacts --jq '.artifacts[] | {name, expired, expires_at}'
   ```

2. Confirm the selected run's latest completed build produced the verified known-good artifact, its corresponding `deploy` job succeeded, and the artifact is unexpired. A full workflow rerun changes that run's upstream build output: deploy-only reruns cannot select an older build attempt's artifact. Choose another verified run if its latest build is unsuitable. Use the deploy job's numeric **databaseId**, not the number embedded in the browser URL. Rerun **only that deploy job**:

   ```sh
   gh run rerun RUN_ID --repo jsvana/bkg.club --job DEPLOY_JOB_DATABASE_ID
   gh run watch RUN_ID --repo jsvana/bkg.club --exit-status
   ```

   GitHub retains the completed upstream build's outputs on a downstream job rerun, so `needs.build.outputs.artifact_name` still names its original archive. Do not rerun the build or all jobs for rollback: those would fetch current Sheets/QRZ data. See [GitHub job reruns](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs) and [the CLI job-ID requirement](https://cli.github.com/manual/gh_run_rerun).

3. Verify the homepage, roster/count, `members.txt`, nearby/downline/outbreak pages, and photos at `https://www.bkg.club/`. Record the restored run/artifact and resolve the cause of the bad release; the unchanged push/manual/six-hour triggers can publish another build afterward.

For longer offline retention, download a verified artifact before expiry with `gh run download RUN_ID --repo jsvana/bkg.club --name ARTIFACT_NAME --dir saved-pages`. Save its `artifact.tar`, run/source SHA, build attempt, and successful deploy record together. An expired artifact cannot be restored by a job rerun; rebuilding an old source revision does not reproduce the old mutable Sheets/QRZ input.
