<script>
  // **"Clean up what you say in Chat" / "…in Code chats"** (2026-11-07,
  // ai/cleanup_styles.rs `cleans_question`): a question said to Yap's AI —
  // the assistant shortcut's, Ask about the selection's, the Chat view's mic
  // — cleaned up as an AI prompt, literally (fillers and false starts out,
  // the words kept), on Dictation cleanup's model, whatever AI cleanup's own
  // switch says. One row each side, each its own switch: Settings → Voice
  // Assistant (`chatCleanSpeech`), Settings → Yap Code (`agentsCleanSpeech`). No
  // cleanup model set up (`question_cleanup_status`): the words go as said,
  // and the row says so with the way there.
  //
  //   cfg:     Settings' config $state (auto-save does the saving).
  //   field:   'chatCleanSpeech' | 'agentsCleanSpeech'.
  //   label, desc, setting: the row's words and its search anchor.
  //   goto:    opens another Settings page.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import { localLlm } from './localLlm.svelte.js';

  let { cfg, field, label, desc, setting, goto = null } = $props();

  // Unknown until Rust answers: no note meanwhile.
  let ready = $state(true);

  async function read() {
    try {
      ready = !!(await invoke('question_cleanup_status'));
    } catch {
      ready = true;
    }
  }

  // Asked again when the on-device model comes or goes…
  $effect(() => {
    void localLlm.status.installed;
    read();
  });
  // …and after every save (a provider, a key, a model picked).
  $effect(() => {
    const un = listen('yap-config-saved', () => setTimeout(read, 300));
    return () => un.then((f) => f()).catch(() => {});
  });
</script>

<Row {setting}>
  <Toggle bind:checked={cfg[field]} {label} {desc} />
  {#if cfg[field] && !ready}
    <p class="set-note needs" data-testid="clean-speech-needs">
      Needs a cleanup model in Language Models → Dictation cleanup. Until then, what you say goes as you said it.
      <button type="button" class="set-link" onclick={() => goto?.({ page: 'cleanup', scope: 'cleanup' })}>Set up a cleanup model</button>
    </p>
  {/if}
</Row>

<style>
  .needs {
    margin: 6px 0 0;
  }
</style>
