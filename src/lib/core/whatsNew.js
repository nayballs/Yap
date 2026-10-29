// New in Yap, newest first: what Home's welcome back card says is new to
// someone back after a while (views/home/WelcomeBack.svelte). Add a line
// when a feature people would want to hear about lands; keep each to one
// short sentence. `date` is when it landed (YYYY-MM-DD); `settings` opens a
// Settings page (an anchor after #), `view` a sidebar view.

export const WHATS_NEW = [
  {
    date: '2026-10-28',
    title: 'Quick note',
    text: 'A small window to jot or dictate a note from any app, saved as you type.',
    action: { label: 'Give it a shortcut', settings: 'shortcuts#shortcut-quick-note' },
  },
  {
    date: '2026-10-24',
    title: 'Transforms',
    text: 'Polish, shorten or translate the text you select, or your last dictation, with one key.',
    action: { label: 'Set up transforms', settings: 'commands#transforms' },
  },
  {
    date: '2026-10-23',
    title: 'Snip to Chat',
    text: 'Drag a box over anything on screen and ask Chat about it.',
    action: { label: 'Open Chat', view: 'chat' },
  },
  {
    date: '2026-10-22',
    title: 'Read aloud, anywhere',
    text: 'Select text in any app and Yap reads it to you, a word at a time.',
    action: { label: 'Its shortcut', settings: 'shortcuts#shortcut-read' },
  },
];

/**
 * The one thing to tell someone whose last dictation was on `since`
 * (YYYY-MM-DD): the newest thing that landed after it (`isNew`), else the
 * newest of all, as something to try. Null when the list is empty.
 */
export function oneNewThing(since, list = WHATS_NEW) {
  const fresh = list.find((n) => !since || n.date > since);
  if (fresh) return { ...fresh, isNew: true };
  return list[0] ? { ...list[0], isNew: false } : null;
}
