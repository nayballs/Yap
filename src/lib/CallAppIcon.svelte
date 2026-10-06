<script>
  // A call app's icon, for Settings → General → Meetings ("Ask about calls
  // in") and the Yap bar's call card: the app's own icon as installed on
  // this PC (app_icons.rs, through callAppIcons.svelte.js), else its bundled
  // mark (bar/callApps.js: Simple Icons, CC0), else a letter badge in its
  // colour, else a phone. Decorative: the row or the card names the app.
  // `data-icon` says which it drew (installed | mark | letter | phone).
  import { onMount } from 'svelte';
  import { callAppIcon } from './bar/callApps.js';
  import { installedIcons, loadInstalledIcons } from './callAppIcons.svelte.js';

  /** @type {{ app?: string | null, size?: number, disabled?: boolean }} */
  let { app = null, size = 20, disabled = false } = $props();

  onMount(() => {
    loadInstalledIcons();
  });

  // An icon the webview couldn't draw falls back to the mark.
  let broken = $state(null);
  const installed = $derived(app && installedIcons[app] && installedIcons[app] !== broken ? installedIcons[app] : null);
  const mark = $derived(callAppIcon(app));
  const kind = $derived(
    installed ? 'installed' : mark.kind === 'glyph' ? 'mark' : mark.kind === 'monogram' ? 'letter' : 'phone'
  );
</script>

<span
  class="icon"
  class:disabled
  data-app-icon={app}
  data-icon={kind}
  style={`--size:${size}px`}
  aria-hidden="true"
>
  {#if installed}
    <img src={installed} alt="" draggable="false" onerror={() => (broken = installed)} />
  {:else if mark.kind === 'glyph'}
    <svg viewBox="0 0 24 24"><path d={mark.path} fill={mark.color} /></svg>
  {:else if mark.kind === 'monogram'}
    <span class="letter" style={`background:${mark.color}`}>{mark.letter}</span>
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="#26231c" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.4 1.8.7 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.7.7a2 2 0 0 1 1.7 2z" /></svg>
  {/if}
</span>

<style>
  .icon {
    flex: 0 0 auto;
    display: inline-flex;
    width: var(--size);
    height: var(--size);
    align-items: center;
    justify-content: center;
    transition:
      filter var(--yap-dur, 0.15s) ease,
      opacity var(--yap-dur, 0.15s) ease;
  }
  /* Greyed out with the switch beside it. */
  .icon.disabled {
    filter: grayscale(1);
    opacity: 0.5;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: calc(var(--size) * 0.22);
    user-select: none;
  }
  svg {
    width: 100%;
    height: 100%;
  }
  /* The letter badge: the app's initial, white on its colour. */
  .letter {
    display: flex;
    width: 100%;
    height: 100%;
    align-items: center;
    justify-content: center;
    border-radius: calc(var(--size) * 0.26);
    color: #fff;
    font-size: calc(var(--size) * 0.6);
    font-weight: 750;
    line-height: 1;
  }
</style>
