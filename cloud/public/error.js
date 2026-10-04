// The sign-in error page (error.html). A file, not inline, so the CSP in
// _headers can allow scripts from this origin only.

// Better Auth redirects here with ?error=<code> (onAPIError.errorURL).
const MESSAGES = {
  access_denied: 'Sign-in was cancelled.',
  state_mismatch: 'That sign-in attempt expired. Start a new one from Yap.',
  please_restart_the_process: 'That sign-in attempt expired. Start a new one from Yap.',
  account_not_linked:
    'That email already has a Yap account that uses a different sign-in method. Use that method, or sign in with an email code to prove the address is yours.',
  email_not_found: "Your sign-in provider didn't share an email address, which Yap needs. Try another method or an email code.",
  // Set by this Worker's sign-in entry point (src/index.ts).
  provider_unavailable: "That sign-in option isn't available right now. Try another one, or an email code.",
  rate_limited: 'Too many sign-in attempts. Wait a minute, then try again.',
  invalid_request: 'That sign-in link was incomplete. Start again from Yap.',
  // Set by Yap's own local listener (dev/portable builds), not Better Auth.
  handoff_failed: "Yap couldn't finish signing you in. It shows the reason in the app.",
};
const error = new URLSearchParams(location.search).get('error');
if (error) {
  document.getElementById('msg').textContent = MESSAGES[error.toLowerCase()] || 'Something went wrong while signing you in.';
  const code = document.getElementById('code');
  code.textContent = `Error code: ${error}`;
  code.hidden = false;
}
