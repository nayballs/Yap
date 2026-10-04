// Sign in with your phone (device.html). Yap on a PC shows a QR code for
// /device?user_code=…; here the person signs in, any way the service offers,
// and approves that code while Yap polls /api/auth/device/token
// (src-tauri/src/auth.rs). A file, not inline, so the CSP in _headers can
// allow scripts from this origin only; server text only ever goes in via
// textContent.

const $ = (id) => document.getElementById(id);
const VIEWS = ['loading', 'enter', 'signin', 'confirm', 'done', 'problem'];
const show = (view) => VIEWS.forEach((v) => ($(v).hidden = v !== view));

// Codes are 8 letters (src/auth.ts phoneUserCode), shown as WDJB-MJHT. The
// server ignores case and separators too.
const normalize = (raw) => (raw || '').toUpperCase().replace(/[^A-Z0-9]/g, '');
const pretty = (code) => (code.length === 8 ? `${code.slice(0, 4)}-${code.slice(4)}` : code);

const params = new URLSearchParams(location.search);
let userCode = normalize(params.get('user_code'));
// A provider sign-in that failed comes back with ?error=… (errorCallbackURL);
// drop it from the address so a reload doesn't show it again.
let providerError = params.get('error');
if (providerError) history.replaceState(null, '', here());

/** Whether this browser holds a session (signed in on this page). */
let signedIn = false;
let methods = null;
let email = '';
let resendTimer = null;

function here() {
  return userCode ? `/device?user_code=${encodeURIComponent(userCode)}` : '/device';
}

function setError(id, message) {
  $(id).textContent = message || '';
  $(id).hidden = !message;
}

/** Disables the current view's buttons and fields while a request runs. */
function setBusy(view, busy) {
  for (const el of $(view).querySelectorAll('button, input')) el.disabled = busy;
}

/** Calls Better Auth: JSON in and out. `auth: false` leaves cookies out. */
async function call(path, { method = 'GET', body, auth = true } = {}) {
  const init = { method, credentials: auth ? 'same-origin' : 'omit', headers: {} };
  if (method === 'POST') {
    init.headers['content-type'] = 'application/json';
    init.body = JSON.stringify(body ?? {});
  }
  try {
    const res = await fetch(`/api/auth${path}`, init);
    const data = await res.json().catch(() => null);
    const wait = Number(res.headers.get('retry-after') || res.headers.get('x-retry-after')) || 0;
    return { ok: res.ok, status: res.status, data, wait };
  } catch {
    return { ok: false, status: 0, data: null, wait: 0 };
  }
}

const waitText = (secs) =>
  secs > 1 && secs < 90 ? `${secs} seconds` : secs >= 90 ? `${Math.ceil(secs / 60)} minutes` : 'a minute';
const tooMany = (secs) => `Too many tries. Wait ${waitText(secs)}, then try again.`;

// Only Yap needs a session (it gets its own from /device/token), so this
// browser's ends with the visit: nothing usable stays behind on the phone.
// keepalive lets it finish even if the page is closed straight away.
function endBrowserSession() {
  if (!signedIn) return Promise.resolve();
  signedIn = false;
  return fetch('/api/auth/sign-out', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'content-type': 'application/json' },
    body: '{}',
    keepalive: true,
  }).catch(() => {});
}

// ---- the code ----

const NEW_CODE = 'Get a new one in Yap on your PC: Settings → Account → Sign in with your phone.';
const PROBLEMS = {
  expired: ['This code has expired', `Codes last 10 minutes. ${NEW_CODE}`],
  used: ['This code was already used', `Each code works once. If Yap on your PC isn't signed in yet, start again there: Settings → Account → Sign in with your phone.`],
  unknown: ["That code isn't right", `Check it against the code Yap shows on your PC, or get a new one there: Settings → Account → Sign in with your phone.`],
  claimed: ['This code belongs to another account', `It was opened with a different account first. ${NEW_CODE}`],
  busy: ['Too many tries', 'Wait a few minutes, then try again.'],
  offline: ["Couldn't reach Yap's account service", 'Check your connection, then try again.'],
  error: ['Something went wrong', 'Try again in a moment.'],
};

function problem(kind, wait = 0) {
  // Worth retrying as is, or a code issue (type another one).
  const retry = kind === 'busy' || kind === 'offline' || kind === 'error';
  // A dead code needs no session here; a hiccup keeps it for "Try again".
  if (!retry) endBrowserSession();
  const [title, text] = PROBLEMS[kind];
  $('problem-title').textContent = title;
  $('problem-text').textContent = kind === 'busy' && wait > 1 ? tooMany(wait) : text;
  $('problem-action').textContent = retry ? 'Try again' : 'Enter a different code';
  $('problem-action').onclick = retry ? start : () => showEnter();
  show('problem');
}

