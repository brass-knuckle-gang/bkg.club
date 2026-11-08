# Downline Tree + Recruit Form

Two pages on bkg.club deal with who recruited whom:

- **`tree.html`**: an interactive "downline" family tree showing who recruited
  who into the gang. It is an indented, collapsible tree (mobile + desktop
  friendly) where each member shows their **direct** and **indirect** induction
  counts, plus a top-recruiters leaderboard.
- **`recruit.html`**: a form where a sponsoring member submits a new recruit's
  info. Applications go straight to the membership intake API for officers to
  review in BKG automation.

The tree is part of the normal build: `scripts/build-roster.py` injects it into
`tree.html` from the published roster snapshot on every push to `main`, and on
the scheduled refresh when something changed (see [deployment](DEPLOYMENT.md)).
The recruit form needs no build step beyond publishing its scripts.

---

## 1. The tree data: sponsors come from the administration roster

Each member in the exported roster has a `sponsor_bkg_number`: the stable BKG
number of the member who recruited them. The build resolves it against the same
snapshot to the sponsor's current callsign, and injects the result between the
`DOWNLINE_DATA` markers in `tree.html`. Nothing in this repository needs editing.

- A null sponsor makes that member a **root** of the tree. The founder, #1, is
  naturally a root. The 317 historical links that were null at the October 2026
  move to the exported roster ([live review](PUBLIC-ROSTER-REVIEW.md)) have
  since been filled in upstream: on November 8, 2026 only #1 and #935 of 1,147
  members had no sponsor.
- A member excluded from publication for an invalid callsign drops out, and
  their recruits appear as roots (see
  [membership rules](JSON-INPUT.md#reviewed-membership-rules)).
- A reference to a missing member, a self-sponsor, or a sponsor cycle fails
  validation, and the live site stays as it is.

**Fixing or backfilling a sponsor:** correct it in administration
([bkg-automation](https://github.com/brass-knuckle-gang/bkg-automation)). The
next website build picks it up. The website never infers sponsors from
callsigns, names, old pages, or the legacy sheet.

### Legacy Google Sheets path

Production never uses this. It applies only to an explicitly requested
`scripts/build-roster.py --source sheets` build, which reads the old roster
Google Sheet. That build takes sponsors from a column whose header matches,
case-insensitively, `Sponsor`, `Sponsored By`, `Recruited By`, `Recruiter`,
`Referred By`, `Upline`, `Elmer`, or anything containing "sponsor"/"recruit".
The sheet uses **`Recruited By`**. Each cell holds the sponsor's callsign
(e.g. `KI7QCF`) or BKG number (`1`, `BKG1`, `BKG #1`). The build matches
callsigns first, then numbers. It logs unrecognized sponsors and treats those
members as roots, and it ignores self-sponsors. The build log prints
`Sponsor column detected as: ...`; `None` means no header matched and the tree
will be flat.

---

## 2. Recruit applications

`recruit.html` sends JSON directly to the membership intake API. An accepted
application receives an opaque receipt and remains pending officer review;
submission never issues a number or publishes a member automatically.

The form matches sponsor callsigns or BKG numbers against the public roster,
separates sponsor/recruit contact addresses, accepts 24-hour UTC time, and asks
for confirmation of a 2m CW QSO and the recruit's agreement to join. Unmatched
sponsors can explicitly request officer review. Officers review the application
in BKG automation and approve or reject it there.

See [the recruit form guide](RECRUIT-FORM.md) for form behavior, private-data
handling and recovery. The former FormSubmit relay is no longer used.
