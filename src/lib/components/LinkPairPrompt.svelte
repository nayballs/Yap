<script>
  // "Allow ‘Android · Chrome’ to use Yap? Code 4821" (Yap Link, link.rs):
  // a phone that scanned Settings → Phone's code waits for a yes here. The
  // code is the phone's too; Allow pairs it, Deny (or Esc) refuses. A phone
  // that gave up, or Link going off, takes the question away
  // (`yap-link-pair-done`). One at a time, oldest first; the ones already
  // waiting when the window opened are read from `link_status`.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';

  let asks = $state([]); // [{ id, name, code }]
  let open = $state(false);
  let ask = $state(null); // the one on screen

  onMount(() => {
    const uns = [];
    listen('yap-link-pair', (e) => {
      const a = e.payload;
      if (a && !asks.some((x) => x.id === a.id)) asks = [...asks, a];
    }).then((u) => uns.push(u));
    listen('yap-link-pair-done', (e) => {
      const id = e.payload?.id;
      asks = asks.filter((x) => x.id !== id);
    }).then((u) => uns.push(u));
    invoke('link_status')
      .then((s) => {
        for (const a of s?.asks ?? []) if (!asks.some((x) => x.id === a.id)) asks = [...asks, a];
      })
      .catch(() => {});
    return () => uns.forEach((u) => u());
  });

  // The next question comes up as soon as there is one.
  $effect(() => {
    if (!ask && asks.length) {
      ask = asks[0];
      open = true;
    }
  });

  // Taken away (the phone gave up, Link went off): it goes from the screen.
  $effect(() => {
    if (ask && !asks.some((x) => x.id === ask.id)) {
      ask = null;
      open = false;
    }
  });

  function reply(allow) {
    const a = ask;
    if (!a) return;
    ask = null;
    asks = asks.filter((x) => x.id !== a.id);
    invoke('link_pair_answer', { id: a.id, allow }).catch(() => {});
  }

  // Closed without Allow (Deny, Esc, the scrim): a no.
  $effect(() => {
    if (!open && ask) reply(false);
  });

  function allow() {
    reply(true);
    open = false;
  }
</script>

<ConfirmDialog
  bind:open
  title={ask ? `Allow ‘${ask.name}’ to use Yap?` : ''}
  body="It can ask Yap's AI and hear the answers, from your home Wi-Fi. Allow it only if your phone shows the same code."
  confirmLabel="Allow"
  cancelLabel="Deny"
  danger={false}
  onconfirm={allow}
>
  {#if ask}
    <p class="code" aria-label="Code {ask.code.split('').join(' ')}">Code <b>{ask.code}</b></p>
  {/if}
</ConfirmDialog>

<style>
  .code {
    margin: 14px 0 0;
    font-size: 15px;
    color: var(--yap-paper-muted);
  }
  .code b {
    margin-left: 6px;
    font-size: 26px;
    font-weight: 650;
    letter-spacing: 0.18em;
    color: var(--yap-paper-ink);
    font-variant-numeric: tabular-nums;
  }
</style>
