# Public roster cutover instructions

This draft implements the website source switch. The public backend is already deployed; do not change Cloudflare Workers, Access, D1, DNS, CNAME, or GitHub Pages hosting. The preparation described below is for the repository operator. No merge, website deployment, secret/settings change, or backend mutation was performed while preparing this PR.

## Prepare before approving the source switch

1. Review the [live comparison](PUBLIC-ROSTER-REVIEW.md), especially reviewed names/callsigns, stored OG labels, and the 317 historical sponsor edges now left unresolved. Inspect a live-feed preview and the offline checks on this draft. Missing historical sponsor edges require reviewed upstream data correction if warranted; the website must not infer them from callsigns or Sheets.

2. In **brass-knuckle-gang/bkg.club → Settings → Secrets and variables → Actions → New repository secret**, create an **Actions secret** named exactly **`ROSTER_EXPORT_URL`**, with this exact value and **no query parameters**:

   ```text
   https://bkg-public-roster.cool-cake-fac1.workers.dev/api/roster/export
   ```

   Use a repository secret, not the Variables tab. The workflow reads it as:

   ```yaml
   env:
     ROSTER_EXPORT_URL: ${{ secrets.ROSTER_EXPORT_URL }}
   ```

   Do not print its value in shell commands, diagnostic output, workflow logs, or artifacts. This secret masks the URL in Actions logs; it **does not authenticate requests**. The endpoint supports anonymous HTTPS GET. No Access credentials or API token are required, and the production workflow supplies none. Remove any stale environment-level secret of the same name if it would override this repository secret, after verifying its scope; this PR does not change settings.

3. Confirm the existing **`github-pages` environment** still makes `QRZ_USERNAME` and `QRZ_PASSWORD` available to the build job. These are for separate photo/geographic enrichment only. Missing credentials fail the build. The first JSON run has no sanitized cache unless explicitly seeded; it must successfully refresh accepted geography and photos. A QRZ outage during an empty bootstrap fails before upload and retains the current live site. The ten custom-photo bindings are already provided, but they do not satisfy the geographic bootstrap requirement.

4. Before merging, run the [local preview commands](JSON-INPUT.md#reproduce-an-inspectable-local-preview) at the exact reviewed PR revision with securely supplied roster URL and QRZ credentials. Keep the roster snapshot and enrichment directory outside the checkout/output. Use `--require-usable`, then build and validate with `--source json`. Review accepted photo/geography coverage, the territory map, nearby, downline, outbreak, notes, and existing application/recruitment links. The checked-in live comparison's photo-only preview demonstrates the roster contract; it does not certify a live QRZ bootstrap. Do not submit applications while inspecting.

5. Record and download a verified successful **currently deployed** Pages artifact and run/source SHA before expiry. Use the [rollback procedure](DEPLOYMENT.md#roll-back-the-exact-published-files). A deploy-only rerun restores that exact archive; rebuilding an older revision fetches mutable data and is not an exact rollback.

## Operator cutover after review

6. Mark the draft ready, obtain the normal review, and merge the reviewed PR into `main` only when configuration, enrichment bootstrap, preview differences, and rollback archive are accepted. **Merging triggers the existing Deploy workflow immediately.** This is the website cutover, so do not merge to test secret configuration. There is no separate hosting/provider/DNS migration.

7. Monitor that `main` run. Confirm its checkout/recorded SHA equals the merged revision; the saved roster summary contains only member count, hash, and timestamp; refresh consumes the saved file; build and validator both use JSON; validation occurs before either artifact upload. Missing URL, transport errors/redirects, oversize/invalid UTF-8, schema/hash/reference/empty input, unusable bootstrap, or failed validation must stop the build. Never rerun with Sheets, fixtures, an empty-roster override, or a different URL as a fallback.

8. After successful deployment, verify `https://www.bkg.club/`, all seven pages, roster count, search, stored OG ribbons, photos, territory and nearby maps, downline/recruitment/outbreak, `members.txt`, application links, and `/data/v1/roster.json`. Record the successful deploy run, exact source SHA, content hash, and both immutable artifact names: `github-pages-<run-id>-<build-attempt>` and `bkg-build-inputs-<run-id>-<build-attempt>`. Retain both for 30 days or download for longer retention. The separate input archive contains sanitized enrichment and reviewed input only.

9. Leave the existing `main` push/manual dispatch and every-six-hour schedule intact. The upstream API caches for up to five minutes, and website publication waits for a successful build/deployment; deactivation is not instantaneous on the website. If the first run fails, resolve its configuration/feed/enrichment issue and retry the normal `main` workflow while the prior site remains published. If a successful release is incorrect, rerun only a verified previous deploy job to restore its immutable artifact, then fix the source/data issue; account for pending scheduled runs as described in the rollback guide.
