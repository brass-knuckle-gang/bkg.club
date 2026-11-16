const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../badges.html'), 'utf8');
const controller = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)]
  .map(match => match[1]).find(source => source.includes('const ROSTER_URLS'));

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
    const attributes = new Map();
    return {value: '', textContent: '', innerHTML: '', hidden: false, disabled: false, dataset: {}, children: [], listeners: {},
      classList: {add() {}, remove() {}, toggle() {}},
      getAttribute(name) { return attributes.get(name) ?? null; },
      setAttribute(name, value) { attributes.set(name, String(value)); },
      removeAttribute(name) { attributes.delete(name); },
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
    badge(options) {
      return `<svg><title>${options.callsign} — ${options.tab}</title><desc>${options.freq ?? '144.025 MHZ'}</desc></svg>`;
    },
  });
  // Exercise the shipped controller without starting font/network bootstrapping.
  vm.runInContext(controller.slice(0, controller.lastIndexOf('unpackFaces()')), context);
  context.input = fixture();
  const run = source => vm.runInContext(source, context);
  const json = source => JSON.parse(JSON.stringify(run(source)));
  run('ROSTER = parseRoster(input)');
  return {context, run, json, elements};
}

test('live V2 fetch reads the published roster and keeps every title', async () => {
  const {context, run, json, elements} = harness();
  const urls = [];
  context.fetch = async url => { urls.push(url); return {ok: true, json: async () => fixture()}; };
  await run('pullLive()');
  assert.deepEqual(urls, ['data/v2/roster.json']);
  assert.deepEqual(json('titlesFor(ROSTER.members[0]).map(title => title.tab)'), ['AB OG', 'CANADA OG']);
  assert.equal(elements.get('#rosterCount').textContent, '2 members · 2 OG titles');
});

