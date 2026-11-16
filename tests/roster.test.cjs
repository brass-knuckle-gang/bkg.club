const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const controller = fs.readFileSync(path.join(__dirname, '../assets/roster.js'), 'utf8');

function node(extra = {}) {
    const listeners = new Map();
    return {
        dataset: {}, style: { setProperty() {} }, listeners,
        addEventListener: (type, callback) => listeners.set(type, callback),
        setAttribute() {}, appendChild() {}, querySelector: () => node(), querySelectorAll: () => [],
        ...extra,
    };
}

// Opens the rap sheet for one member and returns the controls the sheet would
// render. innerHTML is not parsed, so the sheet reports a fixed set of controls.
function openSheet() {
    const document = { activeElement: null, body: node() };
    const focusable = name => ({ name, focus() { document.activeElement = this; } });
    const controls = ['close', 'recruit', 'qrz', 'copy'].map(focusable);
    const row = node({
        id: 'N0SYN1',
        dataset: { n: '1' },
        querySelector: selector => ({
            '.member-name': { textContent: 'Fixture Op' },
            '.rl-badges': { innerHTML: '' },
            '.rl-ini': { textContent: 'FO' },
        })[selector] || null,
    });
    const root = node({ querySelectorAll: selector => selector === '.rl-row[id]' ? [row] : [] });
    let sheet;
    document.querySelector = selector => selector === '.rl' ? root : null;
    document.addEventListener = (type, callback) => (document[type] = callback);
    document.createElement = () => {
        const created = node();
        if (sheet === undefined) sheet = null; // The scrim is created first.
        else sheet = Object.assign(created, {
            querySelector: selector => selector === '.rl-close' ? controls[0] : null,
            querySelectorAll: () => controls,
            contains: target => target === sheet || controls.includes(target),
        });
        return created;
    };
    vm.runInNewContext(controller, {
        document,
        window: node(),
        location: { hash: '#N0SYN1', pathname: '/', search: '', origin: 'https://www.bkg.club' },
        history: { replaceState() {} },
        localStorage: { getItem: () => null, setItem() {} },
        setTimeout, clearTimeout,
    });
    const press = (key, shiftKey = false) => {
        let prevented = false;
        document.keydown({ key, shiftKey, preventDefault: () => (prevented = true) });
        return prevented;
    };
    return { document, sheet, controls, press };
}

test('the rap sheet keeps Tab and Shift+Tab inside the dialog', () => {
    const { document, sheet, controls, press } = openSheet();
    const [close, recruit, , copy] = controls;
    assert.equal(sheet.hidden, false);
    assert.equal(document.activeElement, close);

    document.activeElement = recruit;
    assert.equal(press('Tab'), false, 'Tab between inner controls is left to the browser');
    assert.equal(document.activeElement, recruit);

    copy.focus();
    assert.equal(press('Tab'), true);
    assert.equal(document.activeElement, close);
    assert.equal(press('Tab', true), true);
    assert.equal(document.activeElement, copy);

    document.activeElement = document.body;
    assert.equal(press('Tab'), true, 'focus outside the sheet comes back in');
    assert.equal(document.activeElement, close);
});

test('Escape still closes the rap sheet, after which Tab is not intercepted', () => {
    const { document, sheet, controls, press } = openSheet();
    press('Escape');
    assert.equal(sheet.hidden, true);
    controls[3].focus();
    assert.equal(press('Tab'), false);
    assert.equal(document.activeElement, controls[3]);
});
