# Recruit form

These are website-only improvements using the existing application intake
contract. The public endpoint and submitted fields stay the same. Sponsor lookup,
membership confirmation, unmatched-sponsor acknowledgement, and future-QSO checks
run in the browser. The website can be published with the current automation API;
it requires no backend changes or deployment coordination.

Sponsor lookup reads this site's published `data/v2/roster.json`, falling back
to v1. Only public member identity fields are used. It matches exact callsigns
or membership numbers, shows the member's name/current call/number, and offers
O/0 corrections only as explicit suggestions. The snapshot can lag approvals;
a missing match permits an explicit officer-review request using a callsign.
Matching a call does not authenticate the submitter. Officers continue reviewing
applications through the existing automation flow, including its duplicate
membership guards at approval. The form does not inspect the pending queue.

The form keeps the existing Canadian province and U.S. state/entity selectors.
UTC time accepts `HHMM` or `HH:MM`, normalizes to `HH:MM`, and checks the combined
UTC date/time for a future value with five minutes of clock tolerance. The
current-UTC button sets both date and time, including across local midnight.
Band/mode is fixed at 2m CW. Recruit email stays optional and distinct from the
required sponsor email; officers can arrange a welcome through the sponsor.

The consent checkbox is required on each application. It and the acknowledgement
for an unmatched sponsor stay in browser recovery state, separately from the API
payload. Contact addresses and notes never enter the
public roster. Remembering sponsor callsign/email in local storage is
opt-in and can be cleared by unchecking the setting. Recruit details are never
stored in that setting. Submitting another recruit retains sponsor details for
this page and clears recruit information, QSO and consent.

## Recovery

Before sending, the original payload, UUID and separate UI state are stored in
session storage and in memory. Only the existing API fields and challenge token
are transmitted. An ambiguous response locks that snapshot for retry; reload
restores the review screen. Existing v1 attempts without UI state remain
replayable. A fresh challenge can be used with the same application key.
Editing is available again after a definitive initial rejection. A rejection of
a retry cannot establish the original outcome, so it keeps the original attempt.
Conflicts never clear the key or invite a replacement application. Officers can
reconcile an irrecoverable conflict using the private operational record.
Browser storage can be disabled; in that case recovery survives only while the
page remains open. Accepted receipt data replaces the pending payload in session
storage and remains copyable after reload. No live public status lookup exists.

## Validation

Run `node --test tests/recruit.test.cjs` and
`python3 -m unittest discover -s tests -v` with Python 3.10+ and
`requirements-build.txt` installed. The Node suite covers UTC rollover, sponsor
lookup, payload compatibility and uncertain-attempt rules. The existing Python
suite verifies that the new scripts are included in the validated public build.
Browser checks must use local synthetic roster data and a mocked intake endpoint; never submit synthetic
applications or issue memberships in production.
