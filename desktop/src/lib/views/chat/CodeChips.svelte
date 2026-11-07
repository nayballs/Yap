<script>
  // **A Code chat's chips**, first under Chat's box (codeChats.svelte.js):
  // the workspace chip — "</> garden-planner ▾", the workspace picker above
  // it to switch the chat's workspace or make it a plain chat — and Jobs'
  // Mode chip beside it. A plain chat shows neither (the Mode chip stays
  // mounted, hidden, so a new chat's pick still reaches its conversation).
  import { tick } from 'svelte';
  import ModeChip from './ModeChip.svelte';
  import WorkspacePicker from './WorkspacePicker.svelte';

  /** `chat`: the view's state; `code`: its Code chats. */
  let { chat, code } = $props();

  let open = $state(false);
  let chipEl = $state(null);

  const ws = $derived(code?.current ?? null);
  const shown = $derived(!!code?.enabled && (!!ws || code.onCode));

  function close(focusBack) {
    open = false;
    if (focusBack) tick().then(() => chipEl?.focus());
  }
</script>

{#if shown}
  <button
    type="button"
    class="wschip"
    class:none={!ws}
    bind:this={chipEl}
    aria-haspopup="menu"
    aria-expanded={open}
    aria-label={ws ? `Workspace: ${ws.name}. Change workspace` : 'Choose a workspace'}
    title={ws ? ws.path : 'Choose the workspace this code chat works in'}
    data-testid="chat-workspace-chip"
    data-workspace={ws?.name ?? ''}
    onclick={() => (open = !open)}
  >
    <svg class="glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m8 7-5 5 5 5" /><path d="m16 7 5 5-5 5" /></svg>
    <span class="wlabel">{ws ? ws.name : 'Choose a workspace'}</span>
    <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
  </button>
  {#if open}
    <WorkspacePicker {code} anchor={chipEl} place="above" current={ws} plain={!!ws} onpick={(w) => code.setWorkspace(w)} onplain={() => code.makePlain()} onclose={close} />
  {/if}
{/if}
<ModeChip {chat} shown={!!ws} />

<style>
  /* The model chip's quiet look, in the Code accent. */
  /* (In a narrow thread it gives way after the model chip: its name ellipsized.) */
  .wschip {
    flex: 0 1 auto;
    min-width: 72px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 220px;
    height: 26px;
    padding: 0 7px 0 8px;
    border: 1px solid var(--yap-code-line);
    border-radius: var(--yap-r-full);
    background: var(--yap-code-wash);
    color: var(--yap-code);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--yap-dur);
  }
  .wschip:hover,
  .wschip[aria-expanded='true'] {
    background: var(--yap-code-tint);
  }
  .wschip:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: 1px;
  }
  .wschip.none {
    border-style: dashed;
  }
  .glyph {
    flex: none;
    width: 13px;
    height: 13px;
  }
  .wlabel {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
  }
  .chev {
    flex: none;
    width: 11px;
    height: 11px;
    opacity: 0.75;
  }
</style>
