// The hand-back page (index.html): passes the one-time sign-in code from the
// server's cookie back to Yap. A file, not inline, so the CSP in _headers can
// allow scripts from this origin only.

// Set by the server's desktop-handoff plugin after a sign-in that Yap
// started (client id "yap-desktop"): base64url JSON {identifier, state}.
const COOKIE = 'better-auth.yap-desktop';
const SCHEME = 'com.contextmirror.yap';

const readCookie = () => {
  const hit = document.cookie.split('; ').find((c) => c.startsWith(COOKIE + '='));
  return hit ? hit.slice(COOKIE.length + 1) : null;
};

// Builds without the registered scheme (dev, portable) listen on a local
// port instead and append it to `state` as "-<port>" (src-tauri/src/auth.rs).
function loopbackPort(code) {
  try {
    const json = atob(code.replace(/-/g, '+').replace(/_/g, '/'));
    const m = /-(\d{2,5})$/.exec(JSON.parse(json).state || '');
    const port = m && Number(m[1]);
    return port > 0 && port < 65536 ? port : null;
  } catch {
    return null;
  }
}

function showHandoff(raw) {
  // One use only: clear it so a reload or a later visit can't replay it.
  document.cookie = `${COOKIE}=; expires=Thu, 01 Jan 1970 00:00:00 UTC; path=/`;
  // Plain base64url (no %-escapes or '=' padding) survives Windows' launch
  // of the app intact; Yap decodes either form.
  const code = decodeURIComponent(raw).replace(/=+$/, '');
  const port = loopbackPort(code);
  const href = port
    ? `http://127.0.0.1:${port}/callback?token=${encodeURIComponent(code)}`
    : `${SCHEME}://auth/callback#token=${code}`;
  document.getElementById('open').href = href;
  document.getElementById('code').textContent = code;
  document.getElementById('copy').onclick = async (e) => {
    await navigator.clipboard.writeText(code);
    e.target.textContent = 'Copied';
  };
  document.getElementById('handoff').hidden = false;
  fetch('/api/auth/get-session', { credentials: 'same-origin' })
    .then((r) => r.json())
    .then((d) => {
      if (d && d.user && d.user.email) {
        document.getElementById('who').textContent = `Signed in as ${d.user.email}. Switching you back to Yap…`;
      }
    })
    .catch(() => {})
    .finally(signOutBrowser);
  // Try once automatically; browsers may ask first or need the button.
  window.location.href = href;
}

// The provider sign-in also left this browser signed in, but only Yap needs a
// session (it gets its own when it redeems the code), so end the browser's:
// nothing usable stays behind on a shared computer. keepalive lets it finish
// even when the loopback hand-back navigates away; the "#done" page that
// follows tries again.
function signOutBrowser() {
  fetch('/api/auth/sign-out', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'content-type': 'application/json' },
    body: '{}',
    keepalive: true,
  }).catch(() => {});
}

// The cookie arrives with the sign-in redirect; poll briefly like the
// plugin's own helper in case it lands a beat after the page.
const started = Date.now();
(function wait() {
  if (location.hash === '#done') {
    document.getElementById('done').hidden = false;
    signOutBrowser();
    return;
  }
  const raw = readCookie();
  if (raw) return showHandoff(raw);
  if (Date.now() - started < 1500) return setTimeout(wait, 100);
  document.getElementById('landing').hidden = false;
})();