/** Problem for a failed (or not-pending) code lookup or approve/deny. */
function codeProblem(r) {
  if (r.ok) return problem('used'); // looked up fine, but approved or denied already
  if (r.status === 429) return problem('busy', r.wait);
  if (r.status === 0) return problem('offline');
  const error = r.data?.error;
  if (error === 'expired_token') return problem('expired');
  if (error === 'invalid_request') {
    // The plugin's own descriptions are the only way to tell these apart.
    const description = r.data?.error_description || '';
    if (/already processed/i.test(description)) return problem('used');
    if (/not been claimed/i.test(description)) return problem('error'); // lost a race: try again
    return problem('unknown');
  }
  problem('error');
}

/** Is the code good? Looked up without this browser's session (cookies
 *  left out): a signed-in lookup claims the code for that account, which
 *  should happen only when they tap Approve or Deny. */
async function checkCode() {
  const r = await call(`/device?user_code=${encodeURIComponent(userCode)}`, { auth: false });
  return r.ok && r.data?.status === 'pending' ? null : r;
}

// ---- views ----

async function start() {
  show('loading');
  if (userCode.length !== 8) return showEnter(userCode ? 'That code looks incomplete. It has 8 letters, like WDJB-MJHT.' : '');
  const [bad, session] = await Promise.all([checkCode(), call('/get-session')]);
  signedIn = Boolean(session.ok && session.data?.user);
  if (bad) return codeProblem(bad);
  if (signedIn) return showConfirm(session.data.user);
  const message = providerError ? signInErrorText(providerError) : '';
  providerError = null; // shown once
  showSignIn(message);
}

function showEnter(message = '') {
  endBrowserSession();
  $('enter-code').value = userCode ? pretty(userCode) : '';
  setError('enter-error', message);
  setBusy('enter', false);
  show('enter');
  $('enter-code').focus();
}

$('enter-form').onsubmit = (e) => {
  e.preventDefault();
  const code = normalize($('enter-code').value);
  if (code.length !== 8) return setError('enter-error', 'Enter all 8 letters of the code, like WDJB-MJHT.');
  userCode = code;
  history.replaceState(null, '', here());
  start();
};

const LABELS = { google: 'Google', github: 'GitHub', discord: 'Discord' };
const labelFor = (id) => LABELS[id] || id.charAt(0).toUpperCase() + id.slice(1);

async function showSignIn(message = '') {
  $('signin-code').textContent = pretty(userCode);
  setError('signin-error', message);
  if (!methods) {
    const r = await fetch('/api/providers')
      .then((res) => (res.ok ? res.json() : null))
      .catch(() => null);
    methods = { providers: Array.isArray(r?.providers) ? r.providers : [], email: Boolean(r?.email) };
    const list = $('providers');
    for (const id of methods.providers) {
      // Known providers have a button with their logo; anything else gets a plain one.
      let button = list.querySelector(`[data-provider="${CSS.escape(id)}"]`);
      if (!button) {
        button = document.createElement('button');
        button.type = 'button';
        button.className = 'provider';
        button.dataset.provider = id;
        button.textContent = `Continue with ${labelFor(id)}`;
      }
      button.hidden = false;
      button.onclick = () => startProvider(id);
      list.append(button); // in the service's order
    }
    $('or').hidden = !(methods.providers.length && methods.email);
    $('email-form').hidden = !methods.email;
    $('no-methods').hidden = methods.providers.length > 0 || methods.email;
  }
  showEmailStep('choose');
  setBusy('signin', false);
  show('signin');
}

function showEmailStep(step) {
  $('choose').hidden = step !== 'choose';
  $('otp-form').hidden = step !== 'code';
}

const SIGN_IN_ERRORS = {
  access_denied: 'Sign-in was cancelled. Pick a way to sign in to carry on.',
  account_not_linked:
    'That email already has a Yap account that uses a different sign-in method. Use that one, or an email code.',
  email_not_found: "That sign-in didn't share an email address, which Yap needs. Try another way, or an email code.",
  unable_to_create_user:
    'Yap needs a verified email address. If you used Discord, verify your email there first (User Settings → My Account), or use an email code.',
  state_mismatch: 'That sign-in took too long. Try again.',
  please_restart_the_process: 'That sign-in took too long. Try again.',
};
const signInErrorText = (code) =>
  SIGN_IN_ERRORS[code.toLowerCase()] || "That sign-in didn't finish. Try again, or pick another way.";

async function startProvider(provider) {
  setBusy('signin', true);
  setError('signin-error', '');
  // Back here either way: signed in, or with ?error=… to explain.
  const back = here();
  const r = await call('/sign-in/social', {
    method: 'POST',
    body: { provider, callbackURL: back, errorCallbackURL: back },
  });
  if (r.ok && r.data?.url) {
    location.assign(r.data.url);
    return;
  }
  setBusy('signin', false);
  setError('signin-error', r.status === 429 ? tooMany(r.wait) : "That sign-in option isn't available right now. Try another one.");
}

