// **A chat's Style** (chat/styles.rs, 15 Nov 2026; the Claude app's styles): how its answers are written — Normal,
// Concise, Explanatory, Formal, or the person's own. Picked from the message box's + menu (Style ›, 16 Nov 2026, as
// the Claude app tucks it in its box's menu), shown as a chip in the box while it isn't Normal ("Concise ×"), and
// from Settings → Skills' "Chat styles" (StylesDialog.svelte). One per chat (`chat_style_of`); a new chat starts
// with the last one picked (`conversation` null). Chat's side only: a Code chat's answers are Claude Code's.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';

export class ChatStyle {
  /** Yap's four, then the person's own (`chat_styles`). */
  builtins = $state([]);
  mine = $state([]);
  /** The chat's style id ('normal' for none). */
  current = $state('normal');
  /** The chat it's for (null: a new chat). */
  conversation = $state(null);
  all = $derived([...this.builtins, ...this.mine]);
  /** Its name ('' while the list isn't read yet). */
  name = $derived(this.all.find((s) => s.id === this.current)?.name || (this.current === 'normal' ? 'Normal' : ''));

  /** Read the styles (again: one made, edited or deleted). */
  async loadList() {
    try {
      const out = await invoke('chat_styles');
      this.builtins = out?.builtins || [];
      this.mine = out?.styles || [];
    } catch {
      // (Unread: a chip waits for its name.)
    }
  }

  /** The chat shown now (null: a new chat): its style. */
  async follow(conversation) {
    this.conversation = conversation;
    try {
      const s = await invoke('chat_style_of', { conversation });
      if (conversation === this.conversation) this.current = s || 'normal';
    } catch {
      if (conversation === this.conversation) this.current = 'normal';
    }
  }

  /** Style `id` for the chat shown (kept as the last picked: a new chat starts with it). */
  async pick(id) {
    const conversation = this.conversation;
    try {
      const now = await invoke('chat_style_set', { conversation, style: id });
      if (conversation === this.conversation) this.current = now || 'normal';
      return true;
    } catch (e) {
      toast({ title: "Couldn't change the style", description: String(e), variant: 'destructive' });
      await this.loadList();
      await this.follow(this.conversation);
      return false;
    }
  }
}
