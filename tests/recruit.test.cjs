const { test } = require('node:test');
const assert = require('node:assert/strict');
const rules = require('../assets/recruit-core.js');

test('UTC entry accepts ham notation, midnight and 24-hour times without AM/PM', () => {
    for (const [input, output] of [['0000', '00:00'], ['2359', '23:59'], ['21:30', '21:30'], [' 0730 ', '07:30']]) {
        assert.equal(rules.time(input), output);
    }
    for (const input of ['2400', '12:60', '9:30', '9:30 PM', '123', '', 'ab:cd']) assert.equal(rules.time(input), null);
    assert.deepEqual(rules.utc(new Date('2026-10-30T23:59:00-04:00')), { qsoDate: '2026-10-31', qsoTime: '03:59' });
});

test('sponsor lookup resolves exact international calls or stable membership numbers', () => {
    const members = [{ callsign: 'VE6TEST', bkg_number: 42, name: 'Example' }];
    for (const input of ['ve6test', ' BKG42 ', '#42', '0042']) assert.equal(rules.findSponsor(input, members), members[0]);
    assert.equal(rules.findSponsor('VE6TESO', members), undefined);
    assert.equal(rules.findSponsor('VE6', members), undefined);
    assert.equal(rules.normalizeCall('N0ß'), 'N0ß');
    assert.ok(rules.callIssue('N0ß'));
    assert.ok(rules.callIssue('N0ABC/P'));
});

test('an uncertain attempt cannot be replaced by edited details or a new UUID', () => {
    const payload = { callsign: 'N0NEW', notes: 'Original' };
    const key = '8d746124-0a7d-4a34-b229-5a85a65aa843';
    const attempt = rules.attempt(payload, null, key);
    const restored = rules.pending(JSON.parse(JSON.stringify(attempt)));
    assert.deepEqual(rules.attempt(payload, restored, 'new-key'), attempt);
    assert.throws(() => rules.attempt({ ...payload, notes: 'Edited' }, restored, 'new-key'), /original submission/);
    for (const status of [400, 409, 422, 429, 503]) {
        assert.equal(rules.canEditAfter(status, true), false);
    }
    assert.equal(rules.canEditAfter(400, false), true);
    assert.equal(rules.canEditAfter(409, false), false);
    assert.equal(rules.canEditAfter(503, false), false);
});

test('UI acknowledgements never become intake fields or alter the submission fingerprint', () => {
    const values = {
        sponsorCallsign: 'VE6TEST', sponsorNumber: 'BKG42', sponsorName: 'Example Sponsor',
        submitterEmail: 'sponsor@example.test', callsign: 'N0NEW', name: 'Example Recruit',
        qth: 'Utah', recruitEmail: '', qsoDate: '2026-10-29', qsoTime: '21:30',
        bandMode: '2m CW', notes: '',
        formVersion: '2', membershipConfirmed: true, sponsorReviewRequested: true,
    };
    const payload = rules.payload(values);
    assert.deepEqual(Object.keys(payload).sort(), [
        'bandMode', 'callsign', 'name', 'notes', 'qsoDate', 'qsoTime', 'qth',
        'recruitEmail', 'sponsorCallsign', 'sponsorName', 'sponsorNumber', 'submitterEmail',
    ]);
    assert.equal(payload.recruitEmail, '');
    assert.deepEqual(rules.payload({ ...values, membershipConfirmed: false, sponsorReviewRequested: false }), payload);
    const attempt = { ...rules.attempt(payload, null, 'original-key'), ui: {
        membershipConfirmed: true, sponsorReviewRequested: true,
    } };
    assert.equal(JSON.parse(attempt.payload).membershipConfirmed, undefined);
    assert.equal(rules.attempt(payload, attempt, 'replacement-key').key, 'original-key');
});

test('malformed stored attempts cannot become submissions', () => {
    for (const value of [null, {}, { key: 'bad', payload: '{}' }, { key: '8d746124-0a7d-4a34-b229-5a85a65aa843', payload: 'null' }]) {
        assert.equal(rules.pending(value), null);
    }
});


test('a definitively rejected attempt reuses its key until meaningful details change', () => {
    const payload = { callsign: 'N0NEW', notes: 'Original' };
    const rejected = { ...rules.attempt(payload, null, 'original-key'), state: 'rejected' };
    const retry = rules.attempt(payload, rejected, 'replacement-key');
    assert.equal(retry.key, 'original-key');
    assert.equal(retry.state, undefined); // Persist as uncertain before attempting the retry.
    assert.equal(rules.attempt({ ...payload, notes: 'Corrected' }, rejected, 'replacement-key').key, 'replacement-key');
});
