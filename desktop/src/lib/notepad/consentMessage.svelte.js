// The consent message (Settings → Meetings' `meetingConsentMessage`): "Learn
// more" on the consent line opens it — why, and a message for the meeting
// chat, editable, saved as the popover closes or the box is left (empty =
// Yap's default); the ⋯ menu copies the saved one.
import { invoke } from '@tauri-apps/api/core';
import { consentMessage, consentToSave } from '../core/notepadText.js';
import { toast } from '../ui/toast.svelte.js';
import { copyToClipboard } from './actions.js';

export class ConsentMessage {
  /** As saved ('' = the default). */
  saved = $state('');
  /** The message in the popover's box. */
  draft = $state('');
  #onsaved;

  /** `onsaved(message)`: the notepad's copy of the settings follows. */
  constructor(onsaved) {
    this.#onsaved = onsaved;
  }

  /** The popover opens on the saved message (or Yap's default). */
  fill() {
    this.draft = consentMessage(this.saved);
  }

  async save() {
    const message = consentToSave(this.draft);
    if (message === this.saved) return;
    this.saved = message;
    this.#onsaved?.(message);
    try {
      await invoke('notepad_consent_message', { message });
    } catch (e) {
      toast({ title: "Couldn't save the message", description: String(e), variant: 'destructive' });
    }
  }

  /** Copy the message being edited, or (from the ⋯ menu, `saved`) the saved one. */
  async copy(saved) {
    let text;
    if (saved) {
      text = consentMessage(this.saved);
    } else {
      await this.save();
      text = consentMessage(this.draft);
    }
    if (await copyToClipboard(text)) {
      toast({ title: 'Copied: paste it into the meeting chat', variant: 'success' });
    }
  }
}
