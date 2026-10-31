<script>
  // Think (src-tauri/src/chat/think.rs): the next question thought through,
  // one question at a time — the on-device Qwen thinks before it answers.
  // Shown only for the model on this PC: a cloud model's thinking is its
  // effort, in the model picker (ai/effort.rs). Asked again whenever Chat's
  // model changes (its limits change with it). Kept apart from the prompt
  // chips and Skills.
  import { think, refreshThink } from './chatThink.svelte.js';

  /** `chat`: the view's state (its model's limits say when the model changed). */
  let { chat } = $props();

  $effect(() => {
    // Read so the effect runs again when the model (and so its limits) change.
    void chat.limits?.label;
    void chat.limits?.provider;
    refreshThink();
  });

  const title = 'Think it through: the model on this PC thinks before it answers (a few seconds more). This question only.';
</script>

{#if think.can}
  <button
    type="button"
    class="thinkbtn"
    class:on={think.on}
    aria-pressed={think.on}
    aria-label="Think it through"
    {title}
    onclick={() => {
      think.on = !think.on;
      chat.inputEl?.focus();
    }}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 18h6" /><path d="M10 21h4" /><path d="M12 3a6 6 0 0 0-3.6 10.8c.6.5 1 1.2 1.1 2V16h5v-.2c.1-.8.5-1.5 1.1-2A6 6 0 0 0 12 3z" /></svg>
    <span class="thinklabel">Think</span>
  </button>
{/if}

<style>
  .thinkbtn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding: 0 9px 0 7px;
    border: 1px solid transparent;
    border-radius: var(--yap-r-full, 999px);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    flex: 0 0 auto;
  }
  .thinkbtn svg {
    width: 15px;
    height: 15px;
  }
  .thinkbtn:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .thinkbtn.on {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
  }
  .thinkbtn.on svg {
    color: var(--yap-primary);
  }
  .thinkbtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
</style>
