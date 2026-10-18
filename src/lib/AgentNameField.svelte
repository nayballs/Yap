<script>
  // The Voice Agent's name — the wake word ("Yap, …" in a dictation runs it
  // as a command; agent_detect.rs). Settings → Voice Agent. Moved out of
  // Language Models → Voice Agent (OpenWhispr's DictationAgentSettings "Agent
  // Name"), which now points here.
  //
  //   cfg: the Settings config $state (cfg.agentName + cfg.dictionary).
  import Input from './ui/Input.svelte';
  import { untrack } from 'svelte';

  let { cfg } = $props();

  const displayName = $derived((cfg.agentName || '').trim() || 'Yap');

  // Local editor state for the name field, seeded once from the saved name.
  let agentInput = $state(untrack(() => (cfg.agentName || '').trim()));
  let saved = $state(false);

  function saveAgentName() {
    const trimmed = agentInput.trim();
    if (!trimmed) return;
    const prev = (cfg.agentName || '').trim();

    cfg.agentName = trimmed;
    agentInput = trimmed;

    // Keep the agent's name in the correction dictionary so STT spells it right.
    // Yap's dictionary is a from→to map (config.rs DictionaryEntry); we store the
    // name as a self-mapping entry ({ from: name, to: name }) — a known term the
    // pipeline preserves. Drop the previous name's entry when it's being renamed.
    if (!Array.isArray(cfg.dictionary)) cfg.dictionary = [];
    let dict = cfg.dictionary;
    // (Snippets share the list; a snippet's phrase is never the name's entry.)
    if (prev && prev.toLowerCase() !== trimmed.toLowerCase()) {
      dict = dict.filter((e) => e?.snippet || (e?.from || '').trim().toLowerCase() !== prev.toLowerCase());
    }
    const has = dict.some((e) => !e?.snippet && (e?.from || '').trim().toLowerCase() === trimmed.toLowerCase());
    if (!has) dict = [{ from: trimmed, to: trimmed }, ...dict];
    cfg.dictionary = dict;

    saved = true;
    setTimeout(() => (saved = false), 2500);
  }
</script>

<div class="namefield">
  <div class="nameinput">
    <Input bind:value={agentInput} placeholder="e.g. Jarvis, Nova, Atlas..." />
  </div>
  <button class="save" onclick={saveAgentName} disabled={!agentInput.trim()}>Save</button>
</div>
<p class="helper">Pick something short and natural to say aloud. Until you do, it's "Yap".</p>
{#if saved}
  <p class="savednote">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
    Agent named "{displayName}" — say "Hey {displayName}" followed by your instructions.
  </p>
{/if}

<style>
  .namefield {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .nameinput {
    flex: 1;
    min-width: 0;
  }
  /* centered monospace name entry, OpenWhispr-style */
  .nameinput :global(input) {
    text-align: center;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 13.5px;
  }
  .save {
    flex: 0 0 auto;
    height: 34px;
    padding: 0 16px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      transform var(--yap-dur) ease;
  }
  .save:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .save:active:not(:disabled) {
    transform: scale(0.985);
  }
  .save:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .helper {
    margin: 8px 0 0;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
  .savednote {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 8px 0 0;
    font-size: 11.5px;
    color: var(--yap-success);
  }
  .savednote svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }
</style>
