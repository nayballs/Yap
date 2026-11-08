<script>
  // **A Code chat's Environment card** (Remote Control, 2026-11-08; after the
  // Claude app's session details): opened from the "Code · garden-planner"
  // label on the message box's edge (CodeEdge.svelte). What it holds:
  // - the workspace: its name, its path, "Runs on this PC";
  // - **Remote Control** for this workspace (`link_workspace_remote`, on unless
  //   switched off): with Settings → Phone's switch off, turning it on turns
  //   that on too, asked once with what it allows;
  // - the paired phones with Remote Control: "Office phone is connected" (a
  //   green dot) or "No phone connected", and Turn off for a phone (its tick,
  //   `link_phone_remote`);
  // - the chat's Mode, and that from a phone agents run at most at Auto.
  // Kept modest: wave 3 polishes it. Esc and a click outside close it.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { toast } from '../../ui/toast.svelte.js';

  /** `workspace`: `{name, path}`; `conversation`: the chat's id (none: a new chat); `onclose()`. */
  let { workspace, conversation = null, onclose = null } = $props();

  let status = $state(null);
  let mode = $state(null);
  let asking = $state(false);
  let card = $state(null);

  async function load() {
    try {
      status = await invoke('link_remote_status', { workspace: workspace?.name ?? null });
    } catch {
      status = null;
    }
    try {
      mode = await invoke('agent_mode_get', { conversation: conversation ?? null });
    } catch {
      mode = null;
    }
  }

  onMount(() => {
    load();
    const uns = [];
    listen('yap-link-changed', load).then((u) => uns.push(u));
    const onKey = (e) => {
      if (e.key === 'Escape' && !asking) onclose?.();
    };
    const onDown = (e) => {
      if (card && !card.contains(e.target) && !asking && !e.target.closest?.('[data-testid="chat-code-label"]')) onclose?.();
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('pointerdown', onDown, true);
    return () => {
      uns.forEach((u) => u());
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('pointerdown', onDown, true);
    };
  });

  const on = $derived(!!status?.on && !!status?.workspaceAllowed);
  const phones = $derived((status?.phones ?? []).filter((p) => p.remote));
  const connected = $derived(phones.filter((p) => p.connected));

  /** Settings → Phone's switch, patched onto a fresh config (as the Integrations view's). */
  async function setMaster(next) {
    const fresh = await invoke('get_config');
    await invoke('save_config', { cfg: { ...fresh, linkRemoteControl: next }, base: fresh });
    window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: { linkRemoteControl: next } }));
  }

  async function setWorkspace(next) {
    await invoke('link_workspace_remote', { name: workspace.name, on: next });
  }

  async function toggle() {
    if (!status) return;
    try {
      if (on) {
        await setWorkspace(false);
      } else if (!status.on) {
        // Asked once, with what it allows.
        asking = true;
        return;
      } else {
        await setWorkspace(true);
      }
    } catch (e) {
      toast({ title: "Couldn't change Remote Control", description: String(e?.message ?? e), variant: 'destructive' });
    }
    load();
  }

  async function turnOnEverywhere() {
    try {
      await setWorkspace(true);
      await setMaster(true);
    } catch (e) {
      toast({ title: "Couldn't turn Remote Control on", description: String(e?.message ?? e), variant: 'destructive' });
    }
    asking = false;
    setTimeout(load, 400);
  }

  async function phoneOff(p) {
    try {
      await invoke('link_phone_remote', { id: p.id, on: false });
    } catch {
      /* said by its row staying */
    }
    load();
  }
</script>

<div class="env" role="dialog" aria-label="Environment" data-testid="code-environment" bind:this={card}>
  <p class="h">Environment</p>
  <div class="sec">
    <p class="k">Workspace</p>
    <p class="v"><b>{workspace?.name}</b></p>
    <p class="path" title={workspace?.path}>{workspace?.path}</p>
    <p class="small">Runs on this PC</p>
  </div>
  <div class="sec row">
    <div>
      <p class="k">Remote Control</p>
      <p class="small">
        {#if on}
          Your paired phones can follow agents here, allow their steps and send them work.
        {:else if status && !status.on}
          Off. Turn it on to use this workspace from your phone.
        {:else}
          Off for this workspace.
        {/if}
      </p>
    </div>
    <button
      type="button"
      class="switch"
      role="switch"
      aria-checked={on}
      aria-label="Remote Control for {workspace?.name}"
      data-testid="code-environment-remote"
      onclick={toggle}
    ><span class="knob" aria-hidden="true"></span></button>
  </div>
  {#if status?.on}
    <div class="sec" data-testid="code-environment-phones">
      <p class="k">Phones</p>
      {#if !status.link}
        <p class="small">Use Yap from your phone is off (Settings → Phone).</p>
      {:else if connected.length}
        {#each connected as p (p.id)}
          <p class="phone"><span class="dot on" aria-hidden="true"></span>{p.name} is connected <button type="button" class="link" onclick={() => phoneOff(p)}>Turn off for this phone</button></p>
        {/each}
      {:else if phones.length}
        <p class="phone"><span class="dot" aria-hidden="true"></span>No phone connected</p>
      {:else}
        <p class="small">No phone has Remote Control yet: tick one in Settings → Phone.</p>
      {/if}
    </div>
  {/if}
  {#if mode}
    <div class="sec">
      <p class="k">Mode</p>
      <p class="v">{mode.label}</p>
      <p class="small">From your phone, agents run at most at Auto.</p>
    </div>
  {/if}
</div>

<ConfirmDialog
  bind:open={asking}
  title="Turn on Remote Control?"
  body="Your paired phones ticked for it in Settings → Phone can follow Yap Code's agents, allow their steps and send them work, on your home Wi-Fi. From a phone, agents run at most at Auto, never Bypass permissions, and every step allowed there is recorded."
  confirmLabel="Turn on"
  danger={false}
  onconfirm={turnOnEverywhere}
/>

<style>
  .env {
    position: absolute;
    bottom: calc(100% + 10px);
    left: 8px;
    z-index: 30;
    box-sizing: border-box;
    width: min(320px, calc(100vw - 32px));
    padding: 12px 14px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-paper);
    box-shadow: var(--yap-shadow-popover);
    color: var(--yap-paper-ink);
    font-size: 12.5px;
    text-align: left;
  }
  .h {
    margin: 0 0 8px;
    font-weight: 650;
    font-size: 13px;
  }
  .sec {
    padding: 8px 0;
    border-top: 1px solid var(--yap-paper-rule);
  }
  .row {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
  }
  .k {
    margin: 0 0 2px;
    color: var(--yap-muted);
    font-size: 11.5px;
    font-weight: 600;
  }
  .v,
  .small,
  .phone,
  .path {
    margin: 0;
  }
  .path {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    color: var(--yap-fg-80);
    overflow-wrap: anywhere;
  }
  .small {
    color: var(--yap-muted);
  }
  .phone {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: var(--yap-r-full);
    background: var(--yap-muted-55);
  }
  .dot.on {
    background: var(--yap-success);
  }
  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-code);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .switch {
    flex: none;
    position: relative;
    width: 34px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    cursor: pointer;
  }
  .switch[aria-checked='true'] {
    border-color: var(--yap-code);
    background: var(--yap-code);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: var(--yap-r-full);
    background: var(--yap-paper);
    transition: transform 0.12s ease;
  }
  .switch[aria-checked='true'] .knob {
    transform: translateX(14px);
  }
  .switch:focus-visible,
  .link:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  @media (prefers-reduced-motion: reduce) {
    .knob {
      transition: none;
    }
  }
</style>
