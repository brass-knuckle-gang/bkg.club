# Reviewed public JSON roster

Production uses the public feed from merged [bkg-automation PR #10](https://github.com/brass-knuckle-gang/bkg-automation/pull/10). The v1 schema and fixtures remain byte-for-byte identical to the copies introduced in [#36](https://github.com/brass-knuckle-gang/bkg.club/pull/36). They were rechecked against backend merge revision `060a8334f13a5995065bf942fd83f94340268b76` at `docs/public-roster/schema-v1.json`, `docs/public-roster/fixtures/roster-v1.json`, and `docs/public-roster/fixtures/empty-v1.json`.

The production workflow explicitly selects `--source json` for both builder and site validator. The URL comes from the **Actions secret** `ROSTER_EXPORT_URL`, not an Actions variable. See [exact cutover instructions](PUBLIC-ROSTER-CUTOVER.md) and [live comparison results](PUBLIC-ROSTER-REVIEW.md). The command-line default remains Sheets for compatibility with explicitly requested legacy/offline comparisons; production never invokes it or falls back to it.

## One snapshot per run

`fetch-roster.py` makes one anonymous HTTPS GET, validates the complete response, and atomically saves `$RUNNER_TEMP/bkg-build/roster.json` outside the checkout and published output. Refresh, build, and validation consume that file. Consumers independently revalidate the saved bytes; they never refetch the URL. The final validation also compares the public JSON envelope with the saved input.

HTTPS, redirect rejection, the 30-second transport timeout, 8 MiB input limit, sanitized errors, schema/hash/reference checks, and empty-roster rejection are preserved. Optional Access headers are supported by the reader for other endpoints only when both credential fields are valid; an incomplete or empty pair fails before requesting. The deployed public feed needs neither header nor API token. Missing configuration or invalid input stops the workflow before artifact upload; the last published site remains available.

The output guard deliberately permits JSON in `dist/` as well as external preview directories. It still rejects source/ancestor directories, arbitrary output inside the checkout, input under the output directory, and enrichment overlapping the checkout/output. Public output contains the existing pages/assets, selected sanitized photos, `members.txt`, and the validated reviewed envelope at `data/v1/roster.json`. The private input file, enrichment cache, scripts, fixtures, credentials, raw QRZ data, and photo-source metadata are excluded.

## Reviewed membership rules

`callsign`, `name`, and `qth` are authoritative reviewed values. JSON rendering does not apply legacy name/location overrides or allow QRZ to rename, relocate, or introduce a member. `members.txt` uses the full reviewed name; the legacy Sheets path still uses its abbreviated note labels.

`sponsor_bkg_number` resolves through the stable exported membership number to the sponsor's current reviewed callsign. Null references stay null. Historical callsigns, stale cache records, or a later member reusing a callsign cannot recreate a suppressed sponsor edge.

`qso_date` supplies the existing Join Date semantics. The original text remains intact in the public contract; outbreak uses the builder's existing date parser and leaves empty/unreadable dates null. Export generation time, approval time, and member issuance time never replace a QSO date.

OG badges and note labels come only from stored `og_assignments`. A holder who moves from Vermont to New York keeps `VT OG`; current QTH places that member on the New York map. JSON does not choose a new holder by current state, member number, or enrichment. Inactive members, inactive sponsors, and assignments with inactive holders are suppressed upstream; the builder validates all remaining references and never restores them from a cache.

Validation rejects unsupported schema versions, additional/private fields, duplicate identities or JSON keys, unsorted arrays, invalid sponsor/OG references, sponsor cycles, unsafe text, and mismatched content hashes. Empty active rosters are rejected even though the upstream empty fixture is valid API output. HTML and embedded JSON escape reviewed text before publication. Generation and validation complete in a temporary directory before the previous output can be replaced.

## Separate photo and geographic enrichment

The workflow restores a sanitized cache outside the checkout, runs `refresh-qrz.py` as a separate command with the **same saved roster**, then renders without networking. QRZ credentials retain their existing `github-pages` environment scope. Missing credentials fail configuration. QRZ outages retain usable prior fields/photos for matching active exported identities; they do not select another roster. A fresh or expired cache needs a successful refresh with usable geography and photos (`--require-usable`); an unusable bootstrap fails before cache replacement and retains the published site.

Cache entries require the current active BKG number and exact reviewed callsign. Geography additionally requires a hash of the current reviewed QTH. A callsign change rejects the old binding; a QTH change suppresses stale geography while keeping an identity-bound photo. Refresh rejects QRZ geography unless its state/country agrees with the reviewed location. A reviewed Canadian province name (`Alberta`, `Ontario`, …) parses as that province within Canada, so it is compared with QRZ's `Canada` plus the province code QRZ reports in `<state>`; because QRZ documents that field as US-only, a blank QRZ province still agrees, while a US state must always match. QRZ aliases never rename an exported member. Coordinates are rounded to two decimals or reduced to four-character grids. Nearby omits members without accepted geography; the territory map and outbreak territory fallback always use reviewed QTH.

Custom photos are explicitly bound by both number and callsign in [`images/photo-overrides.json`](../images/photo-overrides.json), reviewed against the live export on October 2, 2026. These 10 existing overrides take precedence over QRZ photos. A reused callsign on a different member number cannot inherit an old override; inactive or stale bindings are ignored. To associate a replacement callsign/photo, review and update the manifest deliberately. Photos are fully decoded, downscaled to at most 480 px, and re-encoded as first-frame WebP with metadata removed before caching/publication; cached copies are re-verified structurally on every read. Only active selected sanitized photos enter the build.

The validated input and sanitized cache are retained together in a separate immutable `bkg-build-inputs-<run-id>-<attempt>` artifact for 30 days, with export hash, generated timestamp, and exact source SHA. Pages archives keep their existing names/retention. Cache restoration is an optimization; absence requires successful bootstrap. No raw QRZ cache is committed back to `main`. See [deployment and rollback](DEPLOYMENT.md) for archive/cache recovery.

## Reproduce an inspectable local preview

Supply `ROSTER_EXPORT_URL` securely in the environment, without printing it. The URL secret hides the URL in Actions logs; it does not authenticate the request.

```sh
preview_dir="$(mktemp -d "${TMPDIR:-/tmp}/bkg-public-roster.XXXXXX")"
python3 -m venv "$preview_dir/venv"
. "$preview_dir/venv/bin/activate"
python3 -m pip install -r requirements-build.txt
python3 scripts/fetch-roster.py --output "$preview_dir/roster.json"
# QRZ_USERNAME and QRZ_PASSWORD must already be supplied securely.
python3 scripts/refresh-qrz.py \
  --roster-json "$preview_dir/roster.json" \
  --enrichment-dir "$preview_dir/enrichment" \
  --photo-overrides images/photo-overrides.json --require-usable
python3 scripts/build-roster.py --source json \
  --roster-json "$preview_dir/roster.json" \
  --enrichment-dir "$preview_dir/enrichment" --output-dir "$preview_dir/site"
python3 scripts/validate-site.py --source json "$preview_dir/site"
python3 -m http.server 8765 --bind 127.0.0.1 --directory "$preview_dir/site"
```

For a deliberately limited local preview without QRZ credentials, replace `--require-usable` with `--overrides-only` in the refresh command. This seeds the existing custom photos without QRZ. Nearby will have no coordinates unless a matching sanitized cache was already supplied. Production never uses this option.

Fixtures are only for offline tests or an explicitly requested fixture build. To render one, replace the fetch with a local fixture path passed to `--roster-json`; never configure a fixture as a production fallback.

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

These are synthetic comparisons. The additional [live review](PUBLIC-ROSTER-REVIEW.md) records actual production differences and local enrichment limitations; the source switch requires the operator steps in [the cutover runbook](PUBLIC-ROSTER-CUTOVER.md).
