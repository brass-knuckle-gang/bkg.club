# Export contract provenance

The V2 schema and fixtures are copied byte-for-byte from
[bkg-automation PR #13](https://github.com/brass-knuckle-gang/bkg-automation/pull/13).
They were rechecked against
[backend main on October 21, 2026](https://github.com/brass-knuckle-gang/bkg-automation/tree/5ba852456ca4cabb49e44bdec7bd49f7ccc6d575/docs/public-roster).

The synthetic roster includes exact JavaScript grid-center coordinates, nullable
locations and photos, and a recursively sorted content hash. The website verifies
the unchanged hash and publishes coordinates verbatim. Independent synthetic WebP
fixtures exercise photo integrity. The valid empty export is deliberately rejected
by the publication adapter.
