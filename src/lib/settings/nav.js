// Settings' pages and how links to them land (2026-10-22 review: the nav
// regrouped by scope, Claude-style, with General split into General,
// Dictation and Meetings, and Advanced and History dissolved).
//
// Pure data and functions (no Svelte), shared by Settings.svelte, the search
// index (settingsIndex.js) and the e2e suite.

/** Nav groups, in order. `label: null` = the first, unlabelled group. */
export const NAV_GROUPS = [
  {
    label: null,
    items: [
      { id: 'general', label: 'General' },
      { id: 'dictation', label: 'Dictation' },
      { id: 'shortcuts', label: 'Shortcuts' },
      { id: 'meetings', label: 'Meetings' },
    ],
  },
  {
    // Commands make the AI *do* something and type it where you are; the
    // Voice Assistant answers out loud; Memory is what both (and Chat) know
    // about you (memory.rs, MemorySection.svelte).
    label: 'Assistant',
    items: [
      { id: 'commands', label: 'Commands' },
      { id: 'voice', label: 'Voice Assistant' },
      { id: 'memory', label: 'Memory' },
    ],
  },
  {
    label: 'AI models',
    items: [
      { id: 'models', label: 'Speech-to-Text' },
      { id: 'cleanup', label: 'Language Models' },
    ],
  },
  {
    label: 'Connections',
    items: [
      { id: 'connectors', label: 'Connectors' },
      { id: 'mcp', label: 'AI apps (MCP)' },
    ],
  },
  {
    label: 'This PC',
    items: [
      { id: 'privacy', label: 'Privacy & data' },
      { id: 'about', label: 'About' },
    ],
  },
];

/** Every page id the nav shows, plus Account (the nav's footer). */
export const PAGE_IDS = [...NAV_GROUPS.flatMap((g) => g.items.map((i) => i.id)), 'account'];

/** id → the page's name, as the nav and the search results say it. */
export const PAGE_LABELS = Object.fromEntries([
  ...NAV_GROUPS.flatMap((g) => g.items.map((i) => [i.id, i.label])),
  ['account', 'Account'],
]);

// Old links, kept working (OpenWhispr's SECTION_ALIASES): Windows
// notifications already in the notification centre carry the string they
// were posted with (calls/meeting_guard.rs: "general#screen-sharing"), and every
// `yap-settings-goto` / `yap-open-settings` / openSettings() caller from
// before the split. A page alias maps every anchor on it; a full
// "page#anchor" alias wins over its page's.
export const ALIASES = {
  'general#meetings': 'meetings',
  'general#screen-sharing': 'meetings#screen-sharing',
  'general#activation': 'dictation#hotkey',
  'general#hotkey': 'dictation#hotkey',
  'general#microphone': 'dictation#microphone',
  'general#audio': 'dictation#microphone',
  history: 'privacy#history',
  advanced: 'dictation#typing',
  'advanced#output': 'dictation#typing',
  'advanced#system': 'general#startup',
  'advanced#debug': 'privacy#logs',
  'advanced#dictionary': 'dictation#dictionary',
};

/**
 * Where a link lands: "page", "page#anchor", or an object from the search
 * ({ page, anchor, scope }). Aliases first. `cleanup#<scope>` opens one of
 * Language Models' tabs (the Chat view's model chip: "cleanup#chat").
 * Returns null for a page this build doesn't have (stay put).
 * @param {string | {page: string, anchor?: string, scope?: string}} target
 * @param {string[]} [scopes] Language Models' tab ids
 */
export function resolveTarget(target, scopes = []) {
  let page;
  let anchor = '';
  let scope = '';
  if (target && typeof target === 'object') {
    page = String(target.page || '');
    anchor = String(target.anchor || '');
    scope = String(target.scope || '');
  } else if (typeof target === 'string' && target) {
    [page, anchor = ''] = (ALIASES[target] ?? target).split('#');
    if (!ALIASES[target] && ALIASES[page]) {
      // A page that moved ("history", "advanced#x"): to where it went, and
      // to the anchor asked for if there was one.
      const [to, toAnchor = ''] = ALIASES[page].split('#');
      page = to;
      anchor = anchor || toAnchor;
    }
  } else {
    return null;
  }
  if (!PAGE_IDS.includes(page)) return null;
  if (page === 'cleanup' && anchor && scopes.includes(anchor)) {
    scope = anchor;
    anchor = '';
  }
  return { page, anchor, scope };
}
