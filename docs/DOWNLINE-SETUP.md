# Downline Tree + Recruit Form — Setup

This adds two pages to bkg.club:

- **`tree.html`** — an interactive "downline" family tree showing who recruited
  who into the gang: an indented, collapsible tree (mobile + desktop friendly)
  where each member shows their **direct** and **indirect** induction counts,
  plus a top-recruiters leaderboard.
- **`recruit.html`** — a form where a sponsoring member submits a new recruit's
  info. Submissions land in a Google Sheet for officers to review.

Both reuse the existing build pipeline: `scripts/build-roster.py` reads the
roster Google Sheet and regenerates the site on every push + hourly when something changed.

---

## 1. The tree data: add a `Sponsor` column to the roster sheet

The tree is built from **one new column** in the existing roster Google Sheet
(the same sheet that already has `Callsign, Name, Join Date, #, QTH`).

1. Open the roster sheet.
2. Add a column for **who recruited each member**. The build auto-detects the
   column by name (case-insensitive), so any of these work: `Sponsor`,
   `Sponsored By`, `Recruited By`, `Recruiter`, `Referred By`, `Upline`,
   `Elmer` — or any header containing "sponsor"/"recruit". The current sheet
   uses **`Recruited By`**. The build log prints the detected column
   (`Sponsor column detected as: ...`); if it says `None`, the header didn't
   match and the tree will be flat.
3. For each member, put **who recruited them** — either that sponsor's
   **callsign** (e.g. `KI7QCF`) or their **BKG number** (e.g. `1`, `BKG1`,
   or `BKG #1`). Both are accepted.
4. Leave it blank for anyone with no known sponsor — they become a **root** of
   the tree (the founder, #1, is naturally a root).

**Backfilling existing members:** this column *is* the backfill mechanism. Fill
in the `Sponsor` for current members as you learn who brought them in. On the
next build the tree updates automatically — nothing else to edit.

> The build resolves a sponsor to a callsign by matching the cell against
> known members (callsign first, then BKG number). Unrecognized sponsors are
> logged and treated as a root. Self-sponsors are ignored.

That's all the tree needs. `scripts/build-roster.py` injects the data into
`tree.html` between the `DOWNLINE_DATA` markers each build.

---

## 2. Recruit applications

`recruit.html` sends JSON directly to the membership intake API. An accepted
application receives an opaque receipt and remains pending officer review;
submission never issues a number or publishes a member automatically.

The enhanced form matches sponsor callsigns or BKG numbers against the public
roster, separates sponsor/recruit contact addresses, accepts 24-hour UTC time,
and asks for confirmation of a 2m CW QSO and the recruit's agreement to join.
Unmatched sponsors can explicitly request officer review. Officers review the
application in BKG automation and approve or reject it there.

These changes use the existing API contract and can be published independently.
See [the recruit form guide](RECRUIT-FORM.md) for form behavior, private-data
handling and recovery. The former FormSubmit relay is no longer used.
