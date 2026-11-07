<script>
  // **Claude Code on this PC**, as Settings → Jobs shows it
  // (ai/claude_code.rs `claude_code_status`): found (its version) and
  // signed in (its plan, when it says), or not — with **Sign in**, which
  // opens Claude Code's own sign-in in a terminal (`claude_code_sign_in`:
  // `claude auth login`; Yap never sees the sign-in), and Check again. One
  // small component, so a shared status store (Chat on your Claude plan's)
  // can take its place.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Row from '../ui/Row.svelte';
  import Button from '../ui/Button.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { openExternalLink } from '../core/externalLinks.js';

  const INSTALL = 'https://code.claude.com/docs/en/setup';

  /** `setting`: the row's anchor. */
  let { setting = 'jobs-claude-code' } = $props();

  let status = $state(null);
  let checking = $state(false);
  let signingIn = $state(false);

  async function check(refresh = false) {
    checking = true;
    try {
      status = await invoke('claude_code_status', { refresh });
    } catch {
      status = { installed: false, error: "Couldn't check Claude Code." };
    } finally {
      checking = false;
    }
  }

  onMount(() => {
    check(false);
  });

  // Back from the sign-in terminal: Yap checks again when its window comes back.
  function onFocus() {
    if (signingIn) {
      signingIn = false;
      check(true);
    }
  }

  async function signIn() {
    try {
      await invoke('claude_code_sign_in');
      signingIn = true;
      toast({ title: "Claude Code's sign-in is open", description: 'Finish signing in in the terminal, then come back here.', variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't open Claude Code's sign-in", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }

  const planName = (p) => (p ? `Claude ${String(p).charAt(0).toUpperCase()}${String(p).slice(1)}` : '');
  const line = $derived(
    !status
      ? 'Checking Claude Code on this PC…'
      : !status.installed
        ? "Claude Code isn't on this PC. Jobs run your own Claude Code: install it, then check again."
        : status.tooOld
          ? `Claude Code ${status.version} is too old for jobs: update it to ${status.minVersion} or later.`
          : status.loggedIn
            ? `Claude Code ${status.version ?? ''} · signed in${status.plan ? ` · ${planName(status.plan)}` : ''}`
            : `Claude Code ${status.version ?? ''} is on this PC, but isn't signed in.`
  );
  const ok = $derived(!!status?.installed && !status.tooOld && status.loggedIn);
</script>

<svelte:window onfocus={onFocus} />

<Row label="Claude Code" desc={line} {setting}>
  <div class="cc" data-testid="jobs-claude-code" data-state={!status ? 'checking' : ok ? 'ready' : !status.installed ? 'missing' : status.tooOld ? 'old' : 'signed-out'}>
    {#if ok}
      <span class="okmark" aria-hidden="true">✓</span>
    {/if}
    {#if status && !status.installed}
      <Button variant="secondary" size="sm" onclick={() => openExternalLink(INSTALL)}>How to install</Button>
    {:else if status?.installed && !status.tooOld && !status.loggedIn}
      <Button variant="primary" size="sm" onclick={signIn}>Sign in</Button>
    {/if}
    <Button variant="ghost" size="sm" disabled={checking} onclick={() => check(true)}>{checking ? 'Checking…' : 'Check again'}</Button>
  </div>
</Row>

<style>
  .cc {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .okmark {
    color: var(--yap-success-text);
    font-weight: 700;
  }
</style>
