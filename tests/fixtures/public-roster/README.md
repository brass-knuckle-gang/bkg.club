# Export contract provenance

`schema-v1.json` and `fixtures/*-v1.json` are copied without changes from
[bkg-automation PR #5](https://github.com/brass-knuckle-gang/bkg-automation/pull/5),
at commit `d6b60f96c1f403a327e621fb9ed4d04146077100`, under
[`docs/public-roster/`](https://github.com/brass-knuckle-gang/bkg-automation/tree/d6b60f96c1f403a327e621fb9ed4d04146077100/docs/public-roster).

These are synthetic examples, never a fallback membership source. The valid
empty export is deliberately rejected by the website publication adapter.

Rechecked byte-for-byte against merged backend PR #10 at revision
`060a8334f13a5995065bf942fd83f94340268b76`; schema and both fixtures are unchanged.

`schema-v2.json` and `fixtures/*-v2.json` are copied byte-for-byte from the
coordinated administration publication implementation at
`docs/public-roster/schema-v2.json` and `docs/public-roster/fixtures/*-v2.json`.
The v2 fixture includes exact JavaScript grid-center floats, nullable accepted
locations/photos, and the recursively sorted content hash
`sha256:8857d8d9cfd7b57c22918780e189fea1139257817456607fbff6de089490e6a7`.
The website verifies that unchanged upstream hash and publishes the coordinates
verbatim. Photo integrity uses independent synthetic WebP fixtures; upstream
fixtures contain no member photos or production identities.
