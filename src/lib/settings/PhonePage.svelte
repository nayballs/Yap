<script>
  // Settings → Phone (Yap Link, src-tauri/src/link.rs): talk to Yap's AI
  // from a phone on the home Wi-Fi. The switch (`linkEnabled`, auto-saved
  // like every setting: the server starts and stops with it), Pair a phone
  // (a QR code of the pairing link, the link with Copy, the one-time
  // certificate warning said in one line), the paired phones with Remove,
  // and what this preview can't do yet. Follows `yap-link-changed` live (a
  // phone paired, connected, gone).
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Button from '../ui/Button.svelte';
  import Select from '../ui/Select.svelte';
  import { toast } from '../ui/toast.svelte.js';

  let { cfg } = $props();

  let status = $state(null);
  let pair = $state(null); // { url, address, qr, expiresMs }
  let pairAt = $state(0);
  let now = $state(Date.now());
  let busy = $state(false);
  let address = $state('');

  async function load() {
    try {
      status = await invoke('link_status');
    } catch {
      status = null;
    }
  }

  onMount(() => {
    load();
    const uns = [];
    listen('yap-link-changed', async () => {
      const before = new Set((status?.phones ?? []).map((p) => p.id));
      await load();
      // A phone paired while the code was up: the code's job is done.
      const added = (status?.phones ?? []).find((p) => !before.has(p.id));
      if (added && pair) {
        pair = null;
        toast({ title: `“${added.name}” is paired`, description: 'Ask away: press Talk on your phone.', variant: 'success' });
      }
    }).then((u) => uns.push(u));
    const tick = setInterval(() => (now = Date.now()), 1000);
    return () => {
      clearInterval(tick);
      uns.forEach((u) => u());
      if (pair) invoke('link_pair_cancel').catch(() => {});
    };
  });

  // The switch went on or off: Rust follows the save; read it again shortly.
  let lastOn = null;
  $effect(() => {
    const on = cfg.linkEnabled;
    if (lastOn !== null && on !== lastOn) {
      if (!on) pair = null;
      setTimeout(load, 600);
    }
    lastOn = on;
  });

  async function startPairing(at = address) {
    busy = true;
    try {
      pair = await invoke('link_pair_start', { address: at || null });
      pairAt = Date.now();
      address = pair.address;
    } catch (e) {
      toast({ title: "Couldn't make a pairing code", description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      busy = false;
    }
  }

  function stopPairing() {
    pair = null;
    invoke('link_pair_cancel').catch(() => {});
  }

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(pair.url);
      toast({ title: 'Link copied', variant: 'success' });
    } catch {
      toast({ title: "Couldn't copy to clipboard", variant: 'destructive' });
    }
  }

  async function remove(phone) {
    try {
      await invoke('link_remove', { id: phone.id });
      toast({ title: `“${phone.name}” removed`, description: "It can't use Yap any more. Pair it again to bring it back." });
    } catch (e) {
      toast({ title: "Couldn't remove it", description: String(e?.message ?? e), variant: 'destructive' });
    }
    load();
  }

  const left = $derived(pair ? Math.max(0, Math.round((pair.expiresMs - (now - pairAt)) / 1000)) : 0);
  const lapsed = $derived(!!pair && left <= 0);
  const leftText = $derived(left >= 60 ? `${Math.ceil(left / 60)} min` : `${left} s`);
  const addressOptions = $derived((status?.addresses ?? []).map((a) => ({ value: a.ip, label: a.adapter ? `${a.ip} · ${a.adapter}` : a.ip })));
  const qrPx = $derived(pair?.qr ? (pair.qr.size + 8) * 5 : 0);

  const DAY = 86_400_000;
  const startOfDay = (ms) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const shortDate = (ms) => new Date(ms).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  function used(ms) {
    if (!ms) return 'never used';
    const days = Math.round((startOfDay(Date.now()) - startOfDay(ms)) / DAY);
    if (days <= 0) return 'last used today';
    if (days === 1) return 'last used yesterday';
    if (days < 14) return `last used ${days} days ago`;
    return `last used ${shortDate(ms)}`;
  }
  const phoneMeta = (p) => (p.connected ? `Paired ${shortDate(p.paired)} · connected now` : `Paired ${shortDate(p.paired)} · ${used(p.lastSeen)}`);
</script>

<div class="page-h">
  <h1>Phone</h1>
  <p>Talk to Yap's AI from your phone around the house: ask out loud, and hear the answer in your headset.</p>
</div>

