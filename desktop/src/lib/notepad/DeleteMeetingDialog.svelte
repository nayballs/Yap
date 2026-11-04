<script>
  // "Delete this meeting?" (Wispr's confirm: a scrim, a card scaling in). The
  // notepad lets go of the note and hides once it's gone (`yap-note-deleted`);
  // a recording into it stops first (`meeting_delete`).
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from '../ui/toast.svelte.js';

  let { open = $bindable(), noteId, recordingThis } = $props();

  let deleting = $state(false);
  async function deleteNote() {
    if (noteId == null || deleting) return;
    deleting = true;
    try {
      await invoke('meeting_delete', { noteId });
      open = false;
    } catch (e) {
      toast({ title: "Couldn't delete the meeting", description: String(e), variant: 'destructive' });
    } finally {
      deleting = false;
    }
  }
</script>

{#if open}
  <div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (open = false)}>
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="np-del-title" aria-describedby="np-del-body">
      <div>
        <h2 id="np-del-title">Delete this meeting?</h2>
        <p id="np-del-body">
          Its transcript, your notes and the summary go for good.{recordingThis ? ' The recording stops first.' : ''}
        </p>
      </div>
      <div class="dbtns">
        <button class="dbtn" onclick={() => (open = false)}>Cancel</button>
        <button class="dbtn danger" onclick={deleteNote} disabled={deleting}>{deleting ? 'Deleting…' : 'Delete'}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  button {
    font: inherit;
    cursor: pointer;
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--yap-paper-scrim);
    animation: bodyin 0.15s var(--yap-ease-spring);
  }
  @keyframes bodyin {
    from {
      opacity: 0;
    }
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 32px;
    width: min(360px, calc(100vw - 48px));
    box-sizing: border-box;
    padding: 24px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 12px;
    background: var(--yap-paper);
    box-shadow: var(--yap-shadow-dialog);
    animation: scalein 0.15s var(--yap-ease-spring) both;
  }
  @keyframes scalein {
    from {
      opacity: 0;
      transform: scale(0.9);
    }
  }
  .dialog h2 {
    margin: 0 0 8px;
    font-size: 17px;
    font-weight: 600;
    line-height: 24px;
  }
  .dialog p {
    margin: 0;
    font-size: 14px;
    line-height: 21px;
    color: var(--yap-paper-text-2);
  }
  .dbtns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .dbtn {
    height: 36px;
    padding: 0 16px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9999px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font-size: 14px;
    font-weight: 600;
  }
  .dbtn:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
  }
  .dbtn.danger {
    border-color: transparent;
    background: var(--yap-danger-fill);
    color: #fff;
  }
  .dbtn.danger:hover:not(:disabled) {
    background: var(--yap-danger-fill-hover);
  }
  .dbtn:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
