# App Turnstile page

`https://www.bkg.club/app-turnstile.html` lets the BKG iOS app get a Cloudflare
Turnstile token for Jump In (recruit) submissions. The app loads it in a
`WKWebView`. It renders the same widget as `recruit.html` (same sitekey, action
`recruit-application`), so the token is valid for the intake API. The page
never submits anything. It is noindex and unlinked. In a normal browser it
only says "This page is for the BKG app".

## Messages to the app

Posted through `window.webkit.messageHandlers.bkgTurnstile.postMessage(...)`:

```json
{"type":"token","token":"<turnstile token>"}
{"type":"error"}
{"type":"expired"}
```

`expired` also covers a challenge that timed out. The widget refreshes expired
tokens by itself, so more than one `token` message can arrive: keep the latest.

## Swift side

1. Add a `WKScriptMessageHandler` named `bkgTurnstile` to the web view's
   `userContentController` before loading the page.
2. On `token`, store it as the `turnstileToken` for the next POST. A token is
   single-use and lasts five minutes.
3. On `error` or `expired`, drop any stored token and call
   `webView.evaluateJavaScript("bkgTurnstileReset()")` (also do this after each
   POST, or when the intake API answers `422 invalid_turnstile`).
4. POST per bkg-automation `docs/direct-intake-contract.md` (one key per application).

## Open question for bkg-automation owners

The intake Worker only accepts an exact allowed `Origin`, and Siteverify checks
the token's hostname. The token's hostname is `www.bkg.club`, but the app's
native POST has no browser Origin, so it would have to send
`Origin: https://www.bkg.club` itself. The owners must confirm that is acceptable.
