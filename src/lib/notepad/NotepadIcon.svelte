<script>
  // The notepad's own icons (stroke style, 24 × 24), sized by the CSS of
  // where they sit: every part of the notepad styles them as `:global(svg)`.
  let { name } = $props();
</script>

<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  {#if name === 'back'}<path d="M15 18l-6-6 6-6" />
  {:else if name === 'play'}<path d="M8 5.5v13l11-6.5z" fill="currentColor" stroke-width="1.5" />
  {:else if name === 'stop'}<rect x="6.5" y="6.5" width="11" height="11" rx="2" fill="currentColor" stroke="none" />
  {:else if name === 'more'}<circle cx="5" cy="12" r="1.3" fill="currentColor" stroke="none" /><circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none" /><circle cx="19" cy="12" r="1.3" fill="currentColor" stroke="none" />
  {:else if name === 'split'}<rect x="3.5" y="4.5" width="17" height="15" rx="2.5" /><path d="M12 4.5v15" />
  {:else if name === 'share'}<path d="M12 15V3.5" /><path d="M7.5 8L12 3.5 16.5 8" /><path d="M5 12.5v5.5a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-5.5" />
  {:else if name === 'link'}<path d="M10 14a4.5 4.5 0 0 0 6.4 0l2.8-2.8a4.5 4.5 0 0 0-6.4-6.4l-1.2 1.2" /><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-2.8 2.8a4.5 4.5 0 0 0 6.4 6.4l1.2-1.2" />
  {:else if name === 'check'}<path d="M5 12.5l4.5 4.5L19 7.5" />
  {:else if name === 'min'}<path d="M5 12h14" />
  {:else if name === 'max'}<rect x="5" y="5" width="14" height="14" rx="1.5" />
  {:else if name === 'restore'}<path d="M8 8V5.5a1 1 0 0 1 1-1h9.5a1 1 0 0 1 1 1V15a1 1 0 0 1-1 1H16" /><rect x="4.5" y="8" width="11.5" height="11.5" rx="1" />
  {:else if name === 'close'}<path d="M6 6l12 12M18 6L6 18" />
  {:else if name === 'clock'}<circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" />
  {:else if name === 'search'}<circle cx="11" cy="11" r="6" /><path d="M20 20l-4.5-4.5" />
  {:else if name === 'person'}<circle cx="12" cy="8.5" r="3.5" /><path d="M5 19.5a7 7 0 0 1 14 0" />
  {:else if name === 'copy'}<rect x="8.5" y="8.5" width="11" height="11" rx="2.5" /><path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" />
  {:else if name === 'x'}<path d="M7 7l10 10M17 7L7 17" />
  {:else if name === 'pause'}<rect x="7" y="5" width="3.2" height="14" rx="1" /><rect x="13.8" y="5" width="3.2" height="14" rx="1" />
  {:else if name === 'mic'}<rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5.5 11a6.5 6.5 0 0 0 13 0" /><path d="M12 17.5V21" />
  {:else if name === 'plus'}<path d="M12 5v14M5 12h14" />
  {:else if name === 'markdown'}<path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z" /><path d="M14 3.5v5h5" /><path d="M9 13.5h6M9 17h4" />
  {:else if name === 'text'}<path d="M5 6.5h14M5 12h14M5 17.5h9" />
  {:else if name === 'audio'}<rect x="9" y="3" width="6" height="10" rx="3" /><path d="M5.5 10.5a6.5 6.5 0 0 0 13 0" /><path d="M12 17v3.5" /><path d="M8.5 20.5h7" />
  {:else if name === 'save'}<path d="M12 4v11" /><path d="M7.5 10.5L12 15l4.5-4.5" /><path d="M5 19.5h14" />
  {:else if name === 'redo'}<path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 5v6h-6" />
  {:else if name === 'mail'}<rect x="3.5" y="5.5" width="17" height="13" rx="2" /><path d="M4 7l8 6 8-6" />
  {:else if name === 'trash'}<path d="M4.5 7h15" /><path d="M9.5 7V5a1 1 0 0 1 1-1h3a1 1 0 0 1 1 1v2" /><path d="M6.5 7l.8 12a1.5 1.5 0 0 0 1.5 1.4h6.4a1.5 1.5 0 0 0 1.5-1.4l.8-12" />
  {:else if name === 'newchat'}<path d="M12 4.5H6.5a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V12" /><path d="M17.5 3.5a2.1 2.1 0 0 1 3 3L13 14l-4 1 1-4z" />
  {:else if name === 'up'}<path d="M7.5 10.5v9H5a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1z" /><path d="M7.5 10.5l3.6-6.3a1.6 1.6 0 0 1 2.9 1.2l-.8 3.6h5.1a2 2 0 0 1 2 2.3l-1.1 6.4a2 2 0 0 1-2 1.7H7.5" />
  {:else if name === 'down'}<path d="M7.5 13.5v-9H5a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1z" /><path d="M7.5 13.5l3.6 6.3a1.6 1.6 0 0 0 2.9-1.2l-.8-3.6h5.1a2 2 0 0 0 2-2.3l-1.1-6.4a2 2 0 0 0-2-1.7H7.5" />
  {:else if name === 'send'}<path d="M12 19V5.5" /><path d="M6.5 11L12 5.5 17.5 11" />
  {:else if name === 'warn'}<path d="M12 9.5v4" /><path d="M12 17h.01" /><path d="M10.3 4.2L2.6 17.5a2 2 0 0 0 1.7 3h15.4a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z" />
  {:else if name === 'retry'}<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3" /><path d="M19.5 4.5V9H15" />
  {:else if name === 'note'}<path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z" /><path d="M14 3.5v5h5" />
  {:else if name === 'chat'}<path d="M20 11.5a7.5 7.5 0 0 1-10.9 6.7L4.5 19.5l1.3-4.3A7.5 7.5 0 1 1 20 11.5z" />
  {:else if name === 'chevron'}<path d="M6 9l6 6 6-6" />
  {:else if name === 'open'}<path d="M13.5 4.5h6v6" /><path d="M19.5 4.5L11 13" /><path d="M17.5 13.5v4a2 2 0 0 1-2 2h-9a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2h4" />
  {:else if name === 'pencil'}<path d="M4.5 19.5h4l10-10a2.8 2.8 0 0 0-4-4l-10 10z" /><path d="M13.5 6.5l4 4" />
  {/if}
</svg>
