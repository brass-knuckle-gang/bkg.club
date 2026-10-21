/* ROSTER v2 (index.html #roster): search, filters, sorting, list/wall view and
   the per-member rap sheet. Reads the rows render_roster_v2() writes into the
   page, so the roster still works as a plain list without JavaScript. Does
   nothing when the classic roster is built (no .r2 element). */
(function () {
    "use strict";
    var root = document.querySelector(".r2");
    if (!root) return;

    var bar = root.querySelector(".r2-bar");
    var out = root.querySelector(".r2-out");
    var jump = root.querySelector(".r2-jump");
    var fame = root.querySelector(".r2-fame");
    var result = root.querySelector(".r2-result");
    var query = root.querySelector("#r2-q");
    var sortChip = root.querySelector(".r2-sort");
    var ghost = root.querySelector(".r2-ghost");
    var BLOCK = 100;
    var DAY = 864e5;

    // ---------- model, read from the server-rendered rows ----------
    var members = [];
    var byCall = {};
    root.querySelectorAll(".r2-row[id]").forEach(function (el) {
        var img = el.querySelector(".r2-shot img");
        var m = {
            el: el,
            call: el.id,
            n: Number(el.dataset.n),
            name: el.querySelector(".member-name").textContent,
            place: el.dataset.place || "",
            flag: el.dataset.flag || "",
            joined: el.dataset.joined || "",
            sponsor: el.dataset.sponsor || "",
            photo: img ? img.getAttribute("src") : "",
            badges: el.querySelector(".r2-badges").innerHTML,
            og: !!el.querySelector(".state-og-badge, .r2-og, .r2-boss"),
            recruits: []
        };
        m.hay = (m.call + " " + m.name + " " + m.place).toUpperCase();
        members.push(m);
        byCall[m.call.toUpperCase()] = m;
    });
    members.forEach(function (m) {
        var s = byCall[m.sponsor.toUpperCase()];
        if (s) s.recruits.push(m);
    });
    var now = Date.now();
    members.forEach(function (m) {
        var t = m.joined ? Date.parse(m.joined + "T00:00:00Z") : NaN;
        m.fresh = !isNaN(t) && now - t <= 30 * DAY;
    });

    // ---------- state ----------
    var SORTS = [["num", "Sort: BKG #"], ["newest", "Sort: Newest"], ["call", "Sort: Callsign"], ["place", "Sort: State"]];
    var state = { q: "", filter: "all", sort: "num" };

    function pad(n) { return String(n).padStart(3, "0"); }
    function esc(s) {
        return String(s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; });
    }

    function matches(m) {
        if (state.filter === "new" && !m.fresh) return false;
        if (state.filter === "og" && !m.og) return false;
        if (state.filter === "recruiters" && !m.recruits.length) return false;
        if (state.filter === "photo" && !m.photo) return false;
        var q = state.q;
        if (!q) return true;
        if (/^#?\d+$/.test(q)) return m.n === Number(q.replace("#", ""));
        return m.hay.indexOf(q) >= 0;
    }

    function ordered(list) {
        if (state.sort === "newest") return list.slice().sort(function (a, b) { return b.n - a.n; });
        if (state.sort === "call") return list.slice().sort(function (a, b) { return a.call < b.call ? -1 : a.call > b.call ? 1 : 0; });
        if (state.sort === "place") {
            return list.slice().sort(function (a, b) {
                if (!a.place !== !b.place) return a.place ? -1 : 1;
                return a.place < b.place ? -1 : a.place > b.place ? 1 : a.n - b.n;
            });
        }
        if (state.filter === "recruiters") return list.slice().sort(function (a, b) { return b.recruits.length - a.recruits.length || a.n - b.n; });
        return list;
    }

    function groupKey(m) {
        if (state.sort === "place") return m.place ? (m.flag ? m.flag + " " : "") + m.place : "UNKNOWN TURF";
        if (state.sort === "call") return /[A-Z]/.test(m.call[0]) ? m.call[0] : "#";
        if (state.sort === "newest") {
            if (!m.joined) return "DATE UNKNOWN";
            return new Date(m.joined + "T00:00:00Z").toLocaleDateString("en-US", { month: "long", year: "numeric", timeZone: "UTC" }).toUpperCase();
        }
        if (state.filter === "recruiters") return "RANKED BY RECRUITS";
        var low = Math.floor((m.n - 1) / BLOCK) * BLOCK + 1;
        return "#" + pad(low) + "–#" + pad(low + BLOCK - 1);
    }

    function render() {
        var list = ordered(members.filter(matches));
        var narrowed = state.q || state.filter !== "all";
        var plain = !narrowed && state.sort === "num";
        fame.hidden = !!narrowed;
        result.innerHTML = narrowed
            ? "<b>" + list.length + "</b> of " + members.length + " match"
            : "Showing all <b>" + members.length + "</b>";
        out.textContent = "";
        jump.textContent = "";
        if (!list.length) {
            out.innerHTML = '<div class="r2-empty"><b>NOBODY BY THAT CALL</b>Try part of a callsign, a first name, a state (WA) or a number (#7).</div>';
            return;
        }
        var groups = [];
        list.forEach(function (m) {
            var key = groupKey(m);
            var g = groups[groups.length - 1];
            if (!g || g.key !== key) groups.push(g = { key: key, items: [] });
            g.items.push(m);
        });
        groups.forEach(function (g, i) {
            var section = document.createElement("section");
            section.className = "r2-block";
            section.id = "r2-g" + i;
            section.innerHTML = '<div class="r2-head"><h3>' + esc(g.key) + "</h3><span>" + g.items.length + " member" + (g.items.length === 1 ? "" : "s") + "</span></div>";
            var ol = document.createElement("ol");
            ol.className = "r2-rows";
            g.items.forEach(function (m) { ol.appendChild(m.el); });
            if (plain && i === groups.length - 1 && ghost) ol.appendChild(ghost);
            section.appendChild(ol);
            out.appendChild(section);
        });
        if (groups.length > 2) {
            jump.innerHTML = groups.map(function (g, i) {
                return '<a href="#r2-g' + i + '">' + esc(g.key.replace(/–#\d+$/, "")) + "</a>";
            }).join("");
        }
    }

    // ---------- controls ----------
    bar.hidden = false;
    var typing;
    query.addEventListener("input", function () {
        clearTimeout(typing);
        typing = setTimeout(function () { state.q = query.value.trim().toUpperCase(); render(); }, 80);
    });
    root.querySelector(".r2-chips").addEventListener("click", function (e) {
        var chip = e.target.closest(".r2-chip");
        if (!chip) return;
        if (chip === sortChip) {
            var i = SORTS.findIndex(function (s) { return s[0] === state.sort; });
            var next = SORTS[(i + 1) % SORTS.length];
            state.sort = next[0];
            sortChip.textContent = next[1];
        } else {
            state.filter = chip.dataset.filter;
            root.querySelectorAll(".r2-chip[data-filter]").forEach(function (c) { c.setAttribute("aria-pressed", String(c === chip)); });
        }
        render();
    });

    var viewButtons = root.querySelectorAll(".r2-views button");
    function setView(view, remember) {
        root.dataset.view = view;
        viewButtons.forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.view === view)); });
        if (remember) { try { localStorage.setItem("bkg-roster-view", view); } catch (e) { /* private mode */ } }
    }
    viewButtons.forEach(function (b) { b.addEventListener("click", function () { setView(b.dataset.view, true); }); });
    var savedView = null;
    try { savedView = localStorage.getItem("bkg-roster-view"); } catch (e) { /* private mode */ }
    setView(savedView === "list" || savedView === "wall" ? savedView
        : (window.matchMedia && matchMedia("(min-width: 720px)").matches ? "wall" : "list"), false);

    // Keep block headers pinned just under the search bar, whatever its height.
    function barHeight() { root.style.setProperty("--r2-bar-h", bar.offsetHeight + "px"); }
    barHeight();
    window.addEventListener("resize", barHeight);
    if (window.ResizeObserver) new ResizeObserver(barHeight).observe(bar);
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(barHeight);

    // ---------- rap sheet ----------
    var scrim = document.createElement("div");
    scrim.className = "r2 r2-scrim";
    scrim.hidden = true;
    var sheet = document.createElement("div");
    sheet.className = "r2 r2-sheet";
    sheet.setAttribute("role", "dialog");
    sheet.setAttribute("aria-modal", "true");
    sheet.setAttribute("aria-labelledby", "r2-sheet-call");
    sheet.hidden = true;
    document.body.appendChild(scrim);
    document.body.appendChild(sheet);
    var opener = null;

    function callButton(m) { return '<button type="button" class="r2-linkish" data-call="' + esc(m.call) + '">' + esc(m.call) + "</button>"; }
    function shot(m) {
        if (m.photo) return '<span class="r2-shot"><img src="' + esc(m.photo) + '" alt=""></span>';
        return '<span class="r2-shot"><span class="r2-ini">' + esc(m.el.querySelector(".r2-ini").textContent) + "</span></span>";
    }
    function longDate(iso) {
        return new Date(iso + "T00:00:00Z").toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric", timeZone: "UTC" });
    }
    function shareLink(m) { return location.origin + location.pathname + "#" + m.call; }

    function open(m, from) {
        if (from) opener = from;
        var sponsor = byCall[m.sponsor.toUpperCase()];
        var recruits = m.recruits.length
            ? '<div class="r2-recruits">' + m.recruits.map(callButton).join("") + "</div>"
            : '<span class="r2-muted">None yet. Get out there and pound some brass!</span>';
        sheet.innerHTML =
            '<div class="r2-grab"></div><button type="button" class="r2-close" aria-label="Close">×</button>' +
            '<div class="r2-top">' + shot(m) + "<div>" +
            '<div class="r2-scall" id="r2-sheet-call">' + esc(m.call) + "</div>" +
            '<div class="r2-badges">' + m.badges + "</div>" +
            '<div class="r2-sname">' + esc(m.name) + '</div><div class="r2-snum">BKG #' + pad(m.n) + "</div></div></div>" +
            '<dl class="r2-facts">' +
            (m.joined ? "<dt>Joined</dt><dd>" + longDate(m.joined) + "</dd>" : "") +
            (m.place ? "<dt>Turf</dt><dd>" + esc((m.flag ? m.flag + " " : "") + m.place) + "</dd>" : "") +
            "<dt>Worked in by</dt><dd>" + (sponsor ? callButton(sponsor) + ' <span class="r2-muted">BKG #' + pad(sponsor.n) + "</span>"
                : m.n === 1 ? "Founded the gang" : '<span class="r2-muted">Not on record</span>') + "</dd>" +
            "<dt>Recruits (" + m.recruits.length + ")</dt><dd>" + recruits + "</dd></dl>" +
            '<div class="r2-actions"><a class="r2-btn r2-btn-primary" href="https://www.qrz.com/db/' + encodeURIComponent(m.call) + '" target="_blank" rel="noopener">QRZ PAGE</a>' +
            '<button type="button" class="r2-btn r2-btn-ghost r2-copy">COPY LINK</button></div>' +
            '<p class="r2-muted r2-copied" aria-live="polite">' + esc(shareLink(m)) + "</p>";
        sheet.hidden = false;
        scrim.hidden = false;
        sheet.scrollTop = 0;
        sheet.querySelector(".r2-close").focus();
        try { history.replaceState(null, "", "#" + m.call); } catch (e) { /* file:// previews */ }
    }

    function close() {
        if (sheet.hidden) return;
        sheet.hidden = true;
        scrim.hidden = true;
        try { history.replaceState(null, "", location.pathname + location.search); } catch (e) { /* file:// previews */ }
        if (opener) { opener.focus(); opener = null; }
    }

    function selectText(el) {
        var range = document.createRange();
        range.selectNodeContents(el);
        var sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
    }

    sheet.addEventListener("click", function (e) {
        if (e.target.closest(".r2-close")) return close();
        var copy = e.target.closest(".r2-copy");
        if (copy) {
            var note = sheet.querySelector(".r2-copied");
            var text = note.textContent.replace(/^Copied /, "");
            if (navigator.clipboard && navigator.clipboard.writeText) {
                navigator.clipboard.writeText(text).then(function () { note.textContent = "Copied " + text; }, function () { selectText(note); });
            } else {
                selectText(note);
            }
            return;
        }
        var other = e.target.closest("[data-call]");
        if (other && byCall[other.dataset.call.toUpperCase()]) open(byCall[other.dataset.call.toUpperCase()]);
    });
    scrim.addEventListener("click", close);
    document.addEventListener("keydown", function (e) { if (e.key === "Escape") close(); });

    root.addEventListener("click", function (e) {
        var card = e.target.closest(".r2-fcard");
        var row = e.target.closest(".r2-row[id]");
        var target = card ? byCall[card.dataset.call.toUpperCase()] : row ? byCall[row.id.toUpperCase()] : null;
        if (!target) return;
        if (e.target.closest("a") && (e.metaKey || e.ctrlKey || e.shiftKey || e.button === 1)) return;
        // Without JavaScript the callsign goes to QRZ and the strip jumps to the
        // row; with it, both open the rap sheet, which links to QRZ.
        e.preventDefault();
        open(target, card || row.querySelector(".member-callsign"));
    });

    function openFromHash() {
        var m = byCall[decodeURIComponent(location.hash.slice(1)).toUpperCase()];
        if (m) open(m);
    }
    window.addEventListener("hashchange", openFromHash);
    openFromHash();
})();
