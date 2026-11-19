<script>
  // **"May Yap's AI use these apps?"** (computer/access.rs, a `confirm`
  // card with act "access"; computer use's `request_access`): the apps
  // asked for, each with its icon and how far it may be used (a browser is
  // read only, a terminal or an IDE takes clicks only), the model's reason,
  // and the extras it asked for (the clipboard, system shortcuts), each a
  // box to untick. **Allow** lets the whole set in for this chat, **Don't**
  // lets none (`chat_computer_access`). Apps that can never be allowed (Yap
  // itself, a password manager, one not in the Start menu) are listed with
  // why. A Code chat in Bypass permissions shows it already allowed. No
  // answer in five minutes is a no. Kept with the answer, answered
  // (ChatMessage.confirms). In PageConfirm's chunk.
  import { invoke } from '@tauri-apps/api/core';

  /** `card`: the confirm event (`{id, state, access: {apps: [{name, id, tier}], reason, clipboardRead, clipboardWrite, systemKeyCombos, denied: [{name, why}], bypass}}`). */
  let { card } = $props();

  let answered = $state('');
  const st = $derived((card.state === 'asking' && answered) || card.state);
  const a = $derived(card.access ?? {});
  const apps = $derived(Array.isArray(a.apps) ? a.apps : []);
  const denied = $derived(Array.isArray(a.denied) ? a.denied : []);

  // The extras asked for, each a box ticked to start with.
  let keep = $state({ clipboardRead: true, clipboardWrite: true, systemKeyCombos: true });
  const EXTRAS = [
    ['clipboardRead', 'Read your clipboard'],
    ['clipboardWrite', 'Put text on your clipboard'],
    ['systemKeyCombos', 'Use system shortcuts (switch or close apps, lock the screen)'],
  ];
  const extras = $derived(EXTRAS.filter(([k]) => a[k]));

  const TIER = { read: 'Read only', click: 'Clicks only' };
  const TIER_TIP = {
    read: 'A browser: Yap sees it but never clicks or types in it. Web pages open in Yap’s own browser beside the chat.',
    click: 'A terminal or a code editor: Yap may click and scroll, never type.',
  };

  // Each app's icon, from Yap's app index (`apps_icons`).
  let icons = $state({});
  $effect(() => {
    const ids = apps.map((x) => x.id).filter(Boolean);
    if (!ids.length) return;
    invoke('apps_icons', { ids })
      .then((got) => {
        if (got && typeof got === 'object') icons = got;
      })
      .catch(() => {});
  });

  async function answer(allow) {
    answered = allow ? 'allowed' : 'refused';
    try {
      await invoke('chat_computer_access', {
        id: card.id,
        allow,
        clipboardRead: keep.clipboardRead,
        clipboardWrite: keep.clipboardWrite,
        systemKeyCombos: keep.systemKeyCombos,
      });
    } catch {
      /* the card had lapsed (the turn stopped): it says no */
    }
  }

  const names = $derived(apps.map((x) => x.name).join(', '));
  const done = (s) =>
    s === 'allowed'
      ? a.bypass
        ? `Allowed in this chat (Bypass permissions): ${names}.`
        : `Allowed in this chat: ${names}.`
      : s === 'timedOut'
        ? 'No answer in time, so Yap didn’t use them.'
        : 'Not allowed: Yap won’t use them.';
</script>

<div class="xaccess" class:asking={st === 'asking'} role="group" aria-label="Yap asks to use apps on your PC" data-testid="chat-computer-access" data-state={st}>
  <svg class="xicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"
    ><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></svg
  >
  <div class="xbody">
    <p class="xq">{st === 'asking' ? 'Let Yap’s AI use these apps on your PC?' : 'Apps on your PC'}</p>
    {#if a.reason}<p class="xreason" data-testid="chat-computer-reason">{a.reason}</p>{/if}
    <ul class="xapps" data-testid="chat-computer-apps">
      {#each apps as app (app.id || app.name)}
        <li>
          {#if icons[app.id]}<img src={icons[app.id]} alt="" width="16" height="16" />{:else}<span class="xblank" aria-hidden="true"></span>{/if}
          <span class="xname">{app.name}</span>
          {#if TIER[app.tier]}<span class="xtier" title={TIER_TIP[app.tier]}>{TIER[app.tier]}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if denied.length}
      <ul class="xdenied" aria-label="Not allowed" data-testid="chat-computer-denied">
        {#each denied as d, i (i)}<li><span class="xname">{d.name}</span>: {d.why}</li>{/each}
      </ul>
    {/if}
    {#if st === 'asking'}
      {#if extras.length}
        <div class="xextras">
          {#each extras as [k, label] (k)}
            <label><input type="checkbox" bind:checked={keep[k]} /> {label}</label>
          {/each}
        </div>
      {/if}
      <p class="xwhy">
        In this chat, Yap’s AI will see these apps’ windows; everything else on your screen stays hidden from it. It can’t click or type in them yet.
      </p>
      <div class="xbtns">
        <button type="button" class="xallow" onclick={() => answer(true)}>Allow</button>
        <button type="button" class="xdeny" onclick={() => answer(false)}>Don't</button>
      </div>
    {:else}
      <p class="xdone" aria-live="polite">{done(st)}</p>
    {/if}
  </div>
</div>

<style>
  .xaccess {
    display: flex;
    gap: 10px;
    max-width: min(560px, 100%);
    margin: 6px 0 2px;
    padding: 10px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  .xaccess.asking {
    border-color: var(--yap-primary-line);
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .xicon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-primary);
  }
  .xbody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .xq,
  .xreason,
  .xwhy,
  .xdone {
    margin: 0;
  }
  .xq {
    font-weight: 600;
  }
  .xreason {
    margin-top: 3px;
    overflow-wrap: anywhere;
  }
  .xapps,
  .xdenied {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }
  .xapps li {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 3px 0;
  }
  .xapps img,
  .xblank {
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    border-radius: 3px;
  }
  .xblank {
    background: var(--yap-paper-s2);
  }
  .xname {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .xtier {
    padding: 1px 7px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .xdenied li {
    font-size: 12px;
    color: var(--yap-paper-muted);
    overflow-wrap: anywhere;
  }
  .xextras {
    display: grid;
    gap: 4px;
    margin-top: 8px;
    font-size: 12.5px;
  }
  .xextras label {
    display: flex;
    align-items: center;
    gap: 7px;
    cursor: pointer;
  }
  .xwhy,
  .xdone {
    margin-top: 8px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .xbtns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 9px;
  }
  .xbtns button {
    height: 28px;
    padding: 0 13px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .xbtns .xallow {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .xbtns button:hover {
    background: var(--yap-paper-s2);
  }
  .xbtns .xallow:hover {
    background: var(--yap-primary-hover);
  }
  .xbtns button:focus-visible,
  .xextras input:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
