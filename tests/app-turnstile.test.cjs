const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ROOT = path.join(__dirname, '..');
const controller = fs.readFileSync(path.join(ROOT, 'assets/app-turnstile.js'), 'utf8');

function loadAppPage({ app = true } = {}) {
    const messages = [];
    const appended = [];
    const makeNode = () => {
        const listeners = new Map();
        return { listeners, textContent: '', addEventListener: (event, callback) => listeners.set(event, callback) };
    };
    const nodes = new Map();
    const document = {
        getElementById(id) {
            if (!nodes.has(id)) nodes.set(id, makeNode());
            return nodes.get(id);
        },
        createElement: makeNode,
        head: { appendChild: node => appended.push(node) },
    };
    const window = {};
    if (app) window.webkit = { messageHandlers: { bkgTurnstile: { postMessage: message => messages.push(message) } } };
    vm.runInNewContext(controller, { document, window });
    return { nodes, window, messages, appended };
}

function fakeTurnstile() {
    const api = { renders: [], resets: [] };
    api.render = (target, options) => { api.renders.push({ target, options }); return 'widget-1'; };
    api.reset = id => api.resets.push(id);
    return api;
}

test('the app page uses the recruit form sitekey and action', () => {
    const recruit = fs.readFileSync(path.join(ROOT, 'assets/recruit.js'), 'utf8');
    const sitekey = recruit.match(/sitekey: '([^']+)', action: 'recruit-application'/)[1];
    const { window, appended } = loadAppPage();
    window.turnstile = fakeTurnstile();
    appended[0].listeners.get('load')();
    const { target, options } = window.turnstile.renders[0];
    assert.equal(target, '#appTurnstileWidget');
    assert.equal(options.sitekey, sitekey);
    assert.equal(options.action, 'recruit-application');
    assert.match(appended[0].src, /^https:\/\/challenges\.cloudflare\.com\/turnstile\/v0\/api\.js\?render=explicit$/);
});

test('widget events are posted to the app message handler', () => {
    const { window, appended, messages } = loadAppPage();
    window.turnstile = fakeTurnstile();
    appended[0].listeners.get('load')();
    const options = window.turnstile.renders[0].options;
    options.callback('token-abc');
    options['expired-callback']();
    options['error-callback']();
    options['timeout-callback']();
    assert.deepEqual(JSON.parse(JSON.stringify(messages)), [
        { type: 'token', token: 'token-abc' }, { type: 'expired' }, { type: 'error' }, { type: 'expired' },
    ]);
});

test('a script that fails to load is reported as an error', () => {
    const { appended, messages } = loadAppPage();
    appended[0].listeners.get('error')();
    assert.deepEqual(JSON.parse(JSON.stringify(messages)), [{ type: 'error' }]);
});

test('bkgTurnstileReset resets the rendered widget for a fresh token', () => {
    const { window, appended } = loadAppPage();
    assert.equal(typeof window.bkgTurnstileReset, 'function');
    assert.equal(window.bkgTurnstileReset(), false); // Turnstile not loaded yet; load will render.
    window.turnstile = fakeTurnstile();
    appended[0].listeners.get('load')();
    assert.equal(window.bkgTurnstileReset(), true);
    assert.equal(window.turnstile.renders.length, 1);
    assert.deepEqual(window.turnstile.resets, ['widget-1']);
});

test('in a normal browser the page only shows a note', () => {
    const { nodes, window, appended, messages } = loadAppPage({ app: false });
    assert.equal(nodes.get('appTurnstileStatus').textContent, 'This page is for the BKG app.');
    assert.deepEqual(appended, []);
    assert.equal(window.bkgTurnstileReset, undefined);
    assert.deepEqual(messages, []);
});

test('the app page is noindex and never contacts the intake API', () => {
    const html = fs.readFileSync(path.join(ROOT, 'app-turnstile.html'), 'utf8');
    assert.match(html, /<meta name="robots" content="noindex, nofollow">/);
    assert.match(html, /id="appTurnstileWidget"/);
    assert.match(html, /<script src="assets\/app-turnstile\.js"><\/script>/);
    for (const source of [html, controller]) {
        assert.doesNotMatch(source, /workers\.dev|\/v1\/applications|fetch\(|XMLHttpRequest/);
    }
});
