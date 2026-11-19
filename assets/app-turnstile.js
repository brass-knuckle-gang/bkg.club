// Turnstile token page for the BKG iOS app (docs/APP-TURNSTILE.md).
// The app loads app-turnstile.html in a WKWebView; this script renders the
// same widget as recruit.html and hands tokens to the app. It never submits
// anything to the intake API.
(function () {
    'use strict';
    // Same sitekey and action as the recruit form (assets/recruit.js).
    const SITEKEY = '0x4AAAAAAFsHyM6nyvrtrADC';
    const ACTION = 'recruit-application';
    const SCRIPT_SRC = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';

    const status = document.getElementById('appTurnstileStatus');
    const handlers = window.webkit && window.webkit.messageHandlers;
    const handler = handlers && handlers.bkgTurnstile;
    if (!handler || typeof handler.postMessage !== 'function') {
        status.textContent = 'This page is for the BKG app.';
        return;
    }

    let widgetId = null;
    function post(message) {
        try { handler.postMessage(message); } catch (error) { /* app went away */ }
    }
    // Until Cloudflare's script loads, window.turnstile may be undefined.
    function turnstileApi() {
        const api = window.turnstile;
        return api && typeof api.render === 'function' ? api : null;
    }
    function render() {
        const api = turnstileApi();
        if (!api) return false;
        if (widgetId !== null) {
            api.reset(widgetId);
            return true;
        }
        widgetId = api.render('#appTurnstileWidget', {
            sitekey: SITEKEY, action: ACTION,
            theme: 'dark', 'refresh-expired': 'auto',
            callback: function (token) { post({ type: 'token', token: token }); },
            'error-callback': function () { post({ type: 'error' }); return true; },
            'expired-callback': function () { post({ type: 'expired' }); },
            'timeout-callback': function () { post({ type: 'expired' }); },
        });
        return true;
    }

    // The app calls this to throw away the current token and get a new one.
    window.bkgTurnstileReset = function () {
        return render();
    };

    status.textContent = 'Quick robot check…';
    const script = document.createElement('script');
    script.src = SCRIPT_SRC;
    script.async = true;
    script.addEventListener('load', function () {
        if (!render()) post({ type: 'error' });
    });
    script.addEventListener('error', function () { post({ type: 'error' }); });
    document.head.appendChild(script);
}());
