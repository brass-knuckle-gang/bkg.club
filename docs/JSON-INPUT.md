# Optional JSON roster input

This is the experimental JSON-input portion of [#30](https://github.com/jsvana/bkg.club/issues/30). Google Sheets remains the default and the production deployment still builds and uploads `dist/` to GitHub Pages. Production triggers, `www.bkg.club`, DNS, hosting, and QRZ secret configuration are unchanged. This change does not switch production membership sources.

The input contract is the schema and synthetic fixtures from merged [bkg-automation PR #5](https://github.com/brass-knuckle-gang/bkg-automation/pull/5), pinned to merge revision `d6b60f96c1f403a327e621fb9ed4d04146077100`. Exact copies are under [`tests/fixtures/public-roster/`](../tests/fixtures/public-roster/). Fixture data is used only by offline tests or an explicitly requested local fixture build. An input failure never selects a fixture, demo roster, Sheets, or another source as a fallback.

## Render a local export outside the production artifact

JSON requires explicit `--source json`. Its output and optional enrichment directory must be outside the repository and separate from each other. The builder rejects JSON output in `dist/`, elsewhere inside the repository, or over its inputs. The existing default command still selects Sheets even when JSON environment variables are present.

```sh
json_trial_dir="$(mktemp -d "${TMPDIR:-/tmp}/bkg-json.XXXXXX")"
python3 -m venv "$json_trial_dir/venv"
. "$json_trial_dir/venv/bin/activate"
python3 -m pip install -r requirements-build.txt
python3 scripts/build-roster.py \
  --source json \
  --roster-json tests/fixtures/public-roster/fixtures/roster-v1.json \
  --output-dir "$json_trial_dir/site"
python3 scripts/validate-site.py --source json "$json_trial_dir/site"
```

The dependency supplies full photo decoding and sanitization for fixture checks and explicit refresh. A JSON render with no photos can run without it. Use a saved reviewed export in place of the fixture to inspect real source differences. An optional `--enrichment-dir "$json_trial_dir/enrichment"` reads an existing sanitized cache. Rendering performs no QRZ login, lookup, download, or cache refresh. The existing Sheets path retains its current QRZ behavior so production remains unchanged.

JSON produces the existing roster cards/count, territory map, nearby, downline, outbreak, and `members.txt` files. It also writes `data/v1/roster.json`, containing the validated reviewed contract alone. Production validation rejects that additional file unless explicitly invoked with `--source json`; the Sheets artifact never gains it. The JSON cache, photo-source metadata, credentials, scripts, and input files are not copied into the artifact.

## Reviewed membership rules

`callsign`, `name`, and `qth` are authoritative reviewed values. JSON rendering does not apply legacy name/location overrides or allow QRZ to rename, relocate, or introduce a member. `members.txt` uses the full reviewed name; the legacy Sheets path still uses its abbreviated note labels.

`sponsor_bkg_number` resolves through the stable exported membership number to the sponsor's current reviewed callsign. Null references stay null. Historical callsigns, stale cache records, or a later member reusing a callsign cannot recreate a suppressed sponsor edge.

`qso_date` supplies the existing Join Date semantics. The original text remains intact in the public contract; outbreak uses the builder's existing date parser and leaves empty/unreadable dates null. Export generation time, approval time, and member issuance time never replace a QSO date.

OG badges and note labels come only from stored `og_assignments`. A holder who moves from Vermont to New York keeps `VT OG`; current QTH places that member on the New York map. JSON does not choose a new holder by current state, member number, or enrichment. Inactive members, inactive sponsors, and assignments with inactive holders are suppressed upstream; the builder validates all remaining references and never restores them from a cache.

Validation rejects unsupported schema versions, additional/private fields, duplicate identities or JSON keys, unsorted arrays, invalid sponsor/OG references, sponsor cycles, unsafe text, and mismatched content hashes. Empty active rosters are rejected even though the upstream empty fixture is valid API output. HTML and embedded JSON escape reviewed text before publication. Generation and validation complete in a temporary directory before the previous output can be replaced.

## Refresh QRZ enrichment separately

The opt-in refresh command consumes the same validated roster and writes a cache outside the repository. Credentials come from the environment; do not place them in command arguments, saved fixtures, or logs.

```sh
# QRZ_USERNAME and QRZ_PASSWORD must already be supplied securely.
python3 scripts/refresh-qrz.py \
  --roster-json /absolute/path/to/reviewed-roster.json \
  --enrichment-dir "$json_trial_dir/enrichment"
python3 scripts/build-roster.py \
  --source json \
  --roster-json /absolute/path/to/reviewed-roster.json \
  --enrichment-dir "$json_trial_dir/enrichment" \
  --output-dir "$json_trial_dir/site"
```

The cache contains only identity-bound coarse geographic fields and sanitized local photo references. It retains last-known-good values/photos for matching reviewed identities/QTH when QRZ login, lookups, or photo downloads fail. Malformed roster input fails before cache writes. Photos are fully decoded and saved as canonical PNGs using their first frame with metadata removed; invalid image data cannot replace a previous photo. Raw QRZ XML, identity suggestions, image URLs, and private application fields do not become public data.

Rendering selects cache entries only for active exported numbers with matching reviewed callsigns. A callsign change invalidates an old enrichment binding until an explicit refresh creates the new binding. Coarse geography also requires a matching hash of the reviewed QTH: moving suppresses stale coordinates while retaining the identity-bound photo. A missing or unusable cache cannot remove reviewed roster members: QTH still drives the map and outbreak territory fallback, while nearby excludes members without usable coarse coordinates. Only selected sanitized photos are staged; unrelated legacy/cache photos are excluded.

The JSON reader can also fetch an authenticated endpoint when `--roster-json` is omitted. It requires `ROSTER_EXPORT_URL` and environment secrets `CF_ACCESS_CLIENT_ID` and `CF_ACCESS_CLIENT_SECRET`, and sends the Cloudflare Access service-token headers. It requires HTTPS, refuses redirects, bounds response size/time, and does not log credentials, URLs, or response bodies. Authentication/fetch/contract failures stop the build and preserve the previous output. Endpoint fetch is not enabled in production by this PR.

## Offline comparison results

Run `python3 -m unittest discover -s tests -v` without production credentials. Tests block network connections and use temporary source/output/cache directories. [`test_json_build.py`](../tests/test_json_build.py) constructs a matched synthetic Sheets CSV from the exact export fixture, supplies identical geographic enrichment, and compares every existing output.

| Output | Synthetic comparison result |
| --- | --- |
| Roster cards and count | Same 6 reviewed identities, names, numbers, order, and count; cards otherwise match after removing intentional OG ribbons. |
| Territory map | Identical 4 mapped members: two in IL, one in NY, one in Canada; two unknown QTHs remain unmapped. |
| Nearby | Identical empty projection without enrichment; identical 3-member projection with matched grids/rounded coordinates. |
| Downline | Identical 6 nodes and 3 stable sponsor edges. |
| Outbreak | Identical 6 identities, sponsor edges, territories, 5 parsed QSO dates, and optional coarse coordinates; the missing date stays null. |
| `members.txt` | Same 6 callsign/number records. JSON uses full reviewed names and only stored OG assignments; Sheets retains abbreviated names and inferred OGs. |
| Public JSON snapshot | Exact validated upstream envelope, including its hash and historical date text; absent from Sheets output. |

The intentional OG differences are explicit: Sheets infers IL for #1 and NY for moved holder #88, while JSON renders stored IL for #7 and VT for #88. Both retain Canada for #21; JSON does not add the hardcoded founder/number OG note. Additional tests remove inactive #7 and its IL assignment, leave IL member #1 without an OG, and reuse #7's callsign on #40 while #21's suppressed sponsor stays null. Tests also exercise reviewed callsign changes, moved QTH suppressing stale coordinates while keeping a photo/stored OG, missing/corrupt enrichment, filtered inactive cache/photos, escaped HTML/script text, and malformed/empty exports preserving the previous output. Separate enrichment tests cover failed refresh retaining sanitized geography and photos.

These are synthetic comparisons. A real reviewed export has not been fetched or compared against production Sheets, and no production source switch or deployment is part of this change.

## Checklist for a later production source switch

Each unchecked item belongs to a separate reviewed rollout. This checklist does not authorize provisioning, a source switch, or deployment.

- [ ] Verify the upstream exporter is deployed/configured at the agreed contract version, or deliberately review schema/fixture changes before updating the website adapter.
- [ ] Configure a dedicated Access application for the exact export hostname/path `/api/roster/export`, with a distinct audience from human administration and a Service Auth policy restricted to the builder's exact service token. Preserve existing administrator access. Follow the upstream [operator configuration](https://github.com/brass-knuckle-gang/bkg-automation/blob/d6b60f96c1f403a327e621fb9ed4d04146077100/docs/public-roster.md#later-operator-configuration).
- [ ] Configure the upstream trusted issuer, dedicated audience, and exact service-token Client ID. Store `CF_ACCESS_CLIENT_ID`/`CF_ACCESS_CLIENT_SECRET` in the website CI secret store and set the full HTTPS `ROSTER_EXPORT_URL`. Keep the Client Secret out of Worker configuration, source control, logs, and artifacts.
- [ ] Verify authorized GET access and rejection of missing credentials, other service tokens, human assertions, administrator-route access by the machine identity, and non-GET mutations. Confirm fetch errors/redirects fail without exposing secrets.
- [ ] Save a real reviewed export and matched Sheets snapshot privately. Compare active membership/count/names/callsigns/QTH, every sponsor reference, QSO-date text/normalization, OG holders/stored labels, maps, nearby/outbreak coordinates, and `members.txt`. Record and explicitly review every intentional difference, particularly inactive people/holders, moved OGs, unresolved sponsors, and callsign reuse.
- [ ] Explicitly refresh a sanitized enrichment cache for that reviewed export and inspect its active identity/QTH bindings and canonical photos. For later CI, restore only the last successfully validated sanitized cache artifact into a directory outside the checkout and Pages output; filter it against the current active export. Keep refresh as an explicit command separate from rendering, retain usable prior values/photos on refresh failure, and agree its execution policy without changing existing schedules.
- [ ] Retain the sanitized cache and photos together for at least 30 days in a private artifact separate from the Pages artifact, recording source/run identity and the export hash. Define expiry/bootstrap behavior before switching: a missing/expired cache requires an explicit successful sanitized refresh and validation, or a deliberately reviewed roster-only baseline; a failed bootstrap retains the current live site. Preserve the matching cache alongside each known-good Pages artifact so rollback restores exact public files immediately and has a verified enrichment baseline for later builds.
- [ ] Build and validate the real export into an external comparison directory. Confirm the artifact allowlist excludes secrets, cache/input files, raw QRZ data, and fixtures. Test authentication, malformed/hash/reference/empty-input failures against an existing good output and verify it is preserved.
- [ ] Retain a verified successful Pages artifact, source/run identity, and rollback procedure from [DEPLOYMENT.md](DEPLOYMENT.md). Establish the comparison result and source-selection change that the operator will approve.
- [ ] In a separate PR, deliberately amend the guard that currently forbids JSON in `dist/`, opt the build and validator into JSON, and review whether `data/v1/roster.json` becomes a production public file. Preserve fixed-revision checkout, validation-before-upload, immutable artifact selection, deployment serialization, secret boundaries, and rollback behavior. Keep GitHub Pages, CNAME/DNS/hosting, and existing deployment schedules unchanged unless separately requested.
- [ ] Obtain approval for that concrete production change, then merge through the existing reviewed workflow. Verify the published roster/count, map, nearby/downline/outbreak, notes, and photos, and record the known-good JSON artifact. Do not deploy manually as part of this experiment.
