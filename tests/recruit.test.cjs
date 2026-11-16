const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
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

function loadRecruit({ attempt = null, respond }) {
    const nodes = new Map();
    const makeNode = () => {
        const classes = new Set();
        const listeners = new Map();
        return {
            classes, listeners, value: '', validationMessage: '', style: {}, children: [],
            classList: { add: c => classes.add(c), contains: c => classes.has(c), remove: c => classes.delete(c) },
            addEventListener: (event, callback) => listeners.set(event, callback),
            replaceChildren() { this.children = []; },
            append(...children) { this.children.push(...children); },
            querySelectorAll() { return []; },
            reportValidity() { return [...nodes.values()].every(node => !node.validationMessage); },
            setCustomValidity(message) { this.validationMessage = message; },
            scrollIntoView() {},
            focus() {},
        };
    };
    const document = {
        getElementById(id) {
            if (!nodes.has(id)) nodes.set(id, makeNode());
            return nodes.get(id);
        },
        createElement: makeNode,
        querySelector: makeNode,
    };
    const window = {
        BkgRecruit: rules,
        sessionStorage: { getItem: key => key === 'bkg-recruit-pending-v1' ? JSON.stringify(attempt) : null },
        localStorage: { getItem: () => null },
        turnstile: makeNode(), // Simulate a named element before the real API arrives.
    };
    const requests = [];
    const controller = fs.readFileSync(path.join(__dirname, '../assets/recruit.js'), 'utf8');
    vm.runInNewContext(controller, {
        document, window,
        fetch: async url => {
            requests.push(url);
            return respond(url);
        },
    });
    return { nodes, window, requests };
}

test('recovering an attempt before Turnstile loads still starts the roster fetch', () => {
    const attempt = rules.attempt({ callsign: 'F4TEST' }, null, '8d746124-0a7d-4a34-b229-5a85a65aa843');
    const { nodes, window, requests } = loadRecruit({
        attempt,
        respond: () => ({ ok: true, json: async () => ({ schema_version: '2', members: [] }) }),
    });
    assert.deepEqual(requests, ['data/v2/roster.json']);
    assert.ok(nodes.get('formConfirm').classes.has('show'));
    assert.equal(nodes.get('confirmBack').disabled, true);

    const targets = [];
    window.turnstile = { render: target => targets.push(target) };
    nodes.get('turnstileScript').listeners.get('load')();
    assert.deepEqual(targets, ['#turnstileWidget']);
    assert.match(fs.readFileSync(path.join(__dirname, '../recruit.html'), 'utf8'), /id="turnstileWidget"/);
});

test('an unavailable V2 roster leaves sponsor officer review available without fetching V1', async () => {
    const v1 = { schema_version: '1', members: [{ bkg_number: 42, callsign: 'F4TEST', name: 'Example Sponsor' }] };
    for (const response of [{ ok: false, status: 404 }, { ok: true, json: async () => v1 }]) {
        const { nodes, requests } = loadRecruit({ respond: () => response });
        nodes.get('sponsorCall').value = 'F4TEST';
        await new Promise(setImmediate);
        assert.deepEqual(requests, ['data/v2/roster.json']);
        assert.match(nodes.get('sponsorMatch').textContent, /Public roster unavailable/);
        assert.equal(nodes.get('sponsorReviewRow').hidden, false);

        nodes.get('recruitCall').value = 'F4NEW';
        nodes.get('qsoTime').value = '21:30';
        const submit = nodes.get('recruitForm').listeners.get('submit');
        submit({ preventDefault() {} });
        assert.match(nodes.get('sponsorReviewRequested').validationMessage, /request officer review/);
        assert.equal(nodes.get('formConfirm').classes.has('show'), false);

        nodes.get('sponsorReviewRequested').checked = true;
        submit({ preventDefault() {} });
        assert.equal(nodes.get('formConfirm').classes.has('show'), true);
        assert.ok(nodes.get('confirmSummary').children.some(node => node.textContent === 'Officer review requested'));
    }
});

// Field order on the page, for the first-invalid-field lookup.
const DOM_ORDER = ['sponsorCall', 'email', 'recruitCall', 'recruitName', 'recruitUsRegion', 'recruitProvince',
    'recruitQth', 'recruitEmail', 'qsoDate', 'qsoTime', 'qsoBand', 'notes'];

