const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../badges.html'), 'utf8');
const controller = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)]
  .map(match => match[1]).find(source => source.includes('const ROSTER_URL'));

function fixture() {
  return {schema_version: '2', generated_at: '2026-10-28T12:00:00Z', members: [
    {bkg_number: 700, callsign: 'VE6SYN', name: 'Synthetic Holder', qth: 'Alberta'},
    {bkg_number: 701, callsign: 'N0SYN', name: 'Other Member', qth: 'New York'},
  ], og_assignments: [
    {region_key: 'CA:AB', region_label: 'AB', bkg_number: 700},
    {region_key: 'QTH:CANADA', region_label: 'Canada', bkg_number: 700},
  ]};
}

function harness() {
  const elements = new Map();
  function element() {
    return {value: '', textContent: '', innerHTML: '', hidden: false, dataset: {}, children: [], listeners: {},
      classList: {add() {}, remove() {}, toggle() {}},
      setAttribute() {}, removeAttribute() {},
      append(...children) { this.children.push(...children); },
      replaceChildren(...children) { this.children = children; },
      addEventListener(name, callback) { this.listeners[name] = callback; }};
  }
  const document = {
    querySelector(selector) {
      if (!elements.has(selector)) elements.set(selector, element());
      return elements.get(selector);
    },
    querySelectorAll() { return []; },
    createElement: element,
  };
  const context = vm.createContext({document, Blob, TextEncoder, Uint8Array, Date,
    setTimeout() {}, clearTimeout() {},
    badge(options) { return `<svg><title>${options.callsign} — ${options.tab}</title></svg>`; },
  });
  // Exercise the shipped controller without starting font/network bootstrapping.
  vm.runInContext(controller.slice(0, controller.lastIndexOf('unpackFaces()')), context);
  context.input = fixture();
  const run = source => vm.runInContext(source, context);
  const json = source => JSON.parse(JSON.stringify(run(source)));
  run('ROSTER = parseRoster(input)');
  return {context, run, json, elements};
}

test('live V2 fetch and pasted/file loading retain every title and count titles, not holders', async () => {
  const {context, run, json, elements} = harness();
  const urls = [];
  context.fetch = async url => { urls.push(url); return {ok: true, json: async () => fixture()}; };
  await run('pullLive()');
  assert.deepEqual(urls, ['https://bkg-public-roster.cool-cake-fac1.workers.dev/api/roster/export/v2']);
  assert.deepEqual(json('titlesFor(ROSTER.members[0]).map(title => title.tab)'), ['AB OG', 'CANADA OG']);
  assert.equal(elements.get('#rosterCount').textContent, '2 members · 2 OG titles');
  run('loadRoster(JSON.stringify(input))');
  assert.deepEqual(json('titlesFor(ROSTER.members[0]).map(title => title.key)'), ['CA:AB', 'QTH:CANADA']);
  assert.match(elements.get('#refreshNote').textContent, /roster you loaded/);
});

test('choosing a title updates the preview and download name, while ordinary members have no title', () => {
  const {run, elements} = harness();
  run('choose(ROSTER.members[0])');
  assert.equal(elements.get('#ogTitleRow').hidden, false);
  assert.deepEqual(elements.get('#ogTitle').children.map(option => option.textContent), ['AB OG', 'CANADA OG']);
  assert.equal(elements.get('#tab').value, 'AB OG');
  elements.get('#ogTitle').value = 'QTH:CANADA';
  elements.get('#ogTitle').listeners.change();
  assert.equal(elements.get('#tab').value, 'CANADA OG');
  assert.match(run('lastSvg'), /VE6SYN — CANADA OG/);
  assert.equal(run('slug()'), 'bkg-ve6syn-canada-og');
  run('choose(ROSTER.members[1])');
  assert.equal(elements.get('#ogTitleRow').hidden, true);
  assert.equal(elements.get('#tab').value, '');
});

test('the bulk download contains one distinct SVG per title, independent of the selected badge', async () => {
  const {context, run, elements} = harness();
  run('choose(ROSTER.members[1])');
  context.save = async (name, blob) => { context.saved = {name, blob}; return true; };
  await elements.get('#dlZip').listeners.click();
  assert.equal(context.saved.name, 'bkg-og-badges.zip');
  const archive = Buffer.from(await context.saved.blob.arrayBuffer());
  const files = new Map();
  let offset = 0;
  while (archive.readUInt32LE(offset) === 0x04034b50) {
    const length = archive.readUInt32LE(offset + 18);
    const nameLength = archive.readUInt16LE(offset + 26);
    const extraLength = archive.readUInt16LE(offset + 28);
    const name = archive.subarray(offset + 30, offset + 30 + nameLength).toString();
    const start = offset + 30 + nameLength + extraLength;
    assert.equal(files.has(name), false);
    files.set(name, archive.subarray(start, start + length).toString());
    offset = start + length;
  }
  assert.deepEqual([...files.keys()], ['bkg-ve6syn-ca-ab-og.svg', 'bkg-ve6syn-qth-canada-og.svg']);
  assert.match(files.get('bkg-ve6syn-ca-ab-og.svg'), /VE6SYN — AB OG/);
  assert.match(files.get('bkg-ve6syn-qth-canada-og.svg'), /VE6SYN — CANADA OG/);
  assert.equal(elements.get('#dlZip').disabled, false);
});

test('region keys distinguish different titles that have the same display label', () => {
  const {context, run, json} = harness();
  context.input.og_assignments[1].region_label = 'AB';
  run('ROSTER = parseRoster(input)');
  const files = json('ogBadges()');
  assert.equal(files.length, 2);
  assert.equal(new Set(files.map(file => file.filename)).size, 2);
});

test('invalid snapshots preserve the last usable roster and never fall back to V1', async () => {
  const {context, run, json, elements} = harness();
  const before = json('ROSTER');
  const invalid = [null, {...fixture(), schema_version: '1'}, {...fixture(), og_assignments: null}];
  const orphan = fixture(); orphan.og_assignments[0].bkg_number = 999; invalid.push(orphan);
  const duplicate = fixture(); duplicate.og_assignments.push(duplicate.og_assignments[0]); invalid.push(duplicate);
  for (const input of invalid) {
    context.bad = input;
    run('loadRoster(JSON.stringify(bad))');
    assert.deepEqual(json('ROSTER'), before);
  }
  const urls = [];
  context.fetch = async url => { urls.push(url); return {ok: true, json: async () => invalid[1]}; };
  await run('pullLive()');
  assert.equal(urls.length, 1);
  assert.ok(urls[0].endsWith('/v2'));
  assert.deepEqual(json('ROSTER'), before);
  assert.match(elements.get('#refreshNote').textContent, /Load a V2 roster export/);
});

test('loading an empty roster clears a previously selected member and title', () => {
  const {context, run, elements} = harness();
  run('choose(ROSTER.members[0])');
  context.input = {...fixture(), members: [], og_assignments: []};
  run('loadRoster(JSON.stringify(input))');
  assert.equal(elements.get('#callsign').value, '');
  assert.equal(elements.get('#tab').value, '');
  assert.equal(elements.get('#ogTitleRow').hidden, true);
});
