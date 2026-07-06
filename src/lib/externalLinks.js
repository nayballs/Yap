// External-link helper, ported from OpenWhispr's utils/externalLinks.ts.
// A plain target="_blank" anchor does nothing inside a Tauri webview (new-window
// requests are denied), so every external link routes through the opener plugin
// — falling back to window.open when running outside Tauri (plain-browser dev).
export async function openExternalLink(url) {
  try {
    if ('__TAURI_INTERNALS__' in window) {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(url);
      return;
    }
  } catch {
    /* plugin missing/denied — fall through to window.open */
  }
  window.open(url, '_blank', 'noopener,noreferrer');
}

export function createExternalLinkHandler(url) {
  return (e) => {
    e.preventDefault();
    openExternalLink(url);
  };
}
