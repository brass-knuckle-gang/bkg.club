(function () {
    'use strict';
    const rules = window.BkgRecruit;
    const INTAKE_ENDPOINT = 'https://bkg-membership-intake.cool-cake-fac1.workers.dev/v1/applications';
    const PENDING_KEY = 'bkg-recruit-pending-v1';
    const SPONSOR_KEY = 'bkg-recruit-sponsor-v1';
    const RECEIPT_KEY = 'bkg-recruit-receipt-v1';
    const FIELDS = [
        ['sponsorCall', 'sponsorCallsign', 'Sponsor callsign'],
        ['sponsorNum', 'sponsorNumber', 'Sponsor BKG #'],
        ['sponsorName', 'sponsorName', 'Sponsor name'],
        ['email', 'submitterEmail', 'Your email'],
        ['recruitCall', 'callsign', 'Recruit callsign'],
        ['recruitName', 'name', 'Recruit name'],
        ['recruitQth', 'qth', 'Recruit state / province / country'],
        ['recruitEmail', 'recruitEmail', 'Recruit email'],
        ['qsoDate', 'qsoDate', 'QSO date (UTC)'],
        ['qsoTime', 'qsoTime', 'QSO time (UTC, 24-hour)'],
        ['qsoBand', 'bandMode', 'Band / mode'],
        ['notes', 'notes', 'Notes']
    ];
    const el = function (id) { return document.getElementById(id); };
    const val = function (id) { return el(id).value.trim(); };
    const form = el('recruitForm');
    const confirmPanel = el('formConfirm');
    const confirmSend = el('confirmSend');
    const confirmBack = el('confirmBack');
    const errorBox = el('formError');
    const note = document.querySelector('.form-note');
    const sendLabel = confirmSend.textContent;
    let members = [];
    let rosterLoaded = false;
    let reviewedPayload = null;
    let inFlight = false;
    let widgetId = null;
    // Keep the attempt in memory even when browser storage is unavailable.
    let pendingAttempt = rules.pending(readStorage('sessionStorage', PENDING_KEY));
    let rejectedAttempt = pendingAttempt && pendingAttempt.state === 'rejected' ? pendingAttempt : null;
    if (rejectedAttempt) pendingAttempt = null;
    function readStorage(kind, key) {
        try { return JSON.parse(window[kind].getItem(key) || 'null'); } catch (_) { return null; }
    }
    function writeStorage(kind, key, value) {
        try {
            if (value === null) window[kind].removeItem(key);
            else window[kind].setItem(key, JSON.stringify(value));
        } catch (_) { /* The current page still retains the attempt. */ }
    }
    function savePending(value) {
        pendingAttempt = value;
        rejectedAttempt = null;
        writeStorage('sessionStorage', PENDING_KEY, value);
    }
    function error(message) {
        errorBox.textContent = message;
        errorBox.classList.add('show');
        errorBox.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
    function clearError() {
        errorBox.classList.remove('show');
        form.querySelectorAll('[aria-invalid]').forEach(function (node) { node.removeAttribute('aria-invalid'); });
    }
    function showForm() {
        if (pendingAttempt || inFlight) return;
        confirmPanel.classList.remove('show');
        form.style.display = '';
        note.style.display = '';
    }
    function rememberSponsor() {
        writeStorage('localStorage', SPONSOR_KEY, el('rememberSponsor').checked ? {
            callsign: val('sponsorCall'), email: val('email')
        } : null);
    }
    function matchSponsor() {
        const value = val('sponsorCall');
        const member = rules.findSponsor(value, members);
        el('sponsorNum').value = member ? 'BKG' + member.bkg_number : '';
        el('sponsorName').value = member ? member.name : '';
        el('sponsorMatch').textContent = member
            ? member.callsign + ' · ' + member.name + ' · BKG #' + member.bkg_number
            : (rosterLoaded ? 'No matching member in this public roster. Check the callsign or request officer review.'
                : 'Public roster unavailable. Enter your callsign and request officer review if needed.');
        el('sponsorReviewRow').hidden = Boolean(member);
        if (member) el('sponsorReviewRequested').setCustomValidity('');
        const suggestion = !member && /O/i.test(value) ? rules.findSponsor(value.replace(/o/gi, '0'), members) : null;
        el('sponsorSuggestion').hidden = !suggestion;
        el('sponsorSuggestion').textContent = suggestion ? 'DID YOU MEAN ' + suggestion.callsign + '? USE THIS MEMBER' : '';
        el('sponsorSuggestion').onclick = function () {
            el('sponsorCall').value = suggestion.callsign;
            el('sponsorReviewRequested').checked = false;
            matchSponsor();
            validateRecruit();
        };
        return member;
    }
    function validateRecruit() {
        const call = rules.normalizeCall(val('recruitCall'));
        const sponsor = rules.findSponsor(val('sponsorCall'), members);
        const sponsorCall = sponsor ? sponsor.callsign : rules.normalizeCall(val('sponsorCall'));
        const member = members.find(function (m) { return m.callsign === call; });
        let issue = call ? rules.callIssue(call) : '';
        if (call && call === sponsorCall) issue = 'The sponsor and recruit must be different operators.';
        if (member) issue = call + ' is already BKG #' + member.bkg_number + '. Check the roster before submitting.';
        el('recruitCall').setCustomValidity(issue);
        const match = el('recruitMatch');
        match.replaceChildren();
        match.textContent = issue;
        if (member) {
            const link = document.createElement('a');
            link.href = 'index.html#roster';
            link.textContent = ' OPEN ROSTER';
            match.appendChild(link);
        }
        return !issue;
    }
        const recruitCallInput = document.getElementById('recruitCall');
        const qthInput = document.getElementById('recruitQth');
        const provinceSelect = document.getElementById('recruitProvince');
        const usRegionSelect = document.getElementById('recruitUsRegion');
        const qthLabel = document.getElementById('recruitQthLabel');
        function updateQthControl() {
            const callsign = recruitCallInput.value.trim().toUpperCase();
            const canadian = /^(?:VA|VE|VO|VY)\d/.test(callsign);
            const prefix = callsign.slice(0, 2);
            const american = /^[KNW]/.test(callsign) || (prefix.length === 2 && prefix >= 'AA' && prefix <= 'AL');
            const activeSelect = canadian ? provinceSelect : american ? usRegionSelect : null;
            qthInput.hidden = Boolean(activeSelect);
            qthInput.disabled = Boolean(activeSelect);
            [provinceSelect, usRegionSelect].forEach(function (select) {
                const active = select === activeSelect;
                select.hidden = !active;
                select.disabled = !active;
                select.required = active;
            });
            qthLabel.htmlFor = activeSelect ? activeSelect.id : 'recruitQth';
            if (activeSelect) {
                const current = qthInput.value.trim().toUpperCase();
                const option = Array.from(activeSelect.options).find(function (option) {
                    const aliases = (option.dataset.aliases || option.dataset.code || '').toUpperCase().split('|');
                    return option.value.toUpperCase() === current || aliases.includes(current);
                });
                activeSelect.value = option ? option.value : '';
                qthInput.value = activeSelect.value;
            }
        }
        recruitCallInput.addEventListener('input', updateQthControl);
        [provinceSelect, usRegionSelect].forEach(function (select) {
            select.addEventListener('change', function () {
                // Keep the canonical region in the API payload's QTH field.
                qthInput.value = select.value;
            });
        });
    function normalizeTime() {
        const normalized = rules.time(val('qsoTime'));
        el('qsoTime').setCustomValidity(normalized ? '' : 'Enter UTC time as 2130 or 21:30 (0000–2359).');
        if (normalized) el('qsoTime').value = normalized;
        return normalized;
    }
    function validateQso() {
        const time = normalizeTime();
        const future = time && Date.parse(val('qsoDate') + 'T' + time + ':00Z') > Date.now() + 300000;
        el('qsoDate').setCustomValidity(future ? 'The QSO date and time must not be in the future (UTC).' : '');
    }
    function payloadFromForm() {
        const payload = {};
        FIELDS.forEach(function (field) { payload[field[1]] = val(field[0]); });
        payload.formVersion = '2';
        payload.membershipConfirmed = el('membershipConfirmed').checked ? 'true' : '';
        payload.sponsorReviewRequested = el('sponsorReviewRequested').checked ? 'true' : '';
        return payload;
    }
    function fillForm(payload) {
        FIELDS.forEach(function (field) {
            if (typeof payload[field[1]] === 'string') el(field[0]).value = payload[field[1]];
        });
        el('membershipConfirmed').checked = payload.membershipConfirmed === 'true';
        el('sponsorReviewRequested').checked = payload.sponsorReviewRequested === 'true';
        updateQthControl();
    }
    function lockRecovery() {
        el('recoveryNote').hidden = !pendingAttempt;
        confirmBack.disabled = Boolean(pendingAttempt) || inFlight;
    }
    function renderWidget() {
        if (!window.turnstile) return;
        if (widgetId !== null) window.turnstile.reset(widgetId);
        else widgetId = window.turnstile.render('#turnstile', {
            sitekey: '0x4AAAAAAFsHyM6nyvrtrADC', action: 'recruit-application',
            theme: 'dark', 'refresh-expired': 'auto'
        });
    }
    function review(payload) {
        reviewedPayload = payload;
        const summary = el('confirmSummary');
        summary.replaceChildren();
        const rows = FIELDS.map(function (field) { return [field[2], payload[field[1]]]; });
        if (payload.formVersion === '2') {
            rows.push(['QSO and membership consent', payload.membershipConfirmed === 'true' ? 'Confirmed' : 'Not confirmed']);
            if (payload.sponsorReviewRequested === 'true') rows.push(['Sponsor', 'Officer review requested']);
        }
        rows.forEach(function (row) {
            const dt = document.createElement('dt');
            const dd = document.createElement('dd');
            dt.textContent = row[0];
            dd.textContent = row[1] || '— left blank —';
            summary.append(dt, dd);
        });
        form.style.display = 'none';
        note.style.display = 'none';
        confirmPanel.classList.add('show');
        lockRecovery();
        renderWidget();
        confirmPanel.scrollIntoView({ behavior: 'smooth' });
    }
    function showReceipt(receipt) {
        reviewedPayload = null;
        savePending(null);
        writeStorage('sessionStorage', RECEIPT_KEY, receipt);
        form.style.display = 'none';
        note.style.display = 'none';
        confirmPanel.classList.remove('show');
        el('receiptCode').textContent = receipt.receipt;
        el('receiptTime').textContent = 'Received ' + receipt.receivedAt + '. Pending officer review.';
        el('formSuccess').classList.add('show');
        el('formSuccess').scrollIntoView({ behavior: 'smooth' });
    }
    function markFields(names) {
        const extra = { membershipConfirmed: 'membershipConfirmed', sponsorReviewRequested: 'sponsorReviewRequested', formVersion: 'submitBtn' };
        (names || []).forEach(function (name) {
            const field = FIELDS.find(function (f) { return f[1] === name; });
            let control = field ? el(field[0]) : el(extra[name]);
            if (name === 'qth') control = !provinceSelect.disabled ? provinceSelect : !usRegionSelect.disabled ? usRegionSelect : qthInput;
            if (name === 'sponsorNumber') control = el('sponsorCall');
            if (control) control.setAttribute('aria-invalid', 'true');
        });
    }
    async function send() {
        if (inFlight || !reviewedPayload) return;
        clearError();
        const token = widgetId !== null && window.turnstile ? window.turnstile.getResponse(widgetId) : '';
        if (!token) { error('Complete the robot check, then send. If it did not load, reload or contact k7bkgang@gmail.com.'); return; }
        const recovering = Boolean(pendingAttempt);
        const attempt = rules.attempt(reviewedPayload, pendingAttempt || rejectedAttempt, crypto.randomUUID());
        savePending(attempt);
        rememberSponsor();
        inFlight = true;
        confirmBack.disabled = true;
        confirmSend.disabled = true;
        confirmSend.textContent = '📡 SENDING...';
        const controller = new AbortController();
        const timer = setTimeout(function () { controller.abort(); }, 20000);
        try {
            const response = await fetch(INTAKE_ENDPOINT, {
                method: 'POST', headers: { 'Content-Type': 'application/json', 'Idempotency-Key': attempt.key },
                body: JSON.stringify(Object.assign({}, JSON.parse(attempt.payload), { turnstileToken: token })),
                signal: controller.signal
            });
            const data = await response.json().catch(function () { return {}; });
            if (response.status === 202 && typeof data.receipt === 'string' && typeof data.receivedAt === 'string') {
                showReceipt(data);
                return;
            }
            const code = data.error && data.error.code || '';
            const editable = rules.canEditAfter(response.status, code, recovering);
            if (editable) {
                pendingAttempt = null;
                rejectedAttempt = Object.assign({}, attempt, { state: 'rejected' });
                writeStorage('sessionStorage', PENDING_KEY, rejectedAttempt);
                // showForm's guard also protects against accidental edits during sends.
                inFlight = false;
                showForm();
                markFields(data.error && data.error.fields);
            }
            let message;
            if (code === 'already_member') message = 'This recruit is already a member. Check the roster or contact the officers.';
            else if (code === 'sponsor_not_found') {
                el('sponsorReviewRow').hidden = false;
                message = 'The current membership records could not match your sponsor callsign. Check it or request officer review.';
            } else if (code === 'invalid_request') message = 'Check the highlighted fields, including callsigns, matching sponsor number, UTC date/time, and membership confirmation.';
            else if (code === 'idempotency_conflict') message = 'The server has different details under this submission key. Keep this attempt and contact k7bkgang@gmail.com; do not start another intake.';
            else if (response.status === 422) message = 'The robot check expired or failed. Complete a fresh check and retry.';
            else if (response.status === 429 || response.status === 503) {
                const seconds = Number(response.headers.get('Retry-After'));
                message = 'Intake is temporarily unavailable or busy. ' + (seconds > 0 ? 'Retry in about ' + seconds + ' seconds.' : 'Try again shortly.');
            } else message = 'The server could not confirm this submission. Retry the original details or contact k7bkgang@gmail.com.';
            if (pendingAttempt && code !== 'idempotency_conflict') message += ' Your original details and submission key are preserved for a safe retry.';
            error(message);
        } catch (_) {
            error('No receipt came back. Your submission may have arrived. Retry the original details to recover its receipt, or contact k7bkgang@gmail.com.');
        } finally {
            clearTimeout(timer);
            inFlight = false;
            confirmSend.disabled = false;
            confirmSend.textContent = sendLabel;
            lockRecovery();
            if (widgetId !== null && window.turnstile) window.turnstile.reset(widgetId);
        }
    }
    form.addEventListener('submit', function (event) {
        event.preventDefault();
        if (pendingAttempt || inFlight) return;
        clearError();
        const member = matchSponsor();
        if (member) el('sponsorCall').value = member.callsign;
        else el('sponsorCall').value = rules.normalizeCall(val('sponsorCall'));
        el('sponsorCall').setCustomValidity(rules.callIssue(val('sponsorCall')));
        el('sponsorReviewRequested').setCustomValidity(!member && !el('sponsorReviewRequested').checked
            ? 'Check your callsign, or request officer review for an unmatched sponsor.' : '');
        el('recruitCall').value = rules.normalizeCall(val('recruitCall'));
        validateRecruit();
        validateQso();
        if (!form.reportValidity()) return;
        review(payloadFromForm());
    });
    confirmBack.addEventListener('click', showForm);
    confirmSend.addEventListener('click', send);
    el('sponsorCall').addEventListener('input', function () {
        el('sponsorCall').setCustomValidity('');
        el('sponsorReviewRequested').checked = false;
        el('sponsorReviewRequested').setCustomValidity('');
        matchSponsor(); validateRecruit();
    });
    el('sponsorReviewRequested').addEventListener('change', function () { this.setCustomValidity(''); });
    el('recruitCall').addEventListener('input', validateRecruit);
    el('qsoTime').addEventListener('blur', validateQso);
    el('qsoTime').addEventListener('input', function () { this.setCustomValidity(''); el('qsoDate').setCustomValidity(''); });
    el('qsoDate').addEventListener('input', function () { this.setCustomValidity(''); });
    el('useUtcNow').addEventListener('click', function () {
        const now = rules.utc(new Date());
        el('qsoDate').value = now.qsoDate;
        el('qsoTime').value = now.qsoTime;
        validateQso();
    });
    el('rememberSponsor').addEventListener('change', rememberSponsor);
    el('copyReceipt').addEventListener('click', async function () {
        try {
            await navigator.clipboard.writeText(el('receiptCode').textContent);
            el('copyStatus').textContent = 'Copied!';
        } catch (_) { el('copyStatus').textContent = 'Select and copy the receipt above.'; }
    });
    el('anotherRecruit').addEventListener('click', function () {
        const sponsor = { call: val('sponsorCall'), email: val('email'), remember: el('rememberSponsor').checked };
        savePending(null);
        form.reset();
        el('sponsorCall').value = sponsor.call;
        el('email').value = sponsor.email;
        el('rememberSponsor').checked = sponsor.remember;
        form.querySelectorAll('input').forEach(function (input) { input.setCustomValidity(''); });
        writeStorage('sessionStorage', RECEIPT_KEY, null);
        el('copyStatus').textContent = '';
        el('formSuccess').classList.remove('show');
        clearError();
        showForm(); updateQthControl(); matchSponsor(); validateRecruit();
        el('recruitCall').focus();
    });
    el('turnstileScript').addEventListener('load', function () {
        if (confirmPanel.classList.contains('show')) renderWidget();
    });
    const remembered = readStorage('localStorage', SPONSOR_KEY);
    if (remembered && typeof remembered.callsign === 'string' && typeof remembered.email === 'string') {
        el('sponsorCall').value = remembered.callsign;
        el('email').value = remembered.email;
        el('rememberSponsor').checked = true;
    }
    updateQthControl();
    if (pendingAttempt) {
        const payload = JSON.parse(pendingAttempt.payload);
        fillForm(payload);
        review(payload);
    } else if (rejectedAttempt) {
        fillForm(JSON.parse(rejectedAttempt.payload));
    } else {
        const receipt = readStorage('sessionStorage', RECEIPT_KEY);
        if (receipt && typeof receipt.receipt === 'string' && typeof receipt.receivedAt === 'string') showReceipt(receipt);
    }
    (async function loadRoster() {
        for (const version of [2, 1]) {
            try {
                const response = await fetch('data/v' + version + '/roster.json');
                if (!response.ok) continue;
                const data = await response.json();
                if (String(data.schema_version) !== String(version) || !Array.isArray(data.members)) continue;
                members = data.members.filter(function (member) {
                    return Number.isSafeInteger(member.bkg_number) && member.bkg_number > 0
                        && typeof member.callsign === 'string' && /^[A-Z0-9]+$/.test(member.callsign)
                        && typeof member.name === 'string';
                });
                rosterLoaded = true;
                break;
            } catch (_) { /* The explicit officer-review path works offline. */ }
        }
        if (!pendingAttempt) { matchSponsor(); validateRecruit(); }
    }());
}());
