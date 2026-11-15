<script>
  // **A message waiting for Claude** (14 Nov 2026; the maintainer, from Claude Code's own app: a message sent while
  // Claude works waits as a bubble of its own). It goes on its own — Claude reads it at its next step, or it's the
  // next turn once this one's answer is in (chatState's queue) — or now (Send now); × takes it back into the box to
  // edit (Cancel and edit); Copy copies its words.
  import { toast } from '../../ui/toast.svelte.js';

  /** `q`: the waiting message ({ qid, text, voice, at, later?, plain? }: `voice`, asked out loud or dictated — the mic
   * as on a sent one; `later`, queued on purpose with Ctrl+Enter; `plain`, waiting for an answer of Chat's own, which
   * nothing can take sooner: no Send now); `oncancel(qid)`, `onsend(qid)`. */
  let { q, oncancel, onsend } = $props();

  async function copy() {
    try {
      await navigator.clipboard.writeText(q.text);
      toast({ title: 'Copied' });
    } catch {
      /* the clipboard said no */
    }
  }
</script>

<div class="queued" data-testid="chat-queued" data-qid={q.qid}>
  <!-- prettier-ignore -->
  <div class="qbubble">{#if q.voice}<span class="viavoice" title="Asked by voice" aria-label="Asked by voice" data-testid="chat-queued-voice"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg></span>{/if}{q.text}</div>
  <div class="qfoot">
    <span
      class="qstate"
      data-later={q.later ? 'true' : undefined}
      title={q.later ? 'Queued for later: it goes as its own message once this answer is in' : 'Claude reads it at its next step, or as its next message once this answer is in'}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></svg>
      Queued
    </span>
    <button type="button" class="qbtn" onclick={copy} aria-label="Copy" title="Copy">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></svg>
    </button>
    <button type="button" class="qbtn" onclick={() => oncancel(q.qid)} aria-label="Cancel and edit message" title="Cancel and edit message" data-testid="chat-queued-cancel">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
    </button>
    <!-- (Waiting for an answer of Chat's own, `plain`: nothing can take it before that answer is in.) -->
    {#if !q.plain}
      <button type="button" class="qsend" onclick={() => onsend(q.qid)} title="Send it to Claude now" data-testid="chat-queued-send-now">Send now</button>
    {/if}
  </div>
</div>

<style>
  .queued {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    margin: 6px 0 2px auto;
    max-width: 85%;
  }
  .qbubble {
    padding: 8px 12px;
    border: 1px dashed var(--yap-paper-rule);
    border-radius: var(--yap-r-lg);
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    opacity: 0.85;
  }
  /* Asked by voice: the mic a sent message shows (ChatMessage's). */
  .viavoice {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 5px;
    color: var(--yap-primary);
  }
  .viavoice svg {
    width: 11px;
    height: 11px;
  }
  .qfoot {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .qstate {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-right: 4px;
  }
  .qstate svg,
  .qbtn svg {
    width: 14px;
    height: 14px;
  }
  .qbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .qbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .qsend {
    padding: 3px 8px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .qsend:hover {
    background: var(--yap-paper-s2);
  }
  .qbtn:focus-visible,
  .qsend:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
