// Small helpers for copy kept as HTML strings (FAQ answers, table cells).

/** A shortcut as keycaps, the way <Keys> draws it: key("Win+Alt+S"). */
export function key(combo: string): string {
  return `<span class="keys">${combo
    .split('+')
    .map((k) => `<kbd>${k.trim()}</kbd>`)
    .join('<span class="plus" aria-hidden="true">+</span>')}</span>`;
}

/** Plain text from an HTML string (structured data wants text). */
export function text(html: string): string {
  return html
    .replace(/<[^>]+>/g, '')
    .replace(/&nbsp;/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}
