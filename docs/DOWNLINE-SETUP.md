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

## 2. The recruit form: email relay (no backend)

The site is static (GitHub Pages), so there's no server to receive
submissions. `recruit.html` POSTs to **[FormSubmit](https://formsubmit.co)** —
a free, no-signup relay that emails each submission to the gang inbox. Officers
review it, assign a BKG #, and add the recruit to the roster (admin-review
flow — submissions do **not** auto-publish to the tree).

The form is already wired and live. There are no API keys and nothing to
deploy; the only setup is a one-time activation click.

### a. One-time activation (required once)

The **first** submission after this goes live triggers a confirmation email
from FormSubmit to the gang inbox (**k7bkgang@gmail.com**). Open that email and
click the activation link **once**. After that, every submission is delivered
automatically. Until it's activated, submissions won't arrive — so send a test
submission through the live form and click the link when it lands.

> The activation email may take a minute and can land in Spam/Promotions — check
> there if it doesn't show up.

### b. Where submissions go / changing the inbox

The target address lives near the bottom of `recruit.html`:

```js
const RELAY_INBOX = "k7bkgang@gmail.com";
```

Change that string and re-activate (step **a**) to point submissions at a
different inbox. Each email arrives as a readable table with the sponsor, the
new op, the QSO details, and notes; if the submitter gave an email it's set as
the reply-to, so officers can reply to them directly.

> **Spam note:** the address appears in the page source. FormSubmit also
> supports a hashed alias endpoint (`formsubmit.co/<your-random-string>`) that
> hides the raw address — grab your string from the confirmation email and swap
> it into `RELAY_ENDPOINT` if harvesting becomes a problem.

### c. Approving a submission

1. Read the emailed submission.
2. To accept: add the new op to the **roster sheet** as usual (assign the next
   BKG #) and set their **`Sponsor`** to the submitting member's callsign/number.
3. Next build → they appear in the roster, map, and the downline tree.

---

## Files

| File                          | What it is |
|-------------------------------|------------|
| `tree.html`                   | Downline tree page (data injected at build) |
| `recruit.html`                | Recruit submission form (emails via FormSubmit relay) |
| `assets/bkg.css`              | Shared theme for the two new pages |
| `scripts/build-roster.py`     | Now also resolves sponsors + builds the tree |

No new secrets or services beyond the free FormSubmit relay. The form needs no
API keys — it POSTs straight to FormSubmit, which emails the gang inbox.
