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
function openSheet({ photo = '', fullPhoto = '', savedView = null, wide = false } = {}) {
    const document = { activeElement: null, body: node() };
    const focusable = name => ({ name, focus() { document.activeElement = this; } });
    const controls = ['close', 'recruit', 'qrz', 'copy'].map(focusable);
    const attributes = new Map([['src', photo], ['data-full-src', fullPhoto]]);
    const imageUpdates = [];
    const image = photo ? node({
        getAttribute: name => attributes.get(name) || null,
        setAttribute: (name, value) => {
            attributes.set(name, value);
            imageUpdates.push({ name, value });
        },
    }) : null;
    const viewButtons = ['list', 'wall'].map(view => node({ dataset: { view } }));
    const row = node({
        id: 'N0SYN1',
        dataset: { n: '1' },
        querySelector: selector => ({
            '.member-name': { textContent: 'Fixture Op' },
            '.rl-badges': { innerHTML: '' },
            '.rl-ini': { textContent: 'FO' },
            '.rl-shot img': image,
        })[selector] || null,
    });
    const root = node({ querySelectorAll: selector => ({
        '.rl-row[id]': [row],
        '.rl-views button': viewButtons,
    })[selector] || [] });
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
    const matchMedia = () => ({ matches: wide });
    vm.runInNewContext(controller, {
        document,
        window: node({ matchMedia }),
        matchMedia,
        location: { hash: '#N0SYN1', pathname: '/', search: '', origin: 'https://www.bkg.club' },
        history: { replaceState() {} },
        localStorage: { getItem: () => savedView, setItem() {} },
        setTimeout, clearTimeout,
    });
    const press = (key, shiftKey = false) => {
        let prevented = false;
        document.keydown({ key, shiftKey, preventDefault: () => (prevented = true) });
        return prevented;
    };
    const selectView = view => viewButtons.find(button => button.dataset.view === view).listeners.get('click')();
    return { document, sheet, controls, press, image, imageUpdates, root, selectView };
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

test('list rows use thumbnails while wall and rap-sheet images use the full photo', () => {
    const photo = 'images/mugshots/thumbnail.webp';
    const fullPhoto = 'images/mugshots/full.webp';
    const { image, imageUpdates, root, sheet, selectView } = openSheet({ photo, fullPhoto });
    assert.equal(root.dataset.view, 'list');
    assert.equal(image.getAttribute('src'), photo);
    assert.ok(sheet.innerHTML.includes(`src="${fullPhoto}"`));
    assert.ok(!sheet.innerHTML.includes(photo));
    assert.equal(imageUpdates.length, 0);
    selectView('list');
    assert.equal(imageUpdates.length, 0);

    selectView('wall');
    assert.equal(root.dataset.view, 'wall');
    assert.equal(image.getAttribute('src'), fullPhoto);
    assert.equal(imageUpdates.length, 1);
    selectView('wall');
    assert.equal(imageUpdates.length, 1);
    selectView('list');
    assert.equal(image.getAttribute('src'), photo);
    assert.equal(imageUpdates.length, 2);
});

test('initial photo choice follows the desktop default and the saved view', () => {
    const photo = 'images/mugshots/thumbnail.webp';
    const fullPhoto = 'images/mugshots/full.webp';
    for (const options of [
        { wide: true, expectedView: 'wall' },
        { savedView: 'wall', expectedView: 'wall' },
        { wide: true, savedView: 'list', expectedView: 'list' },
    ]) {
        const { image, root } = openSheet({ photo, fullPhoto, ...options });
        assert.equal(root.dataset.view, options.expectedView);
        assert.equal(image.getAttribute('src'), options.expectedView === 'wall' ? fullPhoto : photo);
    }
});

test('older rows without a full-photo attribute keep their photo in every view', () => {
    const photo = 'images/mugshots/original.webp';
    const { image, sheet, selectView } = openSheet({ photo });
    assert.equal(image.getAttribute('src'), photo);
    assert.ok(sheet.innerHTML.includes(`src="${photo}"`));
    selectView('wall');
    assert.equal(image.getAttribute('src'), photo);
    selectView('list');
    assert.equal(image.getAttribute('src'), photo);
});