// ---- email code ----

const EMAIL_ERRORS = {
  INVALID_EMAIL: 'Enter a valid email address.',
  INVALID_OTP: "That code isn't right. Check it and try again.",
  OTP_EXPIRED: 'That code has expired. Send a new one.',
  TOO_MANY_ATTEMPTS: 'Too many wrong codes. Send a new one.',
};
const emailError = (r) =>
  r.status === 429
    ? tooMany(r.wait)
    : r.status === 0
      ? "Couldn't reach Yap's account service. Check your connection."
      : EMAIL_ERRORS[r.data?.code] || 'Something went wrong. Try again.';

async function sendEmailCode() {
  const typed = $('email').value.trim().toLowerCase();
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(typed)) return setError('signin-error', 'Enter a valid email address.');
  setBusy('signin', true);
  setError('signin-error', '');
  const r = await call('/email-otp/send-verification-otp', { method: 'POST', body: { email: typed, type: 'sign-in' } });
  setBusy('signin', false);
  if (!r.ok) return setError('signin-error', emailError(r));
  email = typed;
  $('otp-email').textContent = email;
  $('otp').value = '';
  showEmailStep('code');
  startResendTimer();
  $('otp').focus();
}

function startResendTimer() {
  // The service sends one address a code every 30 s at most (src/throttle.ts).
  let left = 30;
  const tick = () => {
    $('otp-resend').disabled = left > 0;
    $('otp-resend').textContent = left > 0 ? `Resend in ${left}s` : 'Resend code';
    left -= 1;
  };
  clearInterval(resendTimer);
  tick();
  resendTimer = setInterval(() => {
    tick();
    if (left < 0) clearInterval(resendTimer);
  }, 1000);
}

async function verifyEmailCode() {
  const otp = $('otp').value.replace(/\D/g, '');
  if (otp.length !== 6) return setError('signin-error', 'Enter the 6-digit code from the email.');
  setBusy('signin', true);
  setError('signin-error', '');
  const r = await call('/sign-in/email-otp', { method: 'POST', body: { email, otp } });
  if (!r.ok) {
    setBusy('signin', false);
    // The resend countdown owns that button's state.
    $('otp-resend').disabled = $('otp-resend').textContent !== 'Resend code';
    return setError('signin-error', emailError(r));
  }
  clearInterval(resendTimer);
  signedIn = true;
  showConfirm(r.data?.user);
}

$('email-form').onsubmit = (e) => {
  e.preventDefault();
  sendEmailCode();
};
$('otp-form').onsubmit = (e) => {
  e.preventDefault();
  verifyEmailCode();
};
$('otp').oninput = (e) => {
  e.target.value = e.target.value.replace(/\D/g, '').slice(0, 6);
  if (e.target.value.length === 6) verifyEmailCode();
};
$('otp-back').onclick = () => {
  clearInterval(resendTimer);
  setError('signin-error', '');
  showEmailStep('choose');
};
$('otp-resend').onclick = sendEmailCode;

// ---- approve / deny ----

function showConfirm(user) {
  $('who').textContent = user?.email || 'your account';
  $('confirm-code').textContent = pretty(userCode);
  setError('confirm-error', '');
  setBusy('confirm', false);
  show('confirm');
}

async function decide(action) {
  setBusy('confirm', true);
  setError('confirm-error', '');
  const offline = () => {
    setBusy('confirm', false);
    setError('confirm-error', "Couldn't reach Yap's account service. Check your connection and try again.");
  };
  // Only the account that looked the code up may approve or deny it, so
  // look it up now, signed in: that claims it for this account.
  const claim = await call(`/device?user_code=${encodeURIComponent(userCode)}`);
  if (claim.status === 0) return offline();
  if (!(claim.ok && claim.data?.status === 'pending')) return codeProblem(claim);
  const r = await call(`/device/${action}`, { method: 'POST', body: { userCode } });
  if (!r.ok) {
    if (r.status === 401) {
      // This browser's sign-in ended in the meantime.
      signedIn = false;
      return showSignIn('Your sign-in here ended. Sign in again to carry on.');
    }
    if (r.status === 403) return problem('claimed');
    if (r.status === 0) return offline();
    return codeProblem(r);
  }
  endBrowserSession();
  const approved = action === 'approve';
  $('done-mark').textContent = approved ? '✓' : '✕';
  $('done-mark').classList.toggle('no', !approved);
  $('done-title').textContent = approved ? 'Done. Go back to your PC' : 'Sign-in denied';
  $('done-text').textContent = approved
    ? 'Yap finishes signing in by itself in a few seconds. You can close this page.'
    : "Yap on that PC won't be signed in. You can close this page.";
  show('done');
}

$('approve').onclick = () => decide('approve');
$('deny').onclick = () => decide('deny');
$('switch').onclick = async () => {
  setBusy('confirm', true);
  await endBrowserSession();
  showSignIn();
};

start();
