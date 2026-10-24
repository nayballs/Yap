<script>
  // Settings → Commands (pipeline.rs run_agent; Wispr Flow's Command Mode
  // and its old "Hey Flow"): the AI does what you say and types the result
  // where you are. Not the Voice Assistant, which answers out loud. Say its
  // name (AgentNameField.svelte), Rewrite selected text (the rewrite
  // shortcut; as_said.rs), Transforms (TransformsSettings.svelte) and a link
  // to its model and prompt in Language Models.
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page.
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import AgentNameField from './AgentNameField.svelte';
  import SettingsHotkey from './SettingsHotkey.svelte';
  import TransformsSettings from './TransformsSettings.svelte';
  import PageExplain from './PageExplain.svelte';
  import { scopeModelLabel } from './scopes.js';
  import { localLlm } from './localLlm.svelte.js';

  let { cfg, goto } = $props();

  const wakeName = $derived((cfg?.agentName || '').trim() || 'Yap');
</script>

<div class="page-h">
  <h1>Commands</h1>
  <p>Tell Yap's AI what to write, and it types the result where you are.</p>
</div>
<PageExplain ask="Want an answer you can hear instead?" to="Voice Assistant" onclick={() => goto('voice')}>
  Say <q>Hey {wakeName}, reply that I'll be late</q> in any app; Yap writes it and types it there.
</PageExplain>
<Group title="Say its name" id="settings-wake-word">
  <Row setting="wake-word">
    <Toggle
      bind:checked={cfg.llmScopes.voiceAgent.enabled}
      label="Answer to its name in any dictation"
      desc={`Say "${wakeName}, …" while you dictate and the AI does what you ask, instead of typing your words`}
    />
  </Row>
  <Row label="Agent name" desc="Short and easy to say. Until you pick one, it's “Yap”" setting="agent-name">
    <AgentNameField {cfg} />
  </Row>
</Group>
<Group title="Rewrite selected text" id="settings-rewrite">
  <Row
    label="Rewrite shortcut"
    desc={cfg.rewriteLastDictation
      ? 'Select text in any app, press it and say how to change it. With nothing selected within a minute of a dictation, it rewrites that dictation where it is; otherwise it writes where your cursor is'
      : 'Select text in any app, press it and say how to change it. With nothing selected, it writes where your cursor is'}
    hint="For example “make this a bulleted list”, “more formal” or “in French”."
    setting="rewrite-shortcut"
  >
    <SettingsHotkey {cfg} target="editHotkey" clearable />
  </Row>
  <!-- as_said.rs: nothing selected, right after a dictation into
       the same field (still ending at the cursor) → that dictation
       rewritten. Off: write mode, as before. -->
  <Row setting="rewrite-last">
    <Toggle
      bind:checked={cfg.rewriteLastDictation}
      label="Rewrite your last dictation when nothing is selected"
      desc="Within a minute, in the same field"
      hint="Only while it's exactly as Yap typed it, with the cursor right after it; otherwise Yap says so and changes nothing."
    />
  </Row>
</Group>
<!-- Saved transforms (transforms.rs): one-key rewrites, no speaking. -->
<Group
  title="Transforms"
  desc="Rewrite the selected text in one press, without saying anything. With nothing selected, your last dictation, by the rules above."
  id="settings-transforms"
>
  <TransformsSettings {cfg} />
</Group>
<Group title="Model and prompt" id="settings-commands-model">
  <LinkRow
    label="Model and prompt"
    desc="In Language Models. With “Answer to its name” off, the rewrite shortcut and transforms use the cleanup model"
    value={scopeModelLabel(cfg, 'voiceAgent', localLlm.status.model)}
    setting="commands-model"
    onclick={() => goto({ page: 'cleanup', scope: 'voiceAgent' })}
  />
</Group>