<Group title="Your phone" id="settings-phone">
  <Row setting="link-enabled">
    <Toggle
      bind:checked={cfg.linkEnabled}
      label="Use Yap from your phone (on your home Wi-Fi)"
      desc="Your phone opens Yap's page from this PC, and questions go to Chat's model, answered out loud on the phone"
      hint="Only phones on your home network, and only ones you've paired. The first time, Windows may ask whether Yap can use the network: choose Private networks."
    />
  </Row>
  {#if cfg.linkEnabled && status?.error}
    <Row>
      <p class="set-note warn" role="alert">{status.error}</p>
    </Row>
  {/if}
</Group>

{#if cfg.linkEnabled && status?.running}
  <Group title="Pair a phone" id="settings-phone-pair">
    {#if !pair}
      <Row label="Pair a phone" desc="Show a code to scan with your phone's camera, then say yes here" setting="link-pair">
        <Button variant="secondary" size="sm" disabled={busy} onclick={() => startPairing()}>Pair a phone</Button>
      </Row>
    {:else}
      <div class="pairing" data-setting="link-pair">
        <div class="qr" class:lapsed>
          {#if pair.qr}
            <svg
              width={qrPx}
              height={qrPx}
              viewBox="-4 -4 {pair.qr.size + 8} {pair.qr.size + 8}"
              shape-rendering="crispEdges"
              role="img"
              aria-label={lapsed ? 'Expired pairing code' : "QR code that opens Yap's page on your phone"}
            >
              <rect x="-4" y="-4" width={pair.qr.size + 8} height={pair.qr.size + 8} fill="#fff" />
              <path d={pair.qr.path} fill="#1f1d17" />
            </svg>
          {/if}
        </div>
        <div class="how">
          <ol>
            <li>On your phone, connected to the same Wi-Fi, scan this with the camera.</li>
            <li>Chrome warns once that the connection isn't private: tap <b>Advanced</b>, then <b>Proceed</b>. Yap's page needs the secure connection for the microphone.</li>
            <li>Say yes here when Yap asks, if the code matches your phone's.</li>
          </ol>
          <div class="link">
            <code class="url" aria-label="Pairing link">{pair.url}</code>
            <button class="mini" aria-label="Copy link" onclick={copyLink}>Copy</button>
          </div>
          <p class="when" aria-live="polite">
            {#if lapsed}
              This code has run out.
            {:else}
              Good once, for {leftText}.
            {/if}
          </p>
          {#if addressOptions.length > 1}
            <div class="addr">
              <span>Address</span>
              <Select bind:value={address} options={addressOptions} ariaLabel="Address" onchange={(v) => startPairing(v ?? address)} />
            </div>
          {/if}
          <div class="actions">
            {#if lapsed}
              <Button variant="secondary" size="sm" disabled={busy} onclick={() => startPairing()}>New code</Button>
            {/if}
            <Button variant="ghost" size="sm" onclick={stopPairing}>Done</Button>
          </div>
        </div>
      </div>
    {/if}
  </Group>
{/if}

<Group title="Paired phones" id="settings-phone-list">
  <div class="phones" data-setting="link-phones">
    {#if status?.phones?.length}
      <ul aria-label="Paired phones">
        {#each status.phones as p (p.id)}
          <li class="phone">
            <span class="dot" class:on={p.connected} aria-hidden="true"></span>
            <span class="who">
              <span class="name">{p.name}</span>
              <span class="meta">{phoneMeta(p)}</span>
            </span>
            <Button variant="ghost" size="sm" title="Remove {p.name}" onclick={() => remove(p)}>Remove</Button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="set-note">No phones paired yet.</p>
    {/if}
  </div>
</Group>

<Group title="Good to know" id="settings-phone-notes">
  <ul class="notes">
    <li>Your phone and this PC need to be on the same Wi-Fi. Away from home, it doesn't connect.</li>
    <li>Keep the phone's screen on while you talk: Yap's page stops listening when the screen locks.</li>
    <li>The headset's own button doesn't start a question yet: press Talk on the screen.</li>
    <li>Your questions and answers are in Chat on this PC, as the Voice Assistant's are. Nothing goes to any server for the connection itself.</li>
  </ul>
</Group>

<style>
  .warn {
    color: var(--yap-danger);
  }
  .pairing {
    display: flex;
    gap: 20px;
    align-items: flex-start;
    padding: 14px 0 4px;
    flex-wrap: wrap;
  }
  .qr {
    flex: none;
    line-height: 0;
    overflow: hidden;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    /* Dark modules on white, always (as the Account page's code): a scanner
       needs them, whatever the theme. */
    background: #fff;
    box-shadow: var(--yap-shadow-sm);
    forced-color-adjust: none;
  }
  .qr.lapsed svg {
    filter: blur(3px);
    opacity: 0.4;
  }
  .how {
    flex: 1 1 260px;
    min-width: 0;
    font-size: 13px;
    color: var(--yap-fg-80);
  }
  .how ol {
    margin: 0 0 12px;
    padding-left: 18px;
    line-height: 1.5;
  }
  .how li + li {
    margin-top: 4px;
  }
  .link {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .url {
    flex: 1 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
    padding: 6px 8px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-sm);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 12px;
  }
  .mini {
    flex: 0 0 auto;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-sm);
    padding: 5px 10px;
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
    cursor: pointer;
  }
  .mini:hover {
    border-color: var(--yap-border-hover);
  }
  .mini:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .when {
    margin: 8px 0 0;
    color: var(--yap-muted);
    font-size: 12.5px;
  }
  .addr {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-top: 10px;
    color: var(--yap-muted);
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
  .phones ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .phone {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 0;
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .phone:last-child {
    border-bottom: none;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: var(--yap-r-full);
    background: var(--yap-muted-55);
  }
  .dot.on {
    background: var(--yap-success);
  }
  .who {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-weight: 600;
    color: var(--yap-fg);
    overflow-wrap: anywhere;
  }
  .meta {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .notes {
    margin: 8px 0 0;
    padding-left: 18px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .notes li + li {
    margin-top: 4px;
  }
</style>
