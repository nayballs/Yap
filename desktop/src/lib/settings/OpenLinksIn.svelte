<script>
  // Settings → General → Links (2026-11-01, shell/browsers.rs): "Open links
  // in" — Windows' default browser, or one of the browsers installed
  // (Windows' registered browsers, read from the registry, with their own
  // icons). Every link button in Yap follows it (`open_external`): Sources,
  // the pictures' and pages' "Open in …", Chat's offered buttons, release
  // notes. Loaded on its own (GeneralPage's chunk stays as it was).
  //
  //   cfg: Settings' config $state (auto-save does the saving).
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Select from '../ui/Select.svelte';

  let { cfg } = $props();

  let list = $state({ browsers: [], defaultName: null });
  let loaded = $state(false);

  $effect(() => {
    invoke('browsers_list')
      .then((l) => (list = { browsers: l?.browsers || [], defaultName: l?.defaultName || null }))
      .catch(() => {})
      .finally(() => (loaded = true));
  });

  const options = $derived([
    { value: '', label: list.defaultName ? `Default browser (${list.defaultName})` : 'Default browser' },
    ...list.browsers.map((b) => ({ value: b.id, label: b.name })),
    // A browser chosen before and uninstalled since: still named, so the
    // choice isn't silently changed (links go to the default meanwhile).
    ...(cfg.openLinksIn && loaded && !list.browsers.some((b) => b.id === cfg.openLinksIn) ? [{ value: cfg.openLinksIn, label: `${cfg.openLinksIn} (not found)` }] : []),
  ]);
  const chosen = $derived(list.browsers.find((b) => b.id === cfg.openLinksIn) ?? null);
  const desc = $derived(
    cfg.openLinksIn && loaded && !chosen
      ? "That browser isn't installed any more: links open in Windows' default"
      : 'Links in Chat, notes and Settings, and Chat’s “Open in …” buttons'
  );
</script>

<Group title="Links" id="settings-links">
  <Row label="Open links in" {desc} setting="open-links-in">
    <span class="browserpick">
      {#if chosen?.icon}<img class="browsericon" src={chosen.icon} alt="" aria-hidden="true" />{/if}
      <Select bind:value={cfg.openLinksIn} {options} ariaLabel="Open links in" />
    </span>
  </Row>
</Group>

<style>
  .browserpick {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .browsericon {
    width: 20px;
    height: 20px;
  }
</style>
