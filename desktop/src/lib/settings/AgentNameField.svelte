<script>
  // The name Commands answer to ("Yap, …" in a dictation runs it as a
  // command; dictation/agent_detect.rs). Settings → Commands. Not on Language Models →
  // Commands (OpenWhispr's DictationAgentSettings "Agent Name"), which points
  // here. It saves itself like every other field (2026-10-22 review #14: no
  // stray Save button): a moment after you stop typing, or as you leave it.
  //
  //   cfg: the Settings config $state (cfg.agentName + cfg.dictionary).
  import Input from '../ui/Input.svelte';
  import { untrack } from 'svelte';

  let { cfg } = $props();

  // Local editor state for the name field, seeded once from the saved name.
  let agentInput = $state(untrack(() => (cfg.agentName || '').trim()));
  let timer = 0;

  function saveAgentName() {
    clearTimeout(timer);
    const trimmed = agentInput.trim();
    if (!trimmed) return;
    const prev = (cfg.agentName || '').trim();
    if (trimmed === prev) return;

    cfg.agentName = trimmed;

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
  }
  function typed() {
    clearTimeout(timer);
    timer = setTimeout(saveAgentName, 800);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="namefield" onfocusout={saveAgentName} onkeydown={(e) => e.key === 'Enter' && saveAgentName()}>
  <Input bind:value={agentInput} placeholder="e.g. Jarvis, Nova, Atlas..." ariaLabel="Agent name" oninput={typed} />
</div>

<style>
  .namefield {
    width: var(--yap-select-w-wide);
  }
</style>
