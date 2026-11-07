<script>
  // Settings → General → Markdown files (2026-11-07, notes/md_file.rs): Yap
  // opens .md and .markdown files in a window of its own (Preview / Source,
  // saved back to the file, following changes on disk).
  //
  // - "Open Markdown files with Yap": the installer offers Yap for both
  //   types ("Open with"), never makes it their default — Windows lets
  //   only the person do that. The button opens Windows' Default apps page
  //   (Yap's own page on it, `md_open_with_settings`), where you pick Yap
  //   once; the line says whether .md files open with Yap now
  //   (`md_open_with_state`), or that this copy isn't installed (a dev or
  //   portable Yap: Windows doesn't know it).
  // - "Open a Markdown file…": the file dialog, then its window.
  // Loaded on its own (GeneralPage's chunk stays as it was).
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Button from '../ui/Button.svelte';

  let assoc = $state(null);
  let failed = $state('');

  async function refresh() {
    try {
      assoc = await invoke('md_open_with_state');
    } catch {
      assoc = null;
    }
  }

  $effect(() => {
    refresh();
    // Back from Windows' Settings: what was picked there.
    const back = () => refresh();
    window.addEventListener('focus', back);
    return () => window.removeEventListener('focus', back);
  });

  const desc = $derived(
    assoc?.isDefault
      ? 'Yap opens .md and .markdown files. Change it on Windows’ Default apps page'
      : assoc && !assoc.registered
        ? 'This copy of Yap isn’t installed, so Windows doesn’t offer it for .md files. Installed, it does'
        : 'Windows asks you to pick Yap once: on its Default apps page, choose Yap for .md and .markdown'
  );

  function defaultApps() {
    failed = '';
    invoke('md_open_with_settings').catch((e) => (failed = String(e)));
  }

  async function openFile() {
    failed = '';
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({ multiple: false, directory: false, filters: [{ name: 'Markdown', extensions: ['md', 'markdown'] }] });
      if (typeof path !== 'string' || !path) return;
      await invoke('open_markdown', { path });
    } catch (e) {
      failed = String(e);
    }
  }
</script>

<Group title="Markdown files" id="settings-markdown">
  <Row label="Open Markdown files with Yap" {desc} setting="open-markdown-with">
    <Button variant="secondary" size="sm" onclick={defaultApps}>Open Default apps</Button>
  </Row>
  <Row
    label="Open a Markdown file…"
    desc={failed || 'In a Yap window: as a page or its Markdown, saved back to the file as you type'}
    setting="open-markdown-file"
  >
    <Button variant="secondary" size="sm" onclick={openFile}>Open a file…</Button>
  </Row>
</Group>
