const assert = require('node:assert/strict');
const test = require('node:test');
const awards = require('../assets/awards.js');

function roster(rows) {
  return {schema_version: '2', members: rows.map(([n, call, sp]) =>
    ({bkg_number: n, callsign: call, name: call, sponsor_bkg_number: sp}))};
}

test('counts direct recruits for the sponsor', () => {
  const d = awards.ladder(roster([[1, 'K0AAA', null], [2, 'K0BBB', 1], [3, 'K0CCC', 1], [4, 'K0DDD', 2]]));
  assert.equal(d.byCall.get('K0AAA').count, 2);
  assert.equal(d.byCall.get('K0BBB').count, 1);
  assert.equal(d.byCall.get('K0DDD').count, 0);
});

test('ignores a sponsor numbered after the recruit, self-sponsors and unknown sponsors', () => {
  const d = awards.ladder(roster([[1, 'K0AAA', 3], [2, 'K0BBB', 2], [3, 'K0CCC', 99]]));
  assert.deepEqual([...d.byCall.values()].map(r => r.count), [0, 0, 0]);
});

test('tier thresholds', () => {
  assert.equal(awards.tierFor(0), null);
  assert.equal(awards.tierFor(1)[1], 'Scout');
  assert.equal(awards.tierFor(4)[1], 'Scout');
  assert.equal(awards.tierFor(5)[1], 'Trainer');
  assert.equal(awards.tierFor(49)[1], 'Promoter');
  assert.equal(awards.tierFor(50)[1], 'Gym Boss');
  assert.equal(awards.tierFor(100)[1], 'Undisputed');
  assert.equal(awards.nextTier(100), null);
});

test('holders count each member once at their highest rank', () => {
  const rows = [[1, 'K0AAA', null]];
  for (let i = 2; i <= 7; i++) rows.push([i, 'K0R' + i, 1]);
  rows.push([8, 'K0ZZZ', 2]);
  const d = awards.ladder(roster(rows));
  assert.deepEqual(d.holders, [1, 1, 0, 0, 0, 0]);
  assert.equal(d.leaders[0].call, 'K0AAA');
});

test('leaders tie-break by lower BKG number', () => {
  const d = awards.ladder(roster([[1, 'K0AAA', null], [2, 'K0BBB', null], [3, 'K0CCC', 2], [4, 'K0DDD', 1]]));
  assert.deepEqual(d.leaders.map(r => r.call), ['K0AAA', 'K0BBB']);
});

test('lookup normalizes input and reports progress', () => {
  const d = awards.ladder(roster([[1, 'K0AAA', null], [2, 'K0BBB', 1], [3, 'K0CCC', 1]]));
  const r = awards.lookup(d, ' k0aaa ');
  assert.equal(r.kind, 'found');
  assert.equal(r.tier[1], 'Scout');
  assert.equal(r.next[1], 'Trainer');
  assert.equal(r.toNext, 3);
  assert.equal(r.pct, 25);
  assert.equal(awards.lookup(d, 'N0PE').kind, 'missing');
  assert.equal(awards.lookup(d, '').kind, 'empty');
});

test('empty or bad roster does not throw', () => {
  assert.equal(awards.ladder(null).total, 0);
  assert.equal(awards.ladder({}).leaders.length, 0);
});
