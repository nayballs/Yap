// Hold a webview's requests and answer them from the test: CDP's `Fetch`
// domain, on a CDP session of the test's own.
//
// Not Playwright's `page.route`: on Yap's WebView2 pages over CDP it
// intercepted nothing at all, for the whole life of a test Yap, in 3 of 4
// fresh ones (19 Nov 2026: a route on the main window's ControlPanel chunk,
// probed with the page's own `fetch`), while `Fetch.enable` on a session of
// its own held every request every time. page-load-retry.spec.js and
// never-blank.spec.js refuse and replace the app's own files with this.

/**
 * Hold `page`'s requests whose URL matches one of `patterns` (CDP's
 * wildcards: `*` any run of characters, `?` one) and answer each with what
 * `answer({ url, resourceType })` returns: `'continue'` (the real file),
 * `'refuse'` (connection refused, as a dev server that's down) or
 * `{ body, contentType }` (served instead). Returns `{ stop }`.
 */
export async function intercept(page, patterns, answer) {
  const cdp = await page.context().newCDPSession(page);
  cdp.on('Fetch.requestPaused', async (e) => {
    const id = { requestId: e.requestId };
    let what = 'continue';
    try {
      what = await answer({ url: e.request.url, resourceType: e.resourceType });
    } catch {
      /* the real file */
    }
    if (what === 'refuse') {
      await cdp.send('Fetch.failRequest', { ...id, errorReason: 'ConnectionRefused' }).catch(() => {});
    } else if (what && typeof what === 'object') {
      await cdp
        .send('Fetch.fulfillRequest', {
          ...id,
          responseCode: 200,
          responseHeaders: [{ name: 'Content-Type', value: what.contentType ?? 'text/javascript' }],
          body: Buffer.from(what.body).toString('base64'),
        })
        .catch(() => {});
    } else {
      await cdp.send('Fetch.continueRequest', id).catch(() => {});
    }
  });
  await cdp.send('Fetch.enable', { patterns: patterns.map((urlPattern) => ({ urlPattern, requestStage: 'Request' })) });
  return {
    async stop() {
      await cdp.send('Fetch.disable').catch(() => {});
      await cdp.detach().catch(() => {});
    },
  };
}
