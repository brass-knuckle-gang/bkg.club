const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../nearby.html'), 'utf8');
const controller = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)]
    .map(match => match[1])
    .find(source => source.includes('bkg-geo-data'));
const mappedMember = { call: 'N0SYN1', name: 'Fixture Op', num: 1, lat: 41, lon: -88, grid: 'EN61AB' };
const unmappedMember = { call: 'N0SYN7', num: 7 };
const locationHit = { lat: '41.85', lon: '-87.65', display_name: 'Chicago' };

function nearby({ mapped = [mappedMember], geocode = async () => ({ ok: true, json: async () => [locationHit] }) } = {}) {
    const elements = new Map();
    const document = {
        getElementById(id) {
            if (!elements.has(id)) {
                elements.set(id, {
                    value: id === 'radiusSelect' ? '100' : '',
                    style: {},
                    classList: { toggle() {} },
                    listeners: {},
                    addEventListener(name, callback) { this.listeners[name] = callback; },
                });
            }
            return elements.get(id);
        },
    };
    document.getElementById('bkg-geo-data').textContent = JSON.stringify(mapped);
    document.getElementById('bkg-unmapped-members').textContent = JSON.stringify([unmappedMember]);
    const requests = [];
    vm.runInNewContext(controller, {
        document,
        fetch(url) {
            requests.push(new URL(url));
            return geocode();
        },
    });

    return {
        requests,
        status: () => document.getElementById('statusLine').textContent,
        results: () => document.getElementById('resultsList').innerHTML,
        demoShown: () => document.getElementById('demoBanner').style.display === 'block',
        submit(query) {
            document.getElementById('addressInput').value = query;
            return document.getElementById('searchForm').listeners.submit({ preventDefault() {} });
        },
    };
}

test('member matches, members without coordinates, and member search misses stay local', async () => {
    const page = nearby();
    await page.submit(' bkg #001 ');
    assert.match(page.status(), /Found N0SYN1/);
    assert.match(page.results(), /0\.0 mi/);

    for (const query of ['n0syn7', 'BKG #7']) {
        await page.submit(query);
        assert.match(page.status(), /No published map location for N0SYN7/);
    }

    for (const query of ['N0ZZZ', 'EA8/M0ABC', '#9999']) {
        await page.submit(query);
        assert.match(page.status(), /No member found/);
    }

    assert.deepEqual(page.requests, []);
});

test('a published roster with no coordinates does not show demo members', async () => {
    const page = nearby({ mapped: [] });
    await page.submit('N0SYN7');
    assert.match(page.status(), /No published map location for N0SYN7/);
    assert.equal(page.demoShown(), false);
    assert.deepEqual(page.requests, []);
});

test('grid squares work locally and region, ZIP, postal, and address searches geocode', async () => {
    const page = nearby();
    for (const grid of ['CO49', 'co49tx']) {
        await page.submit(grid);
        assert.equal(page.status(), 'Pin dropped: ' + grid.toUpperCase());
    }
    assert.deepEqual(page.requests, []);

    await page.submit('WA');
    assert.equal(page.requests[0].searchParams.get('state'), 'Washington');
    assert.equal(page.requests[0].searchParams.get('country'), 'United States');
    for (const query of ['60601', 'K1A 0B1', '123 Main Street']) {
        await page.submit(query);
        assert.equal(page.requests.at(-1).searchParams.get('q'), query);
    }
});

test('an older geocoder response cannot replace a newer member result', async () => {
    let respond;
    const page = nearby({ geocode: () => new Promise(resolve => { respond = resolve; }) });
    const addressSearch = page.submit('Chicago');
    await page.submit('N0SYN1');
    respond({ ok: true, json: async () => [locationHit] });
    await addressSearch;
    assert.match(page.status(), /Found N0SYN1/);
});

test('eight-character grid searches use the finer center locally', async () => {
    const page = nearby({ mapped: [{ ...mappedMember, grid: 'FN31PR12', lat: 41.71875, lon: -72.7375 }] });
    await page.submit('fn31pr12');
    assert.equal(page.status(), 'Pin dropped: FN31PR12');
    assert.match(page.results(), /0\.0 mi/);
    assert.deepEqual(page.requests, []);
});
