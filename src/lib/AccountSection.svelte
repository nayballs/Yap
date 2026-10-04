<script>
  // Settings → Account. Sign-in is optional: Google / Microsoft / GitHub run
  // in the system browser and hand back to Yap (auth.rs), email works with a
  // 6-digit code typed here. Signed in, it shows the profile, sign-out and
  // account deletion (which asks for a fresh sign-in when the session is
  // older than a day — the server's rule).
  // "Sign in with your phone" shows a QR code: any of those methods on the
  // phone, then approve the code there (auth_device_*, RFC 8628).
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Button from './ui/Button.svelte';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import { account, displayName, initAccount, initials, PROVIDER_LABELS } from './account.svelte.js';
  import { openExternalLink } from './externalLinks.js';

  const RESEND_SECONDS = 30;

  let status = $derived(account.status);

  // Sign-in form state.
  let email = $state('');
  let emailStep = $state('enter'); // 'enter' | 'code'
  let code = $state('');
  let pasted = $state('');
  let busy = $state(''); // which action is running ('' = idle)
  let error = $state('');
  let resendIn = $state(0);
  let resendTimer = null;

  // Delete flow: null → 'confirm' → (server wants a fresh sign-in) 'reauth'.
  let deleteStep = $state(null);
  let reauthFor = $state(null); // { userId, signedInAt } when re-auth started
  let notice = $state('');

  let imgFailed = $state(false);
  let showSignIn = $derived(!!status && (!status.signedIn || deleteStep === 'reauth'));

  // What the account service offers. Yap only asks when this page opens, so
  // show no buttons until it answers; if it can't be reached, offer everything
  // and let the service explain if something's off.
  let methodsChecked = $state(false);
  let methods = $derived(
    status?.methods ?? (methodsChecked ? { providers: ['google', 'microsoft', 'github'], email: true } : null)
  );
  let offered = $derived(methods?.providers ?? []);
  let emailOffered = $derived(methods?.email ?? false);

  onMount(() => {
    initAccount();
    invoke('auth_check_methods')
      .catch(() => {})
      .finally(() => (methodsChecked = true));
    return () => clearInterval(resendTimer);
  });

  // A new avatar URL gets a fresh chance to load.
  $effect(() => {
    status?.user?.image;
    imgFailed = false;
  });

  // Re-auth for deletion: once a NEW session for the same account appears,
  // carry on with the delete; a different account deletes nothing.
  $effect(() => {
    if (deleteStep !== 'reauth' || !reauthFor || !status?.signedIn) return;
    if (status.signedInAt === reauthFor.signedInAt) return;
    if (status.user?.id === reauthFor.userId) {
      reauthFor = null;
      deleteAccount();
    } else {
      reauthFor = null;
      deleteStep = null;
      notice = `You signed in as ${status.user?.email}, a different account, so nothing was deleted.`;
    }
  });

  async function run(name, fn) {
    busy = name;
    error = '';
    try {
      await fn();
    } catch (e) {
      error = String(e);
    } finally {
      busy = '';
    }
  }

  function startProvider(provider) {
    pasted = '';
    run(provider, () => invoke('auth_start', { provider }));
  }

  function cancelBrowser() {
    invoke('auth_cancel');
    error = '';
  }

  // Sign in with your phone: status.device holds the QR code and code while
  // Yap waits. It clears once the phone approves (signed in) or the attempt
  // ends (denied, expired: yap-auth-error says why).
  let device = $derived(status?.device ?? null);
  let nowSecs = $state(Math.floor(Date.now() / 1000));
  $effect(() => {
    if (!device) return;
    const tick = () => (nowSecs = Math.floor(Date.now() / 1000));
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  });
  let deviceLeft = $derived(device ? Math.max(0, device.expiresAt - nowSecs) : 0);
  // Whole pixels per module (plus the 4-module quiet zone each side).
  let qrPx = $derived(device ? (device.qr.size + 8) * 5 : 0);
  const clock = (secs) => `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}`;
  const withoutScheme = (url) => url.replace(/^https?:\/\//, '');

  function startPhone() {
    run('phone', () => invoke('auth_device_start'));
  }

  function cancelPhone() {
    invoke('auth_device_cancel');
    error = '';
  }

  function submitPasted() {
    if (!pasted.trim()) return;
    run('paste', () => invoke('auth_submit_code', { code: pasted }));
  }

  function startResendTimer() {
    resendIn = RESEND_SECONDS;
    clearInterval(resendTimer);
    resendTimer = setInterval(() => {
      resendIn = Math.max(0, resendIn - 1);
      if (resendIn === 0) clearInterval(resendTimer);
    }, 1000);
  }

  function sendCode() {
    run('email', async () => {
      await invoke('auth_email_send', { email });
      emailStep = 'code';
      code = '';
      startResendTimer();
    });
  }

  function verifyCode() {
    if (busy) return;
    run('verify', async () => {
      await invoke('auth_email_verify', { email, code });
      emailStep = 'enter';
      code = '';
    });
  }

  function onCodeInput(e) {
    code = e.currentTarget.value.replace(/\D/g, '').slice(0, 6);
    e.currentTarget.value = code;
    if (code.length === 6) verifyCode();
  }

  function useDifferentEmail() {
    emailStep = 'enter';
    code = '';
    error = '';
  }

  function signOut() {
    notice = '';
    run('signout', () => invoke('auth_sign_out'));
  }

  function deleteAccount() {
    notice = '';
    run('delete', async () => {
      try {
        await invoke('auth_delete_account');
        deleteStep = null;
        notice = 'Your Yap account has been deleted. Dictation on this PC carries on as before.';
      } catch (e) {
        if (String(e) !== 'reauth') throw e;
        // The session is too old to delete with: sign in again first.
        reauthFor = { userId: status.user.id, signedInAt: status.signedInAt };
        email = status.user.email;
        emailStep = 'enter';
        deleteStep = 'reauth';
      }
    });
  }

  function cancelDelete() {
    deleteStep = null;
    reauthFor = null;
    error = '';
    if (status?.pending) invoke('auth_cancel');
    if (status?.device) invoke('auth_device_cancel');
  }

  const openPage = (path) => openExternalLink(`${status?.serviceUrl ?? 'https://auth.contextmirror.com'}${path}`);
</script>

{#snippet providerIcon(id)}
  {#if id === 'google'}
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
      <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.27-4.74 3.27-8.1z" />
      <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84A11 11 0 0 0 12 23z" />
      <path fill="#FBBC05" d="M5.84 14.1a6.6 6.6 0 0 1 0-4.2V7.06H2.18a11 11 0 0 0 0 9.88l3.66-2.84z" />
      <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1A11 11 0 0 0 2.18 7.06l3.66 2.84C6.71 7.3 9.14 5.38 12 5.38z" />
    </svg>
  {:else if id === 'microsoft'}
    <svg viewBox="0 0 21 21" width="17" height="17" aria-hidden="true">
      <rect x="1" y="1" width="9" height="9" fill="#F25022" />
      <rect x="11" y="1" width="9" height="9" fill="#7FBA00" />
      <rect x="1" y="11" width="9" height="9" fill="#00A4EF" />
      <rect x="11" y="11" width="9" height="9" fill="#FFB900" />
    </svg>
  {:else if id === 'github'}
    <svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true">
      <path
        fill="currentColor"
        d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z"
      />
    </svg>
  {/if}
{/snippet}

{#snippet phoneIcon()}
  <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="6.5" y="2.5" width="11" height="19" rx="2.5" />
    <path d="M10.5 18.25h3" />
  </svg>
{/snippet}

{#snippet signInBody()}
  {#if status.pending}
    <div class="waiting">
      <span class="spinner" aria-hidden="true"></span>
      <div>
        <p class="lead">Finish signing in with {PROVIDER_LABELS[status.pending]} in your browser.</p>
        <p class="muted">A browser tab has opened. Yap picks the sign-in up as soon as you're done there.</p>
      </div>
    </div>
    <div class="paste">
      <label class="small muted" for="acct-paste">Signed in but Yap didn't notice? Paste the code the page shows:</label>
      <div class="inline">
        <input
          id="acct-paste"
          class="field mono"
          bind:value={pasted}
          placeholder="Paste code"
          autocomplete="off"
          spellcheck="false"
          onkeydown={(e) => e.key === 'Enter' && submitPasted()}
        />
        <Button variant="secondary" disabled={!pasted.trim() || busy === 'paste'} onclick={submitPasted}>Continue</Button>
      </div>
    </div>
    <div class="actions">
      <Button variant="ghost" size="sm" onclick={() => startProvider(status.pending)}>Open the browser again</Button>
      <Button variant="ghost" size="sm" onclick={cancelBrowser}>Cancel</Button>
    </div>
  {:else if device}
    <div class="phone">
      <div class="qr">
        <svg
          width={qrPx}
          height={qrPx}
          viewBox="-4 -4 {device.qr.size + 8} {device.qr.size + 8}"
          shape-rendering="crispEdges"
          role="img"
          aria-label="QR code that opens Yap's sign-in page on your phone"
        >
          <rect x="-4" y="-4" width={device.qr.size + 8} height={device.qr.size + 8} fill="#fff" />
          <path d={device.qr.path} fill="#1f1d17" />
        </svg>
      </div>
      <div class="phone-steps">
        <p class="lead">Scan with your phone's camera</p>
        <p class="muted">Sign in on the page that opens, check it shows this code, then approve.</p>
        <p class="usercode">{device.userCode}</p>
        <p class="small muted">
          No camera? On your phone, go to <strong>{withoutScheme(device.verificationUri)}</strong> and enter the code.
        </p>
        <div class="waiting phone-wait">
          <span class="spinner" aria-hidden="true"></span>
          <p class="small muted">
            {deviceLeft > 0 ? `Waiting for your phone. The code expires in ${clock(deviceLeft)}.` : 'The code has expired.'}
          </p>
        </div>
        <div class="actions">
          <Button variant="ghost" size="sm" onclick={cancelPhone}>Cancel</Button>
        </div>
      </div>
    </div>
  {:else if emailStep === 'code'}
    <p class="lead">Check your email</p>
    <p class="muted">We sent a 6-digit code to <strong>{email.trim()}</strong>. It expires in 10 minutes.</p>
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="field otp"
      value={code}
      oninput={onCodeInput}
      onkeydown={(e) => e.key === 'Enter' && code.length === 6 && verifyCode()}
      inputmode="numeric"
      autocomplete="one-time-code"
      maxlength="6"
      placeholder="000000"
      aria-label="6-digit code"
      autofocus
    />
    <div class="actions">
      <Button disabled={code.length !== 6 || busy === 'verify'} onclick={verifyCode}>
        {busy === 'verify' ? 'Checking…' : 'Verify'}
      </Button>
      <Button variant="ghost" size="sm" onclick={useDifferentEmail}>Use a different email</Button>
      <Button variant="ghost" size="sm" disabled={resendIn > 0 || busy === 'email'} onclick={sendCode}>
        {resendIn > 0 ? `Resend in ${resendIn}s` : 'Resend code'}
      </Button>
    </div>
  {:else}
    {#if offered.length}
      <div class="providers">
        {#each offered as id (id)}
          <button class="provider" type="button" disabled={!!busy} onclick={() => startProvider(id)}>
            <span class="picon">{@render providerIcon(id)}</span>
            Continue with {PROVIDER_LABELS[id]}
          </button>
        {/each}
      </div>
    {/if}
    {#if offered.length || emailOffered}
      <!-- Any of those methods, on the phone (auth_device_*). -->
      <div class="providers phone-option">
        <button class="provider" type="button" disabled={!!busy} onclick={startPhone}>
          <span class="picon">{@render phoneIcon()}</span>
          {busy === 'phone' ? 'Getting a code…' : 'Sign in with your phone'}
        </button>
      </div>
    {/if}
    {#if offered.length && emailOffered}
      <div class="or"><span>or</span></div>
    {/if}
    {#if methods && !offered.length && !emailOffered}
      <p class="muted">Signing in isn't available right now. Please try again later.</p>
    {/if}
    {#if emailOffered}
      <form
        class="inline"
        onsubmit={(e) => {
          e.preventDefault();
          sendCode();
        }}
      >
        <input
          class="field"
          type="email"
          bind:value={email}
          placeholder="you@example.com"
          autocomplete="email"
          aria-label="Email address"
        />
        <Button type="submit" variant="secondary" disabled={!email.trim() || !!busy}>
          {busy === 'email' ? 'Sending…' : 'Email me a code'}
        </Button>
      </form>
    {/if}
    {#if !status.signedIn}
      <p class="fine">
        By continuing you agree to Yap's
        <a href="#terms" onclick={(e) => (e.preventDefault(), openPage('/terms'))}>Terms</a> and
        <a href="#privacy" onclick={(e) => (e.preventDefault(), openPage('/privacy'))}>Privacy Policy</a>.
      </p>
    {/if}
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
{/snippet}

{#if !status}
  <p class="muted">Loading…</p>
{:else}
  {#if notice}
    <div class="notice" role="status">{notice}</div>
  {/if}

  {#if status.signedIn}
    <div class="profile">
      {#if status.user?.image && !imgFailed}
        <img class="avatar" src={status.user.image} alt="" referrerpolicy="no-referrer" onerror={() => (imgFailed = true)} />
      {:else}
        <span class="avatar initials" aria-hidden="true">{initials(status.user)}</span>
      {/if}
      <div class="who">
        <h3>{displayName(status.user)}</h3>
        {#if status.user?.name?.trim()}<p class="muted">{status.user.email}</p>{/if}
        <p class="small muted">
          {#if status.providers.length}
            Signs in with {status.providers.map((p) => PROVIDER_LABELS[p] ?? p).join(', ')} or an email code
          {:else}
            Signs in with an email code
          {/if}
        </p>
        {#if status.offline}
          <p class="small offline">Offline: showing your saved sign-in.</p>
        {/if}
      </div>
      <Button variant="secondary" disabled={busy === 'signout'} onclick={signOut}>Sign out</Button>
    </div>
  {/if}

  {#if showSignIn}
    <div class="card hero">
      <h3 class="title">{status.signedIn ? "Confirm it's you" : 'Sign in to Yap'}</h3>
      <p class="muted intro">
        {#if status.signedIn}
          For your security, sign in again as <strong>{status.user?.email}</strong> to delete your account.
        {:else}
          Optional. Your voice, transcripts and notes stay on this PC whether you sign in or not.
        {/if}
      </p>
      {@render signInBody()}
      {#if status.signedIn}
        <div class="actions"><Button variant="ghost" size="sm" onclick={cancelDelete}>Keep my account</Button></div>
      {/if}
    </div>
  {/if}

  <Group title={status.signedIn ? 'Coming to your account' : 'What an account will add'}>
    <Row label="Hosted Pro cleanup" desc="A stronger cloud cleanup model when you want it. Local stays the default.">
      {#snippet children()}<span class="soon-tag">Planned</span>{/snippet}
    </Row>
    <Row label="Settings sync" desc="Carry your settings and dictionary across your PCs.">
      {#snippet children()}<span class="soon-tag">Planned</span>{/snippet}
    </Row>
  </Group>

  {#if status.signedIn && deleteStep !== 'reauth'}
    <Group title="Delete account">
      <div class="danger">
        {#if deleteStep === 'confirm'}
          <p><strong>Delete your Yap account?</strong> This permanently removes your account and signs you out everywhere. It can't be undone. Dictation, history and notes on this PC aren't affected.</p>
          <div class="actions">
            <Button variant="danger" disabled={busy === 'delete'} onclick={deleteAccount}>
              {busy === 'delete' ? 'Deleting…' : 'Delete permanently'}
            </Button>
            <Button variant="ghost" onclick={cancelDelete}>Cancel</Button>
          </div>
          {#if error}<p class="error" role="alert">{error}</p>{/if}
        {:else}
          <p class="muted">Permanently deletes your account and signs you out everywhere. Your data on this PC stays put.</p>
          <Button variant="secondary" onclick={() => ((deleteStep = 'confirm'), (error = ''), (notice = ''))}>Delete account…</Button>
        {/if}
      </div>
    </Group>
  {/if}

  <p class="footer-links">
    <a href="#privacy" onclick={(e) => (e.preventDefault(), openPage('/privacy'))}>Privacy</a>
    <span aria-hidden="true">·</span>
    <a href="#terms" onclick={(e) => (e.preventDefault(), openPage('/terms'))}>Terms</a>
  </p>
{/if}

<style>
  .muted {
    color: var(--yap-muted-70);
  }
  .small {
    font-size: 11.5px;
  }
  p {
    margin: 0;
    line-height: 1.55;
  }

  .card {
    padding: 22px;
    margin-bottom: 22px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .hero {
    border-color: var(--yap-primary-line);
    background: linear-gradient(180deg, var(--yap-primary-wash), var(--yap-s2) 60%);
  }
  .title {
    margin: 0 0 4px;
    font-family: var(--yap-font-display);
    font-size: 24px;
    font-weight: 500;
    color: var(--yap-fg);
  }
  .intro {
    font-size: 12.5px;
    margin-bottom: 18px;
    max-width: 52ch;
  }
  .lead {
    font-size: 13.5px;
    font-weight: 600;
    color: var(--yap-fg);
    margin-bottom: 2px;
  }

  .providers {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 340px;
  }
  .provider {
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    position: relative;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: #fff;
    color: #1f1f1f;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color var(--yap-dur) ease, background var(--yap-dur) ease;
  }
  .provider:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    background: #fdfcfa;
  }
  .provider:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .picon {
    position: absolute;
    left: 14px;
    display: grid;
    place-items: center;
    color: #181717;
  }

  .or {
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 340px;
    margin: 14px 0;
    color: var(--yap-muted-55);
    font-size: 11.5px;
  }
  .or::before,
  .or::after {
    content: '';
    flex: 1;
    height: 1px;
    background: var(--yap-border);
  }

  .inline {
    display: flex;
    gap: 8px;
    max-width: 420px;
  }
  .field {
    flex: 1;
    min-width: 0;
    height: 34px;
    box-sizing: border-box;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    padding: 0 12px;
    font: inherit;
    font-size: 12.5px;
  }
  .field:focus {
    outline: none;
    border-color: var(--yap-primary);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .mono {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 11.5px;
  }
  .otp {
    display: block;
    flex: none;
    width: 190px;
    height: 46px;
    margin: 14px 0 12px;
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 24px;
    letter-spacing: 0.32em;
    text-align: center;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
  }
  .fine {
    margin-top: 14px;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
  .fine a,
  .footer-links a {
    color: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .error {
    margin-top: 12px;
    font-size: 12px;
    color: var(--yap-danger);
  }

  .waiting {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .spinner {
    width: 16px;
    height: 16px;
    flex: 0 0 16px;
    margin-top: 2px;
    border-radius: 50%;
    border: 2px solid var(--yap-primary-tint);
    border-top-color: var(--yap-primary);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .paste {
    margin-top: 18px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  /* Sign in with your phone */
  .phone-option {
    margin-top: 8px;
  }
  .phone {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 18px 24px;
  }
  .qr {
    flex: none;
    line-height: 0;
    overflow: hidden;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: #fff;
    box-shadow: var(--yap-shadow-sm);
  }
  .qr svg {
    display: block;
  }
  .phone-steps {
    flex: 1;
    min-width: 230px;
  }
  .usercode {
    margin: 12px 0 10px;
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 28px;
    font-weight: 600;
    letter-spacing: 0.12em;
    color: var(--yap-fg);
  }
  .phone-wait {
    margin-top: 14px;
    align-items: center;
  }
  .phone-wait .spinner {
    margin-top: 0;
  }

  .profile {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 20px 22px;
    margin-bottom: 22px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .avatar {
    width: 52px;
    height: 52px;
    flex: 0 0 52px;
    border-radius: var(--yap-r-full);
    object-fit: cover;
  }
  .initials {
    display: grid;
    place-items: center;
    background: var(--yap-primary-tint);
    color: var(--yap-primary-hover);
    font-family: var(--yap-font-display);
    font-size: 22px;
    font-weight: 600;
  }
  .who {
    flex: 1;
    min-width: 0;
  }
  .who h3 {
    margin: 0 0 1px;
    font-family: var(--yap-font-display);
    font-size: 22px;
    font-weight: 500;
    color: var(--yap-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .who p {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .offline {
    color: var(--yap-warning);
  }

  .notice {
    padding: 12px 14px;
    margin-bottom: 18px;
    border-radius: var(--yap-r);
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border);
    font-size: 12.5px;
    color: var(--yap-fg-80);
  }
  .danger {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
    font-size: 12.5px;
  }
  .danger .actions {
    margin-top: 0;
  }
  .soon-tag {
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-muted);
    background: var(--yap-raised);
    padding: 2px 9px;
    border-radius: var(--yap-r-sm);
  }
  .footer-links {
    display: flex;
    gap: 8px;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
</style>
