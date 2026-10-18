// What the right-click menus on Chat's messages say and copy, shared by the
// Chat view, the Notes view's Ask thread and the meeting notepad's Ask sheet
// (the menus themselves: ui/contextMenu.svelte.js). One order everywhere: the
// main action first, Copy-type ones next, anything destructive last.
import { markdownToText } from './markdown.js';
import { toast } from './ui/toast.svelte.js';

/** Copy `text`; a toast says it (the menu has no ✓ of its own to show). */
export async function copyText(text, title = 'Copied') {
  try {
    await navigator.clipboard.writeText(text);
    toast({ title, variant: 'success' });
    return true;
  } catch {
    toast({ title: "Couldn't copy", description: 'The clipboard is busy. Try again.', variant: 'destructive' });
    return false;
  }
}

/**
 * A chat message's menu items. An answer (`role` 'assistant'): Copy (as
 * text: markdown marks dropped; an email, `plain`, exactly as shown), Copy
 * as Markdown (not for `plain`), and ▶ Read aloud / ■ Stop reading when
 * `read` is given (`reading`: it's being read now). A question: Copy.
 */
export function messageItems({ role, text, plain = false }, { read = null, reading = false } = {}) {
  if (!text?.trim()) return [];
  if (role !== 'assistant') return [{ label: 'Copy', icon: 'copy', onselect: () => copyText(text) }];
  const items = [{ label: 'Copy', icon: 'copy', onselect: () => copyText(plain ? text : markdownToText(text)) }];
  if (!plain) items.push({ label: 'Copy as Markdown', icon: 'copy', onselect: () => copyText(text, 'Copied as Markdown') });
  if (read) {
    items.push({ separator: true });
    items.push(
      reading
        ? { label: 'Stop reading', icon: 'stop', onselect: read }
        : { label: 'Read aloud', icon: 'play', onselect: read }
    );
  }
  return items;
}

/** A whole conversation as Markdown: its title, then each turn. */
export function conversationMarkdown(conv) {
  const title = conv?.title?.trim() || 'Untitled';
  let out = `# ${title}\n\n`;
  for (const m of conv?.messages || []) {
    const text = (m.text || '').trim();
    if (!text) continue;
    out += `**${m.role === 'assistant' ? 'Yap' : 'You'}:** ${text}\n\n`;
  }
  return out.trimEnd() + '\n';
}
