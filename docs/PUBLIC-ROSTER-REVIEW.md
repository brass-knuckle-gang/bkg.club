# Live public roster review

Comparison captured October 2, 2026, against the currently published `www.bkg.club` pages. The source was fetched **once anonymously** and validated against the complete v1 contract. Backend PR #10 merge revision `060a8334f13a5995065bf942fd83f94340268b76` has unchanged schema and fixtures, verified byte-for-byte against the vendored test files.

Snapshot: `generated_at` **2026-10-02T17:26:41.605Z**; `content_hash` **`sha256:5a97382075c41fa1074f9887f8488f7921e9268e3805741952c96d826255dc0c`**. All artifacts in the local preview were built from this saved snapshot. No Sheets fetch, private backend read, or relationship inference contributed to the build. Published pages were downloaded solely for comparison.

| Check | Published site | Reviewed preview | Reason |
| --- | --- | --- | --- |
| Member count/numbers | 614 | Same 614; none added/removed | Active exported membership is authoritative. |
| Current callsigns | QRZ-normalized values | Four changes: #11 KE7G → KM7ANM; #170 KZ0O → KF0UFI; #314 NU4J → N9FZ; #422 NB0E → KF0CZD | The reviewed export is authoritative; QRZ cannot rename it. |
| Display names | Legacy name overrides | #161 Max P → Max Praglin; #200 QRS Forrest → Forrest Filler | Full reviewed names replace legacy display aliases. |
| Map territories | Existing public territories | All 614 territory projections agree | Reviewed QTH supplies territory. |
| Join Dates | Existing parsed dates | All 614 parsed dates agree | Stored `qso_date` supplies Join Date, independent of export/approval time. |
| Sponsor edges | 613 | 296 resolved; 317 historical edges now null | Exported stable `sponsor_bkg_number` alone resolves relationships. Missing historical references stay unresolved. |
| Stored OG labels | Legacy inferred/generic OG note labels | 60 stored assignments/ribbons; five member-note tag sets differ | Stored labels replace inference and hardcoded generic OG note tags. A moved holder keeps its stored label. |
| Custom photos | Ten repository overrides | All ten sanitized and bound to current number + callsign | Metadata removed; reuse/inactive bindings rejected. |
| Nearby coordinates | 612 in the public site | None in this local preview | Local QRZ credentials were unavailable; no old geography was imported. Production requires a separate usable refresh/cache before upload. |

`members.txt` deliberately uses every member's full reviewed name, rather than abbreviated legacy note labels. Founder styling and page features remain. Stored assignments include the exact upstream labels (for example `AB`, `Alberta`, and `Austria`); the website does not coalesce upstream regions or choose a new OG by current QTH. Null sponsors appear as roots/unresolved members in downline; recruitment rankings and outbreak spread change accordingly. This is expected source behavior, not an invitation to restore historical edges from old pages or Sheets.

The local preview retains all seven pages, CSS/assets, CNAME, shared navigation, roster cards, territory map, downline, outbreak QSO-date timeline, recruitment form, application links, and existing social-card assets. No template redesign or form submission is part of this change. The homepage's existing decorative search field is unchanged; interactive page search is preserved. The separate nearby page remains functional but cannot place members without accepted enrichment. Production does not use the photo-only preview mode.

Offline tests cover fixture/source comparisons, anonymous and optional paired Access headers, redirect/error/timeout/size behavior, malformed/hash/schema/reference/empty input, snapshot reuse without network fallback, production `dist/` allowance, atomic preservation of previous outputs, inactive/reused callsign and moved-QTH enrichment rejection, QRZ geography mismatch, canonical photo sanitization, and unusable bootstrap rejection. Run them with `python3 -m unittest discover -s tests -v` after installing `requirements-build.txt`. Both Actions workflows also pass actionlint/shellcheck.

The local snapshot, sanitized photo cache, rendered site, and screenshots are outside the checkout at `/tmp/bkg-public-roster-preview/`. Serve `site/` on localhost to inspect; the [reproduction instructions](JSON-INPUT.md#reproduce-an-inspectable-local-preview) support either a full separately authenticated QRZ refresh or explicit `--overrides-only` preview. Operator review of live QRZ bootstrap/coverage remains a cutover prerequisite; no website deployment, secret/settings mutation, or Cloudflare/D1 changes were made.
