const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../index.html'), 'utf8');
const script = html.match(/\/\/ ===== Gang Territory Map =====([\s\S]*?)<\/script>/)[1];

const op = (call, num) => ({ call, name: 'Fixture ' + call, num });
const MAP_DATA = {
    states: { WA: [op('N0SYN1', 1), op('N0SYN2', 2)], UT: [op('N0SYN3', 3)] },
    dx: { Poland: { flag: '🇵🇱', members: [op('SP0SYN', 4), op('SP0SYO', 5)] }, Canada: { members: [op('VE0SYN', 6)] } },
};
const NAMES = { WA: 'Washington', UT: 'Utah', CA: 'California', DX: 'Worldwide DX' };

function openMap() {
    const nodes = new Map();
    const document = { activeElement: null };
    const node = id => {
        const classes = new Set();
        const listeners = new Map();
        const attributes = new Map();
        return {
            id, classes, listeners, attributes, style: {}, dataset: {}, innerHTML: '', textContent: '',
            classList: { add: (...c) => c.forEach(x => classes.add(x)), remove: c => classes.delete(c) },
            addEventListener: (type, callback) => listeners.set(type, callback),
            setAttribute: (name, value) => attributes.set(name, String(value)),
            querySelector: () => null,
            scrollIntoView() {},
            focus() { document.activeElement = this; },
        };
    };
    document.getElementById = id => {
        if (!nodes.has(id)) {
            const created = node(id);
            const abbr = id.startsWith('state-') && id.slice(6);
            if (abbr) Object.assign(created.dataset, { abbr, name: NAMES[abbr] });
            nodes.set(id, created);
        }
        return nodes.get(id);
    };
    for (const abbr of Object.keys(NAMES)) document.getElementById('state-' + abbr);
    document.getElementById('bkg-map-data').textContent = '<!-- MAP_DATA:START -->' + JSON.stringify(MAP_DATA) + '<!-- MAP_DATA:END -->';
    let rows = [];
    document.querySelectorAll = selector => {
        assert.equal(selector, '#topStatesList .leader-row');
        rows = [...document.getElementById('topStatesList').innerHTML.matchAll(/data-abbr="(\w+)"/g)].map(match => {
            const row = node();
            row.dataset.abbr = match[1];
            return row;
        });
        return rows;
    };
    vm.runInNewContext(script, { document });
    const state = abbr => nodes.get('state-' + abbr);
    const press = (abbr, key) => {
        let prevented = false;
        state(abbr).listeners.get('keydown')({ key, preventDefault: () => (prevented = true) });
        return prevented;
    };
    return { state, press, rows, info: () => nodes.get('mapInfo').innerHTML, list: () => nodes.get('topStatesList').innerHTML, document };
}

test('the map exposes its regions instead of hiding them in one image', () => {
    assert.match(html, /<svg class="us-map-svg"[^>]*role="group"[^>]*aria-label="Map of US states/);
});

test('occupied regions are named, focusable buttons; empty ones stay out of the tab order', () => {
    const map = openMap();
    assert.equal(map.state('WA').attributes.get('tabindex'), '0');
    assert.equal(map.state('WA').attributes.get('role'), 'button');
    assert.equal(map.state('WA').attributes.get('aria-label'), 'Washington: 2 ops');
    assert.equal(map.state('UT').attributes.get('aria-label'), 'Utah: 1 op');
    assert.equal(map.state('DX').attributes.get('aria-label'), 'Worldwide DX: 3 ops in 2 countries');
    assert.equal(map.state('CA').attributes.size, 0);
    assert.equal(map.state('CA').listeners.size, 0);
});

test('Enter and Space select a region like a click; other keys are left alone', () => {
    const map = openMap();
    assert.equal(map.press('WA', 'Tab'), false);
    assert.equal(map.state('WA').classes.has('is-active'), false);

    assert.equal(map.press('WA', 'Enter'), true);
    assert.ok(map.state('WA').classes.has('is-active'));
    assert.match(map.info(), /Washington[\s\S]*N0SYN2/);

    assert.equal(map.press('DX', ' '), true);
    assert.ok(map.state('DX').classes.has('is-active'));
    assert.equal(map.state('WA').classes.has('is-active'), false);
    assert.match(map.info(), /WORLDWIDE DX[\s\S]*Poland[\s\S]*SP0SYO/);
});

test('focus previews a region like hover, and leaving it restores the selection', () => {
    const map = openMap();
    map.press('WA', 'Enter');
    map.state('UT').listeners.get('focus')();
    assert.match(map.info(), /Utah[\s\S]*N0SYN3/);
    map.state('UT').listeners.get('blur')();
    assert.match(map.info(), /Washington/);
});

test('Top States rows are buttons that select the state and move focus to it', () => {
    const map = openMap();
    assert.match(map.list(), /^<button type="button" class="leader-row" data-abbr="WA">/);
    assert.equal((map.list().match(/<button /g) || []).length, 2);
    assert.match(map.list(), /<\/span> <span class="leader-abbr">WA<\/span> <span class="leader-name">Washington<\/span> /);

    const utah = map.rows.find(row => row.dataset.abbr === 'UT');
    utah.listeners.get('click')();
    assert.ok(map.state('UT').classes.has('is-active'));
    assert.equal(map.document.activeElement, map.state('UT'));
});