test('the live V1 rollback snapshot and pasted and uploaded exports use the same projection', async () => {
  const {context, run, json, elements} = harness();
  const urls = [];
  const v1 = {...fixture(), schema_version: '1'};
  context.fetch = async url => {
    urls.push(url);
    return url === 'data/v1/roster.json' ? {ok: true, json: async () => v1} : {ok: false, status: 404};
  };
  await run('pullLive()');
  assert.deepEqual(urls, ['data/v2/roster.json', 'data/v1/roster.json']);
  assert.deepEqual(json('titlesFor(ROSTER.members[0]).map(title => title.key)'), ['CA:AB', 'QTH:CANADA']);
  context.document.querySelector('#pasteJson').value = JSON.stringify(fixture());
  elements.get('#applyJson').listeners.click();
  assert.deepEqual(json('titlesFor(ROSTER.members[0]).map(title => title.key)'), ['CA:AB', 'QTH:CANADA']);
  assert.match(elements.get('#refreshNote').textContent, /roster you loaded/);
  v1.og_assignments.pop();
  const file = {text: async () => JSON.stringify(v1)};
  elements.get('#fileInput').listeners.change({target: {files: [file]}});
  await Promise.resolve();
  assert.deepEqual(json('titlesFor(ROSTER.members[0]).map(title => title.key)'), ['CA:AB']);
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

async function readZip(blob) {
  const archive = Buffer.from(await blob.arrayBuffer());
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
  return files;
}

const exportButtons = ['#dlSvg', '#dlPng', '#copySvg', '#dlZip'];

function captureExports(context) {
  const downloads = new Map(), rasterized = [], copied = [];
  context.save = async (name, contents) => { downloads.set(path.extname(name), contents); return true; };
  context.svgToPng = async svg => { rasterized.push(svg); return new Blob(); };
  context.navigator = {clipboard: {writeText: async svg => { copied.push(svg); }}};
  return {downloads, rasterized, copied};
}

function setFrequency(elements, value) {
  const field = elements.get('#frequency');
  field.value = value;
  field.listeners.input({target: field, currentTarget: field});
}

test('a custom frequency is normalized in the preview and all exports and persists across member selection', async () => {
  const {context, run, elements} = harness();
  const {downloads, rasterized, copied} = captureExports(context);
  run('choose(ROSTER.members[0])');
  setFrequency(elements, ' 144,050 ');
  run('choose(ROSTER.members[1])');
  const preview = elements.get('#preview').innerHTML;
  assert.match(preview, /N0SYN/);
  assert.match(preview, /144\.050 MHZ/);

  for (const id of exportButtons) await elements.get(id).listeners.click();
  assert.equal(downloads.get('.svg'), preview);
  assert.ok(downloads.has('.png'));
  assert.deepEqual(rasterized, [preview]);
  assert.deepEqual(copied, [preview]);
  const zip = await readZip(downloads.get('.zip'));
  assert.equal(zip.size, 2);
  for (const svg of zip.values()) assert.match(svg, /144\.050 MHZ/);

  setFrequency(elements, '  ');
  assert.match(elements.get('#preview').innerHTML, /144\.025 MHZ/);
  setFrequency(elements, ',5');
  assert.match(elements.get('#preview').innerHTML, /0\.5 MHZ/);
});

test('invalid frequencies keep the last valid preview and block every export until corrected', async () => {
  const {context, run, elements} = harness();
  const {downloads, rasterized, copied} = captureExports(context);
  run('choose(ROSTER.members[0])');
  setFrequency(elements, '146.520');
  const preview = elements.get('#preview').innerHTML;
  assert.match(preview, /146\.520 MHZ/);
  const field = elements.get('#frequency'), error = elements.get('#frequencyError');

  for (const invalid of ['144..050', '144.050 MHz', '1e3', '0', '-144.050']) {
    setFrequency(elements, invalid);
    assert.equal(elements.get('#preview').innerHTML, preview, invalid);
    assert.equal(field.getAttribute('aria-invalid'), 'true', invalid);
    assert.equal(error.hidden, false, invalid);
    assert.ok(error.textContent.trim(), invalid);
    for (const id of exportButtons) {
      assert.equal(elements.get(id).disabled, true, `${invalid} ${id}`);
      await elements.get(id).listeners.click();
    }
  }
  assert.equal(downloads.size, 0);
  assert.deepEqual(rasterized, []);
  assert.deepEqual(copied, []);

  setFrequency(elements, '433.500');
  assert.match(elements.get('#preview').innerHTML, /433\.500 MHZ/);
  assert.notEqual(field.getAttribute('aria-invalid'), 'true');
  assert.equal(error.hidden, true);
  for (const id of exportButtons) assert.equal(elements.get(id).disabled, false, id);
});

test('ZIP stays disabled during export and after an invalid edit', async () => {
  const {context, elements} = harness();
  let finishSave;
  context.save = () => new Promise(resolve => { finishSave = resolve; });
  const zipButton = elements.get('#dlZip');
  const pending = zipButton.listeners.click();

  setFrequency(elements, '144.050');
  assert.equal(zipButton.disabled, true);
  setFrequency(elements, '144..050');
  finishSave(true);
  await pending;
  assert.equal(zipButton.disabled, true);
  setFrequency(elements, '144.050');
  assert.equal(zipButton.disabled, false);
});

test('the bulk download contains one distinct SVG per title, independent of the selected badge', async () => {
  const {context, run, elements} = harness();
  run('choose(ROSTER.members[1])');
  context.save = async (name, blob) => { context.saved = {name, blob}; return true; };
  await elements.get('#dlZip').listeners.click();
  assert.equal(context.saved.name, 'bkg-og-badges.zip');
  const files = await readZip(context.saved.blob);
  assert.deepEqual([...files.keys()], ['bkg-ve6syn-ca-ab-og.svg', 'bkg-ve6syn-qth-canada-og.svg']);
  assert.match(files.get('bkg-ve6syn-ca-ab-og.svg'), /VE6SYN — AB OG/);
  assert.match(files.get('bkg-ve6syn-qth-canada-og.svg'), /VE6SYN — CANADA OG/);
  assert.equal(elements.get('#dlZip').disabled, false);
});

test('ZIP filenames remain distinct when region keys have the same slug', async () => {
  const {context, run, elements} = harness();
  context.input.og_assignments = [
    {region_key: 'QTH:TIMOR LESTE', region_label: 'East Timor', bkg_number: 700},
    {region_key: 'QTH:TIMOR-LESTE', region_label: 'Timor-Leste', bkg_number: 700},
  ];
  run('ROSTER = parseRoster(input)');
  context.save = async (name, blob) => { context.saved = {name, blob}; return true; };
  await elements.get('#dlZip').listeners.click();
  const files = await readZip(context.saved.blob);
  assert.deepEqual([...files.keys()], ['bkg-ve6syn-qth-timor-leste-og.svg', 'bkg-ve6syn-qth-timor-leste-og-2.svg']);
  assert.match(files.get('bkg-ve6syn-qth-timor-leste-og.svg'), /VE6SYN — EAST TIMOR OG/);
  assert.match(files.get('bkg-ve6syn-qth-timor-leste-og-2.svg'), /VE6SYN — TIMOR-LESTE OG/);
});

test('invalid snapshots preserve the last usable roster after both live URLs fail', async () => {
  const {context, run, json, elements} = harness();
  const before = json('ROSTER');
  const invalid = [null, {...fixture(), schema_version: '3'}, {...fixture(), og_assignments: null}];
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
  assert.deepEqual(urls, ['data/v2/roster.json', 'data/v1/roster.json']);
  assert.deepEqual(json('ROSTER'), before);
  assert.match(elements.get('#refreshNote').textContent, /Load a V1 or V2 roster export/);
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
