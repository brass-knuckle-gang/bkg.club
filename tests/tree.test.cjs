const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../tree.html'), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];

const MEMBERS = [
    { call: 'N0SYN1', name: 'Founder', num: 1 },
    { call: 'N0SYN2', name: 'Recruiter', num: 2, sponsor: 'N0SYN1' },
    { call: 'N0SYN3', name: 'Recruit', num: 3, sponsor: 'N0SYN2' },
    { call: 'N0SYN4', name: 'Recruit', num: 4, sponsor: 'N0SYN1' },
];

function openTree() {
    const nodes = new Map();
    const node = () => {
        const classes = new Set();
        const listeners = new Map();
        const attributes = new Map();
        let toggle;
        return {
            classes, listeners, attributes, style: {}, innerHTML: '', textContent: '',
            classList: {
                toggle(name, force) {
                    const on = force === undefined ? !classes.has(name) : force;
                    on ? classes.add(name) : classes.delete(name);
                    return on;
                },
                contains: name => classes.has(name),
            },
            addEventListener: (type, callback) => listeners.set(type, callback),
            setAttribute: (name, value) => attributes.set(name, String(value)),
            appendChild() {},
            // The node's own toggle; innerHTML is not parsed.
            querySelector(selector) {
                assert.ok(['.node-toggle', ':scope > .node-card > .node-toggle'].includes(selector), selector);
                return toggle || (toggle = Object.assign(node(), { li: this }));
            },
            querySelectorAll(selector) {
                assert.equal(selector, '.tree-node');
                return lis;
            },
        };
    };
    const lis = [];
    const document = {
        getElementById(id) {
            if (!nodes.has(id)) nodes.set(id, node());
            return nodes.get(id);
        },
        createElement(tag) {
            const created = node();
            if (tag === 'li') lis.push(created);
            return created;
        },
    };
    document.getElementById('bkg-downline-data').textContent = JSON.stringify(MEMBERS);
    vm.runInNewContext(script, { document });
    const toggleFor = call => lis.find(li => li.innerHTML.includes('>' + call + '</a>')).querySelector('.node-toggle');
    const click = id => document.getElementById(id).listeners.get('click')();
    return { lis, toggleFor, click };
}

test('subtree toggles start labelled to collapse their downline', () => {
    const { lis } = openTree();
    assert.match(lis[0].innerHTML, /aria-expanded="true" aria-label="Collapse N0SYN1’s downline"/);
    assert.match(lis.find(li => li.innerHTML.includes('>N0SYN3</a>')).innerHTML, /aria-hidden="true" tabindex="-1"/);
});

test('collapse all and expand all keep each toggle label in step with its state', () => {
    const tree = openTree();
    const founder = tree.toggleFor('N0SYN1');
    const recruiter = tree.toggleFor('N0SYN2');

    tree.click('collapseAll');
    for (const [toggle, call] of [[founder, 'N0SYN1'], [recruiter, 'N0SYN2']]) {
        assert.ok(toggle.li.classes.has('node-collapsed'));
        assert.equal(toggle.attributes.get('aria-expanded'), 'false');
        assert.equal(toggle.attributes.get('aria-label'), 'Expand ' + call + '’s downline');
    }

    founder.listeners.get('click')();
    assert.equal(founder.attributes.get('aria-expanded'), 'true');
    assert.equal(founder.attributes.get('aria-label'), 'Collapse N0SYN1’s downline');
    assert.equal(recruiter.attributes.get('aria-label'), 'Expand N0SYN2’s downline');

    tree.click('expandAll');
    assert.equal(recruiter.li.classes.has('node-collapsed'), false);
    assert.equal(recruiter.attributes.get('aria-expanded'), 'true');
    assert.equal(recruiter.attributes.get('aria-label'), 'Collapse N0SYN2’s downline');
});
