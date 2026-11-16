<script>
  // **Research** (desktop/src-tauri/src/chat/research.rs, 16 Nov 2026; the Claude app's Research
  // mode): under the message box beside Style. On (in the accent, as the Claude app's blue
  // indicator): each question is researched — a short plan to approve first, then many searches and
  // page reads, then a long report as a document beside the chat, with numbered citations to the
  // pages read. Stays on until it's turned off. Greyed, with why, when Chat's model can't (web search
  // off, a model that can't call tools). Chat's side only.
  import { research, refreshResearch } from './chatResearch.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';

  /** `chat`: the view's state (its model's limits say when the model changed). */
  let { chat } = $props();

  $effect(() => {
    // Read so the effect runs again when the model (and so its limits) change.
    void chat.limits?.label;
    void chat.limits?.provider;
    refreshResearch();
  });

  async function flip() {
    await refreshResearch();
    if (!research.can) {
      research.on = false;
      toast({ title: "Research isn't available", description: research.why || "Chat's model can't research now." });
      return;
    }
    research.on = !research.on;
    chat.inputEl?.focus();
  }

  const title = $derived(
    !research.can && research.why
      ? research.why
      : research.on
        ? 'Research is on: each question gets a plan, many searches and a report beside the chat. Click to turn it off.'
        : 'Research: a plan, many searches and page reads, then a long report with citations'
  );
</script>

<button
  type="button"
  class="researchchip"
  class:on={research.on && research.can}
  class:off={!research.can}
  aria-pressed={research.on && research.can}
  aria-label="Research"
  {title}
  data-testid="chat-research-chip"
  onclick={flip}
>
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6" /><path d="m15 15 5 5" /><path d="M8 10.5h5M10.5 8v5" /></svg>
  <span class="rname">Research</span>
</button>

<style>
  .researchchip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    height: 26px;
    padding: 0 8px 0 6px;
    border: 1px solid transparent;
    border-radius: var(--yap-r-full);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--yap-dur);
  }
  .researchchip svg {
    flex: none;
    width: 13px;
    height: 13px;
  }
  .researchchip:hover {
    background: var(--yap-s3);
  }
  .researchchip.on {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
  }
  .researchchip.on svg {
    color: var(--yap-primary);
  }
  .researchchip.on .rname {
    font-weight: 600;
  }
  .researchchip.off {
    opacity: 0.6;
  }
  .researchchip:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: 1px;
  }
  .rname {
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
