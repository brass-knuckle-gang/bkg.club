# bkg.club — working context

Public website of the Brass Knuckle Gang (BKG), a ham radio group. Live at www.bkg.club (GitHub Pages, `CNAME`).
Repo: `brass-knuckle-gang/bkg.club` (public).
Sibling repos in the org: `bkg-automation` (private; forms/automation, the roster publisher side) and `bkg-programmer` (public).

## Ownership
The form/automation side and the roster publisher live in `bkg-automation` and the administration-owned V2 publisher. Don't change the roster data contract or publisher behaviour from this repo without checking with the maintainers.

## How the site works
- Static site, built by `scripts/build-roster.py` into `dist/` and deployed to GitHub Pages by `.github/workflows/deploy.yml` (push to `main`, manual dispatch, and hourly at `:23`; unchanged scheduled builds skip deploy).
- Source pages: `index.html` (roster section is rebuilt between `<!-- ROSTER:START/END -->`, member count between `MEMBER_COUNT` markers), `badges.html`, `clubs.html`, `nearby.html`, `outbreak.html`, `recruit.html`, `safety.html`, `tree.html`. `tree`, `nearby` and `outbreak` get JSON payloads injected between their own START/END markers.
- Assets: `assets/bkg.css`, `assets/roster.css`, `assets/roster.js`. Roster has two layouts, `list` (default) and `classic`, chosen by the `ROSTER_LAYOUT` build variable.
- Roster data comes from a reviewed public JSON feed (contract V1 or V2, `ROSTER_EXPORT_VERSION`; V2 is the administration-owned publisher and skips QRZ enrichment). Secrets: `ROSTER_EXPORT_V2_URL` in the `github-pages` environment.
- `members.txt` is auto-generated (Ham2K PoLo callsign notes) — never hand-edit. `qrz-cache.json`, `name-overrides.txt`, `location-overrides.txt` are legacy/enrichment inputs.
- Public file allowlist and validation live in `scripts/site_contract.py` and `scripts/validate-site.py`. Any new public file must be added to the allowlist or it will not ship.
- PRs run offline fixture tests only (no secrets, no deploy). Pushes to `main` deploy.

## Commands
```sh
pip install --break-system-packages -r requirements-build.txt   # Pillow
python3 -m unittest discover -s tests -v                         # 113 tests, ~3s, all passing at 13af855
ROSTER_LAYOUT=classic python3 scripts/build-roster.py --source json --roster-json <file> --output-dir dist
```
Fixtures for local previews are in `tests/fixtures/public-roster/fixtures/`. See `docs/JSON-INPUT.md` for the local-preview recipe.

## Docs map
- `docs/DEPLOYMENT.md` — deploy, rollback (30-day Pages artifacts), layout switch, input-artifact recovery
- `docs/JSON-INPUT.md` — JSON roster contract and local preview
- `docs/PUBLIC-ROSTER-CUTOVER.md`, `PUBLIC-ROSTER-V2-CUTOVER.md`, `PUBLIC-ROSTER-V2-REVIEW.md`, `PUBLIC-ROSTER-REVIEW.md` — V1/V2 cutover history and comparisons
- `docs/PUBLISHER-DATA-INVENTORY.md` — which data the website vs administration owns
- `docs/DOWNLINE-SETUP.md` — tree page and recruit form setup
- `docs/mockups/` — roster-at-scale and mobile-readability mockups

## Working rules
- Changes go through a branch and PR. Pushes to `main` deploy to production.
- Run the offline tests before proposing a change.