function recruitPage({ stored = null, intake } = {}) {
    const nodes = new Map();
    const document = { activeElement: null };
    const makeNode = id => {
        const classes = new Set();
        const listeners = new Map();
        const attributes = new Map();
        return {
            id, classes, listeners, attributes, value: '', style: {}, dataset: {}, options: [],
            classList: { add: c => classes.add(c), remove: c => classes.delete(c), contains: c => classes.has(c) },
            addEventListener: (event, callback) => listeners.set(event, callback),
            setAttribute: (name, value) => attributes.set(name, String(value)),
            removeAttribute: name => attributes.delete(name),
            setCustomValidity() {}, replaceChildren() {}, append() {}, appendChild() {}, scrollIntoView() {}, reset() {},
            focus() { document.activeElement = this; },
        };
    };
    document.getElementById = id => {
        if (!nodes.has(id)) nodes.set(id, makeNode(id));
        return nodes.get(id);
    };
    document.createElement = () => makeNode();
    document.querySelector = () => makeNode();
    const form = document.getElementById('recruitForm');
    form.reportValidity = () => true;
    form.querySelectorAll = selector => selector === '[aria-invalid]'
        ? [...nodes.values()].filter(n => n.attributes.has('aria-invalid')) : [];
    form.querySelector = selector => {
        assert.equal(selector, '[aria-invalid="true"]');
        return DOM_ORDER.map(id => nodes.get(id)).find(n => n && n.attributes.get('aria-invalid') === 'true') || null;
    };
    const confirmPanel = document.getElementById('formConfirm');
    confirmPanel.contains = target => ['confirmHeading', 'confirmBack', 'confirmSend'].includes(target && target.id);
    const session = new Map(stored ? [['bkg-recruit-pending-v1', JSON.stringify(stored)]] : []);
    const storage = map => ({ getItem: k => map.get(k) || null, setItem: (k, v) => map.set(k, v), removeItem: k => map.delete(k) });
    const window = {
        BkgRecruit: rules,
        sessionStorage: storage(session),
        localStorage: storage(new Map()),
        turnstile: { render: () => 'widget', getResponse: () => 'token', reset() {} },
    };
    const controller = fs.readFileSync(path.join(__dirname, '../assets/recruit.js'), 'utf8');
    vm.runInNewContext(controller, {
        document, window, AbortController, setTimeout, clearTimeout,
        crypto: { randomUUID: () => '8d746124-0a7d-4a34-b229-5a85a65aa843' },
        fetch: async url => url.startsWith('data/')
            ? { ok: true, json: async () => ({ schema_version: '2', members: [] }) }
            : intake(),
    });
    const el = id => document.getElementById(id);
    return {
        document, el,
        review() {
            el('sponsorCall').value = 'VE6TEST';
            el('recruitCall').value = 'N0NEW';
            el('sponsorReviewRequested').checked = true;
            el('recruitForm').listeners.get('submit')({ preventDefault() {} });
        },
        send: () => el('confirmSend').listeners.get('click')(),
    };
}

const reply = (status, data) => async () => ({ status, json: async () => data, headers: { get: () => null } });

test('focus moves to the review panel and back to the form with the user', () => {
    const page = recruitPage();
    page.review();
    assert.ok(page.el('formConfirm').classes.has('show'));
    assert.equal(page.document.activeElement, page.el('confirmHeading'));
    page.el('confirmBack').listeners.get('click')();
    assert.equal(page.el('formConfirm').classes.has('show'), false);
    assert.equal(page.document.activeElement, page.el('sponsorCall'));
});

test('a receipt focuses the success panel heading', async () => {
    const page = recruitPage({ intake: reply(202, { receipt: 'BKG-R-1', receivedAt: '2026-11-16T00:00:00Z' }) });
    page.review();
    await page.send();
    assert.ok(page.el('formSuccess').classes.has('show'));
    assert.equal(page.document.activeElement, page.el('successHeading'));
});

test('a rejected intake focuses the first highlighted field, or the review button', async () => {
    let page = recruitPage({ intake: reply(400, { error: { code: 'invalid_request', fields: ['qsoTime', 'callsign'] } }) });
    page.review();
    await page.send();
    assert.equal(page.el('recruitCall').attributes.get('aria-invalid'), 'true');
    assert.equal(page.el('qsoTime').attributes.get('aria-invalid'), 'true');
    assert.equal(page.document.activeElement, page.el('recruitCall'));

    page = recruitPage({ intake: reply(422, { error: { code: 'turnstile_failed' } }) });
    page.review();
    await page.send();
    assert.equal(page.document.activeElement, page.el('submitBtn'));
});

test('an uncertain send keeps focus on the send button for a safe retry', async () => {
    const page = recruitPage({ intake: reply(503, {}) });
    page.review();
    page.document.activeElement = null; // The disabled button dropped focus.
    await page.send();
    assert.ok(page.el('formConfirm').classes.has('show'));
    assert.equal(page.document.activeElement, page.el('confirmSend'));
});

test('restoring a saved attempt on page load does not move focus', () => {
    const attempt = rules.attempt({ callsign: 'F4TEST' }, null, '8d746124-0a7d-4a34-b229-5a85a65aa843');
    const page = recruitPage({ stored: attempt });
    assert.ok(page.el('formConfirm').classes.has('show'));
    assert.equal(page.document.activeElement, null);
});
