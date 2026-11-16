const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');

const page = readFileSync(join(__dirname, '..', 'outbreak.html'), 'utf8');
const script = page.match(/<script>([\s\S]*?)<\/script>/)[1];

function element() {
    const handlers = new Map();
    return {
        dataset: {},
        classList: { contains: () => false, toggle() {}, add() {}, remove() {} },
        addEventListener(type, handler) { handlers.set(type, handler); },
        dispatch(type) { handlers.get(type)(); },
        setAttribute() {},
        appendChild() {},
    };
}

function openOutbreak(members) {
    const elements = new Map();
    const frames = [];
    class ReplayDate extends Date {
        constructor(...args) {
            super(...(args.length ? args : [2026, 10, 16]));
        }
    }
    function getElementById(id) {
        if (!elements.has(id)) elements.set(id, element());
        return elements.get(id);
    }
    const california = element();
    california.dataset.abbr = 'CA';
    california.getBBox = () => ({ x: 0, y: 0, width: 1, height: 1 });
    getElementById('obMapSvg').querySelectorAll = () => [california];
    getElementById('bkg-outbreak-data').textContent = JSON.stringify(members);
    Object.defineProperty(getElementById('obStats'), 'innerHTML', {
        set(html) {
            getElementById('obR0Tile').textContent = html.match(/>([^<]+)<\/div><div class="tree-stat-label">R₀/)[1];
        },
    });
    runInNewContext(script, {
        Date: ReplayDate,
        document: { getElementById, createElementNS: element, addEventListener() {}, querySelectorAll: () => [] },
        window: {}, setTimeout() {}, clearTimeout() {},
        requestAnimationFrame(callback) { return frames.push(callback); },
        cancelAnimationFrame() { frames.length = 0; },
    });
    return {
        getElementById,
        california,
        reproduction: () => getElementById('obR0Tile').textContent,
        scrub(day) {
            const scrub = getElementById('obScrub');
            scrub.value = day;
            scrub.dispatch('input');
        },
        playTo(day) {
            getElementById('obPlay').dispatch('click');
            for (let time = 0; time <= 10000; time += 16) {
                frames.shift()(time);
                if (getElementById('obDayNum').textContent === `DAY ${day}`) return;
            }
            assert.fail(`Replay did not reach day ${day}`);
        },
    };
}

function member(num, call, date, sponsor = null, state = null) {
    return { num, call, date, sponsor, state, dx: null };
}

test('Outbreak uses recorded join dates for the 30-day metric at each replay cutoff', () => {
    const outbreak = openOutbreak([
        member(1, 'K1FO', '2026-10-01'),
        member(2, 'K2SP', '2026-10-05'),
        member(3, 'K3CA', '2026-11-10', null, 'CA'),
        member(4, 'K4IN', '2026-10-10', 'K2SP', 'CA'),
        member(5, 'K5ON', '2026-11-04', 'K2SP'), // Day 30 counts.
        member(6, 'K6LT', '2026-11-05', 'K2SP'),
        member(7, 'K7PR', '2026-10-04', 'K2SP'), // Joined before the sponsor.
        member(8, 'K8UN', null, 'K2SP'),
        member(9, 'K9DS', '2026-10-08'),
        member(10, 'K0CH', '2026-10-09', 'K9DS'),
    ]);

    assert.equal(outbreak.reproduction(), '?');
    outbreak.scrub(35);
    assert.equal(outbreak.reproduction(), '0.00'); // Later member numbers have not appeared.
    assert.equal(outbreak.getElementById('obCaseNum').textContent, 'CASE 2 / 10');
    assert.equal(outbreak.getElementById('obDayDate').textContent, 'Nov 5, 2026');

    outbreak.scrub(40);
    assert.equal(outbreak.reproduction(), '0.50'); // Three recruits across six mature members.
    assert.equal(outbreak.getElementById('obCaseNum').textContent, 'CASE 10 / 10');
    outbreak.california.dispatch('click');
    assert.match(outbreak.getElementById('obTurfBody').innerHTML, /2 cases — first: Oct 10, 2026/);
});

test('Outbreak updates the 30-day metric during a quiet stretch of replay', () => {
    const outbreak = openOutbreak([
        member(1, 'K1FO', '2026-10-01'),
        member(2, 'K2IN', '2026-10-02', 'K1FO'),
        member(3, 'K3CA', '2026-11-10'),
    ]);
    outbreak.scrub(29);
    assert.equal(outbreak.reproduction(), '?');
    outbreak.playTo(30);
    assert.equal(outbreak.reproduction(), '1.00');
});
