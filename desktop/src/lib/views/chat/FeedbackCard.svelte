<script>
  // **👎's card** (16 Nov 2026, chat/feedback.rs; the Claude app's "What was wrong?"): under an answer marked not
  // good. Yap is local, so it goes to no one: what's picked here is kept with the answer on this PC and put to use —
  // **Retry with this note** asks the question again with what was wrong given to the model (a new version, both
  // kept, ‹ 2/2 ›; in a Code chat through Rewind), and **Learn from this** (a plain chat only) keeps a line in memory's
  // Preferences or switches the chat's style, each only on the person's click. Reasons are kept as they're picked,
  // the line as it's left (Enter, leaving the box, Done, Retry). Esc or × closes it; the 👎 stays.
  //
  // `value`: the answer's mark (`{rating, reasons, note}`); `onchange(fb)` keeps a new one (its promise: kept);
  // `retry`: `{why, onretry}` (ChatThread's `retryOf`; none: no Retry here); `learn`: `{conversation, onremember(fact)}`
  // (none: a Code chat or an incognito one); `onclose()`.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { toast } from '../../ui/toast.svelte.js';

  let { value = null, retry = null, learn = null, onchange, onclose } = $props();

  // The card's reasons (chat/feedback.rs `REASONS`, in its order).
  const REASONS = [
    { key: 'untrue', label: 'Not true' },
    { key: 'ignored', label: 'Didn’t do what I asked' },
    { key: 'long', label: 'Too long' },
    { key: 'short', label: 'Too short' },
    { key: 'tone', label: 'Wrong tone' },
    { key: 'other', label: 'Other' },
  ];
  const NOTE_MAX = 300;
  // What Learn from this offers for a reason: a line for memory, and a style of Yap's own (chat/styles.rs).
  const LEARN = {
    long: { fact: 'Prefers shorter, more direct answers', style: { id: 'concise', name: 'Concise' } },
    short: { fact: 'Prefers fuller, more detailed answers', style: { id: 'explanatory', name: 'Explanatory' } },
  };

  // (Its mark as it opened: what's picked after is this card's own.)
  // svelte-ignore state_referenced_locally
  let reasons = $state([...(value?.reasons || [])]);
  // svelte-ignore state_referenced_locally
  let note = $state(value?.note || '');
  let el = $state(null);
  let noteEl = $state(null);
  let saving = Promise.resolve();
  let keptNote = '';

  onMount(() => {
    keptNote = note.trim();
    el?.scrollIntoView?.({ block: 'nearest' });
    el?.querySelector('.fbchip')?.focus({ preventScroll: true });
  });

  /** The mark kept as the card has it. */
  function save() {
    keptNote = note.trim();
    saving = Promise.resolve(onchange?.({ rating: 'bad', reasons: [...reasons], note: keptNote })).catch(() => {});
    return saving;
  }
  /** The line kept when it changed. */
  function saveNote() {
    if (note.trim() !== keptNote) save();
  }

  function toggle(key) {
    const on = !reasons.includes(key);
    reasons = on ? [...reasons, key] : reasons.filter((k) => k !== key);
    save();
    if (key === 'other' && on) setTimeout(() => noteEl?.focus({ preventScroll: true }), 0);
  }

  async function retryNow() {
    if (!retry || retry.why) return;
    if (note.trim() !== keptNote) save();
    await saving;
    // (The latest Retry: the one that knows the answer is marked 👎 now.)
    const go = retry?.onretry;
    onclose?.();
    go?.();
  }

  function done() {
    saveNote();
    onclose?.();
  }

  function key(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      done();
    }
  }
  function noteKey(e) {
    if (e.key === 'Enter' && !e.isComposing) {
      e.preventDefault();
      saveNote();
    }
  }

  // ---- Learn from this (a plain chat only: memory.md, "Code chats keep memory out")
  const offer = $derived(reasons.map((r) => LEARN[r]).find(Boolean) ?? null);
  const learnable = $derived(!!learn && (!!offer || !!note.trim()));
  let learning = $state(false);
  let fact = $state('');
  let remembered = $state(false);
  let styleNow = $state('');
  let styled = $state(false);

  async function openLearn() {
    saveNote();
    fact = offer?.fact || note.trim();
    learning = true;
    remembered = false;
    styled = false;
    if (offer?.style && learn?.conversation != null) {
      try {
        styleNow = (await invoke('chat_style_of', { conversation: learn.conversation })) || 'normal';
      } catch {
        styleNow = '';
      }
    }
  }

  async function remember() {
    const words = fact.trim();
    if (!words || remembered) return;
    remembered = !!(await learn?.onremember?.(words));
  }

  async function useStyle() {
    const s = offer?.style;
    if (!s || learn?.conversation == null) return;
    try {
      styleNow = await invoke('chat_style_set', { conversation: learn.conversation, style: s.id });
      styled = true;
      // The Style chip under the message box follows (StyleChip.svelte).
      window.dispatchEvent(new CustomEvent('yap-chat-style', { detail: { conversation: learn.conversation, style: styleNow } }));
      toast({ title: `This chat’s answers are ${s.name} now`, description: 'Change it any time from Style under the message box.', variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't change the style", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="fbcard" bind:this={el} role="group" aria-label="What was wrong with this answer?" data-testid="chat-feedback-card" onkeydown={key}>
  <div class="fbhead">
    <span class="fbtitle">What was wrong?</span>
    <button type="button" class="fbx" aria-label="Close" title="Close (Esc)" data-testid="chat-feedback-close" onclick={done}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
  </div>
  <div class="fbreasons" role="group" aria-label="Reasons">
    {#each REASONS as r (r.key)}
      <button type="button" class="fbchip" class:on={reasons.includes(r.key)} aria-pressed={reasons.includes(r.key) ? 'true' : 'false'} data-testid={`chat-feedback-reason-${r.key}`} onclick={() => toggle(r.key)}>
        {#if reasons.includes(r.key)}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
        {/if}
        {r.label}
      </button>
    {/each}
  </div>
  {#if reasons.includes('other') || note}
    <input
      class="fbnote"
      bind:this={noteEl}
      bind:value={note}
      maxlength={NOTE_MAX}
      placeholder="What should it have done?"
      aria-label="What should it have done?"
      data-testid="chat-feedback-note"
      onblur={saveNote}
      onkeydown={noteKey}
    />
  {/if}
  {#if learning}
    <div class="fblearn" data-testid="chat-feedback-learn-row">
      <label class="fbfact">
        <span>Remember in every chat</span>
        <input bind:value={fact} maxlength="140" disabled={remembered} data-testid="chat-feedback-fact" />
      </label>
      <div class="fblearnbtns">
        <button type="button" class="mbtn" disabled={!fact.trim() || remembered} data-testid="chat-feedback-remember" onclick={remember}>
          {remembered ? 'Remembered' : 'Remember'}
        </button>
        {#if offer?.style && styleNow && (styled || styleNow !== offer.style.id)}
          <button type="button" class="mbtn" disabled={styled} data-testid="chat-feedback-style" onclick={useStyle}>
            {styled ? `${offer.style.name} in this chat` : `Use ${offer.style.name} in this chat`}
          </button>
        {/if}
      </div>
      <p class="fbsmall">Memory keeps it under Preferences (Settings → Memory), where you can change or remove it.</p>
    </div>
  {/if}
  <div class="fbfoot">
    <span class="fbsmall fbprivate">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></svg>
      Kept on this PC: Yap sends it nowhere.
    </span>
    {#if learnable && !learning}
      <button type="button" class="fblink" data-testid="chat-feedback-learn" onclick={openLearn}>Learn from this</button>
    {/if}
    <button type="button" class="mbtn" data-testid="chat-feedback-done" onclick={done}>Done</button>
    {#if retry}
      <button
        type="button"
        class="mbtn primary"
        aria-disabled={retry.why ? 'true' : undefined}
        class:off={!!retry.why}
        title={retry.why || 'Answer it again with what you said was wrong (this answer is kept)'}
        data-testid="chat-feedback-retry"
        onclick={retryNow}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-15.5 6.2L3 16" /><path d="M3 21v-5h5" /></svg>
        Retry with this note
      </button>
    {/if}
  </div>
</div>

<style>
  .fbcard {
    align-self: flex-start;
    box-sizing: border-box;
    width: min(100%, 520px);
    max-width: 78%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0 0 4px 2px;
    padding: 10px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s1);
    font-size: 12px;
  }
  .fbhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .fbtitle {
    font-weight: 650;
    color: var(--yap-fg);
  }
  .fbx {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .fbx:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .fbx svg {
    width: 13px;
    height: 13px;
  }
  .fbreasons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .fbchip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .fbchip:hover {
    border-color: var(--yap-border-hover);
  }
  .fbchip.on {
    border-color: color-mix(in srgb, var(--yap-primary) 55%, var(--yap-border-subtle));
    background: var(--yap-primary-wash);
    font-weight: 600;
  }
  .fbchip svg {
    width: 11px;
    height: 11px;
    color: var(--yap-primary);
  }
  .fbnote,
  .fbfact input {
    box-sizing: border-box;
    width: 100%;
    height: 30px;
    padding: 0 9px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-bg);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
  }
  .fbnote:focus,
  .fbfact input:focus {
    outline: none;
    border-color: var(--yap-primary-line);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .fblearn {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 0 0;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .fbfact {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--yap-fg-62);
    font-weight: 600;
  }
  .fblearnbtns {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .fbsmall {
    margin: 0;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .fbfoot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
  }
  .fbprivate {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-right: auto;
  }
  .fbprivate svg {
    width: 11px;
    height: 11px;
  }
  .fblink {
    padding: 0 4px;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .fblink:hover {
    text-decoration: underline;
  }
  .mbtn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 11px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .mbtn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .mbtn.primary {
    border-color: transparent;
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .mbtn.primary:hover:not(:disabled):not(.off) {
    filter: brightness(1.06);
  }
  .mbtn:disabled,
  .mbtn.off {
    opacity: 0.5;
    cursor: default;
  }
  .mbtn svg {
    width: 11px;
    height: 11px;
  }
  .fbchip:focus-visible,
  .mbtn:focus-visible,
  .fbx:focus-visible,
  .fblink:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
