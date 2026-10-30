/* Shared form rules; also exercised by Node's offline test runner. */
(function (root) {
    'use strict';
    function normalizeCall(value) {
        return value.trim().replace(/[a-z]/g, function (c) { return c.toUpperCase(); });
    }
    function time(value) {
        var match = /^(\d{2}):?(\d{2})$/.exec(value.trim());
        return match && Number(match[1]) < 24 && Number(match[2]) < 60
            ? match[1] + ':' + match[2] : null;
    }
    function utc(now) {
        var stamp = now.toISOString();
        return { qsoDate: stamp.slice(0, 10), qsoTime: stamp.slice(11, 16) };
    }
    function findSponsor(value, members) {
        var query = normalizeCall(value);
        var number = /^(?:BKG\s*|#)?(\d+)$/.exec(query);
        return members.find(function (m) {
            return number ? m.bkg_number === Number(number[1]) : m.callsign === query;
        });
    }
    function callIssue(value) {
        return !/^[A-Z0-9]+$/.test(normalizeCall(value))
            ? 'Use the base callsign with A–Z letters and 0–9 digits.' : '';
    }
    function payload(values) {
        var result = {};
        // Keep browser-only confirmations out of the existing intake contract.
        ['name', 'callsign', 'recruitEmail', 'submitterEmail', 'qth', 'sponsorName',
            'sponsorCallsign', 'sponsorNumber', 'qsoDate', 'qsoTime', 'bandMode', 'notes']
            .forEach(function (key) { if (typeof values[key] === 'string') result[key] = values[key]; });
        return result;
    }
    function pending(value) {
        if (!value || typeof value.key !== 'string' || typeof value.payload !== 'string'
            || !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value.key)) return null;
        try {
            var payload = JSON.parse(value.payload);
            if (!payload || typeof payload !== 'object' || Array.isArray(payload)
                || typeof payload.callsign !== 'string') return null;
            return value;
        } catch (_) { return null; }
    }
    function attempt(payload, previous, key) {
        if (previous && previous.payload === JSON.stringify(payload)) {
            return { key: previous.key, payload: previous.payload, startedAt: previous.startedAt };
        }
        if (previous && previous.state !== 'rejected') throw new Error('Resolve the original submission first.');
        return { key: key, payload: JSON.stringify(payload), startedAt: new Date().toISOString() };
    }
    function canEditAfter(status, recovering) {
        // A rejection of a retry cannot disprove acceptance of an earlier send.
        return !recovering && (status === 400 || status === 413 || status === 415
            || status === 422 || status === 429);
    }
    var api = { normalizeCall: normalizeCall, time: time, utc: utc, findSponsor: findSponsor,
        callIssue: callIssue, payload: payload, pending: pending, attempt: attempt, canEditAfter: canEditAfter };
    if (typeof module === 'object' && module.exports) module.exports = api;
    else root.BkgRecruit = api;
}(typeof globalThis === 'object' ? globalThis : this));
