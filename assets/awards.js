// BKG Awards: Jump In ladder, built from the site's published V2 roster.
// Pure functions first (tested in tests/awards.test.cjs), page wiring last.
(function (root) {
    'use strict';

    const TIERS = [
        [1, 'Scout', 'Your first recruit gets their number. Welcome to the ring.'],
        [5, 'Trainer', 'Five new brass pounders carrying your name on their sponsor line.'],
        [10, 'Manager', 'Double digits. Ten members who answered your CQ and stayed.'],
        [25, 'Promoter', 'Twenty-five. The gang is noticing who keeps filling the roster.'],
        [50, 'Gym Boss', 'Fifty members. You are basically running a gym.'],
        [100, 'Undisputed', 'One hundred Jump Ins. Nobody has done it. Nobody argues with it.'],
    ];

    function normCall(s) {
        return String(s == null ? '' : s).toUpperCase().replace(/[^A-Z0-9]/g, '');
    }

    // A Jump In counts for the sponsor when the recruit names them and the
    // sponsor's number is lower (they were numbered first). The roster has no
    // QSO mode or issue date, so the CW / 2m checks happen at intake.
    function jumpInCounts(members) {
        const byNum = new Map();
        members.forEach(m => byNum.set(m.bkg_number, m));
        const counts = new Map();
        members.forEach(m => counts.set(m.bkg_number, 0));
        members.forEach(m => {
            const sp = m.sponsor_bkg_number;
            if (sp == null || sp === m.bkg_number || !byNum.has(sp)) return;
            if (!(sp < m.bkg_number)) return;
            counts.set(sp, counts.get(sp) + 1);
        });
        return counts;
    }

    function tierFor(n) {
        let t = null;
        for (const tier of TIERS) if (n >= tier[0]) t = tier;
        return t;
    }

    function nextTier(n) {
        for (const tier of TIERS) if (n < tier[0]) return tier;
        return null;
    }

    function ladder(roster) {
        const members = (roster && Array.isArray(roster.members)) ? roster.members : [];
        const counts = jumpInCounts(members);
        const rows = members.map(m => ({
            call: m.callsign, name: m.name, num: m.bkg_number, count: counts.get(m.bkg_number) || 0,
        }));
        const holders = TIERS.map(t => rows.filter(r => {
            const h = tierFor(r.count);
            return h && h[0] === t[0];
        }).length);
        const leaders = rows.filter(r => r.count > 0)
            .sort((a, b) => b.count - a.count || a.num - b.num);
        const byCall = new Map(rows.map(r => [normCall(r.call), r]));
        return { rows, holders, leaders, byCall, total: members.length };
    }

    function lookup(data, input) {
        const call = normCall(input);
        if (!call) return { kind: 'empty' };
        const row = data.byCall.get(call);
        if (!row) return { kind: 'missing', call };
        const tier = tierFor(row.count);
        const next = nextTier(row.count);
        const base = tier ? tier[0] : 0;
        const pct = next ? Math.round((row.count - base) / (next[0] - base) * 100) : 100;
        return { kind: 'found', call: row.call, count: row.count, tier, next, pct,
                 toNext: next ? next[0] - row.count : 0 };
    }

    const api = { TIERS, normCall, jumpInCounts, tierFor, nextTier, ladder, lookup };
    if (typeof module === 'object' && module.exports) { module.exports = api; return; }

    // ---------- page ----------
    const $ = sel => document.querySelector(sel);
    const esc = s => String(s == null ? '' : s).replace(/[&<>"]/g,
        c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
    let data = null;

    function renderCards() {
        $('#rankGrid').innerHTML = TIERS.map((t, i) => {
            const n = data ? data.holders[i] : null;
            const who = n == null ? '' : n ? n + ' member' + (n === 1 ? '' : 's') + ' at this rank' : 'NO ONE HOLDS THIS YET';
            return '<div class="rank-card"><div class="medallion"><span>' + t[0] + '</span></div>' +
                '<div class="rank-name">' + esc(t[1].toUpperCase()) + '</div>' +
                '<div class="rank-thresh">' + t[0] + ' Jump In' + (t[0] === 1 ? '' : 's') + '</div>' +
                '<div class="rank-blurb">' + esc(t[2]) + '</div>' +
                '<div class="rank-holders">' + who + '</div></div>';
        }).join('');
    }

    function renderLeaders() {
        const medals = ['🥇', '🥈', '🥉'];
        $('#leaderList').innerHTML = data.leaders.slice(0, 10).map((r, i) =>
            '<div class="kingpin-row"><span class="kingpin-rank">' + (medals[i] || (i + 1) + '.') + '</span>' +
            '<a class="kingpin-call" href="https://www.qrz.com/db/' + esc(r.call) + '" target="_blank" rel="noopener">' + esc(r.call) + '</a>' +
            '<span class="kingpin-name">' + esc(tierFor(r.count)[1].toUpperCase()) + '</span>' +
            '<span class="kingpin-count">' + r.count + ' Jump In' + (r.count === 1 ? '' : 's') + '</span></div>'
        ).join('');
    }

    function check() {
        const out = $('#lookupOut');
        if (!data) { out.textContent = 'Roster is still loading.'; return; }
        const r = lookup(data, $('#lookupCall').value);
        if (r.kind === 'empty') { out.textContent = ''; return; }
        if (r.kind === 'missing') {
            out.innerHTML = esc(r.call) + ' is not on the roster. No number, no awards. ' +
                '<a href="recruit.html">Get jumped in.</a>';
            return;
        }
        let h = '<strong class="hl">' + esc(r.call) + '</strong>: ' + r.count + ' Jump In' + (r.count === 1 ? '' : 's') + '. ';
        h += r.tier ? 'Rank: <strong class="hl">' + esc(r.tier[1].toUpperCase()) + '</strong>.' : 'No rank yet. Jump somebody in!';
        h += r.next
            ? '<br>' + r.toNext + ' more to ' + esc(r.next[1].toUpperCase()) + '.<div class="bar"><i style="width:' + r.pct + '%"></i></div>'
            : '<br>Top of the ladder.';
        out.innerHTML = h;
    }

    renderCards();
    $('#lookupGo').addEventListener('click', check);
    $('#lookupCall').addEventListener('keydown', e => { if (e.key === 'Enter') check(); });
    fetch('data/v2/roster.json')
        .then(r => { if (!r.ok) throw new Error('returned ' + r.status); return r.json(); })
        .then(json => {
            data = ladder(json);
            renderCards();
            renderLeaders();
            $('#rosterNote').textContent = data.total + ' members on the roster';
        })
        .catch(e => { $('#rosterNote').textContent = "Couldn't load the roster: " + e.message; });
})(this);
