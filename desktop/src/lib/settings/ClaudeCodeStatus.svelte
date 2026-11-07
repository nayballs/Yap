<script>
  // **Claude Code on this PC**, as Settings → Jobs shows it: the same
  // status and Sign in as Language Models' Claude tab
  // (ClaudePlanProvider.svelte), from the one store
  // (stores/claudePlan.svelte.js, `claude_plan_status`: probes throttled to
  // one per 5 s, Check again always probes). Found (its version) and signed
  // in (its plan, when it says), or not — with **Sign in**, Claude Code's
  // own sign-in in a terminal Yap opens (Yap never sees it; it reads the
  // status again when its window gets the focus back).
  import { onMount } from 'svelte';
  import Row from '../ui/Row.svelte';
  import Button from '../ui/Button.svelte';
  import { openExternalLink } from '../core/externalLinks.js';
  import { claudePlan, loadClaudePlan, signInClaudePlan } from '../stores/claudePlan.svelte.js';

  /** `setting`: the row's anchor. */
  let { setting = 'jobs-claude-code' } = $props();

  onMount(() => loadClaudePlan(true));

  const planName = (p) => (p ? `Claude ${String(p).charAt(0).toUpperCase()}${String(p).slice(1)}` : '');
  // Jobs run on a key login too (Pay with: API key): signed in is enough.
  const ok = $derived(claudePlan.loaded && claudePlan.installed && !claudePlan.tooOld && claudePlan.loggedIn);
  const line = $derived(
    !claudePlan.loaded
      ? 'Looking for Claude Code on this PC…'
      : !claudePlan.installed
        ? "Claude Code isn't installed on this PC. Jobs run your own Claude Code: install it, then check again."
        : claudePlan.tooOld
          ? `Claude Code ${claudePlan.version} is too old for jobs: Yap needs ${claudePlan.minVersion} or newer.`
          : claudePlan.loggedIn
            ? `Claude Code ${claudePlan.version ?? ''} · signed in${claudePlan.plan ? ` · ${planName(claudePlan.plan)}` : claudePlan.onAKey ? ' with an API key' : ''}`
            : `Claude Code ${claudePlan.version ?? ''} is on this PC, but isn't signed in.`
  );
  const state = $derived(!claudePlan.loaded ? 'checking' : ok ? 'ready' : !claudePlan.installed ? 'missing' : claudePlan.tooOld ? 'old' : 'signed-out');
</script>

<Row label="Claude Code" desc={line} {setting}>
  <div class="cc" data-testid="jobs-claude-code" data-state={state}>
    {#if ok}
      <span class="okmark" aria-hidden="true">✓</span>
    {/if}
    {#if claudePlan.loaded && (!claudePlan.installed || claudePlan.tooOld)}
      <Button variant="secondary" size="sm" onclick={() => openExternalLink(claudePlan.installUrl)}>{claudePlan.tooOld ? 'How to update' : 'Install Claude Code'}</Button>
    {:else if claudePlan.loaded && !claudePlan.loggedIn}
      <Button variant="primary" size="sm" onclick={signInClaudePlan}>Sign in</Button>
    {/if}
    <Button variant="ghost" size="sm" disabled={claudePlan.checking} onclick={() => loadClaudePlan(true, true)}>{claudePlan.checking ? 'Checking…' : 'Check again'}</Button>
  </div>
</Row>
{#if claudePlan.failed}
  <Row>
    <p class="set-note" role="alert">{claudePlan.failed}</p>
  </Row>
{/if}

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
