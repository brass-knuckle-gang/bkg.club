# Recruit form

Deploy the coordinated BKG automation intake changes before publishing this
website change. The public submission endpoint stays the same; the form sends
`formVersion: "2"`, `membershipConfirmed: "true"`, and an optional
`sponsorReviewRequested: "true"`. Cached v1 forms remain supported upstream.
No website credentials, administrator access or public QRZ endpoint are needed.

Sponsor lookup reads this site's published `data/v2/roster.json`, falling back
to v1. Only public member identity fields are used. It matches exact callsigns
or membership numbers, shows the member's name/current call/number, and offers
O/0 corrections only as explicit suggestions. The snapshot can lag approvals;
a missing match permits an explicit officer-review request using a callsign.
The server checks the authoritative membership and refuses inconsistent sponsor
numbers or existing recruit membership. Matching a call does not authenticate
the submitter. Pending duplicates are flagged privately by automation.

The form keeps the existing Canadian province and U.S. state/entity selectors.
UTC time accepts `HHMM` or `HH:MM`, normalizes to `HH:MM`, and checks the combined
UTC date/time for a future value with five minutes of clock tolerance. The
current-UTC button sets both date and time, including across local midnight.
Band/mode is fixed at 2m CW. Recruit email stays optional and distinct from the
required sponsor email; officers can arrange a welcome through the sponsor.

The consent checkbox is required on each application. Automation records its
statement and server receipt time privately. Contact addresses and notes never
enter the public roster. Remembering sponsor callsign/email in local storage is
opt-in and can be cleared by unchecking the setting. Recruit details are never
stored in that setting. Submitting another recruit retains sponsor details for
this page and clears recruit information, QSO and consent.

## Recovery

Before sending, the original payload and UUID are stored in session storage and
in memory. An ambiguous response locks that snapshot for retry; reload restores
the review screen. A fresh challenge can be used with the same application key.
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
lookup and uncertain-attempt rules. The existing Python suite verifies that the
new scripts are included in the validated public build. Browser checks must use
local synthetic roster data and a mocked intake endpoint; never submit synthetic
applications or issue memberships in production.
