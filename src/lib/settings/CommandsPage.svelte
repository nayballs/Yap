<script>
  // Settings → Commands (dictation/pipeline.rs run_agent; Wispr Flow's Command Mode
  // and its old "Hey Flow"): the AI does what you say and types the result
  // where you are. Not the Voice Assistant, which answers out loud. Say its
  // name (AgentNameField.svelte), Rewrite selected text (the rewrite
  // shortcut; pasting/as_said.rs; a question about the selection answered
  // instead, dictation/ask_or_edit.rs, and Ask about the selection's own
  // shortcut, chat/assistant/selected.rs), Replies to what's on screen (a
  // Command reading the window in front, dictation/command_screen.rs),
  // Transforms (TransformsSettings.svelte),
  // Writing samples (WritingSamples.svelte) and a link to its model and
  // prompt in Language Models.
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
  import WritingSamples from './WritingSamples.svelte';
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
  <!-- pasting/as_said.rs: nothing selected, right after a dictation into
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
<!-- A Command that replies to what's on screen reads the window in front
     (dictation/command_screen.rs; Wispr Flow's Instruct Mode, on this PC):
     its text, the box being written in left out. A model on this PC by
     default; the cloud only when allowed (asked once). -->
<Group
  title="Replies to what's on screen"
  desc="Ask for a reply and Yap drafts it from the email or chat in front of you, then types it where your cursor is."
  id="settings-commands-screen"
>
  <Row setting="commands-screen">
    <Toggle
      bind:checked={cfg.commandsScreen}
      label="Let Commands read the window in front when you ask for a reply"
      desc={`Say "Hey ${wakeName}, reply saying Thursday works" or "answer her question", or use the rewrite shortcut with nothing selected`}
      hint="Only when what you say replies to what's on screen. Yap reads the window's text, never the box you're typing in, never its own windows or a terminal, and keeps none of it. An app that shows Yap no text is sent as a screenshot to a model that reads pictures."
    />
  </Row>
  <Row setting="commands-screen-cloud">
    <Toggle
      bind:checked={cfg.commandsScreenCloud}
      label="With cloud models too"
      desc="Off, the window's text only goes to a model on this PC. On, it goes to the cloud provider the Commands model runs on too"
      hint="Network connections in Privacy & data lists each one as “Commands + the window's text”."
      disabled={!cfg.commandsScreen}
    />
  </Row>
</Group>
<!-- Asking about a selection without replacing it (dictation/ask_or_edit.rs,
     chat/assistant/selected.rs): the answer comes as the Voice Assistant's. -->
<Group
  title="Ask about selected text"
  desc="Ask about text you selected and hear or read the answer, without the text being changed."
  id="settings-ask-selection"
>
  <!-- A question said with the rewrite shortcut ("what does this mean?") is
       answered, never pasted over the selection. Off: it always rewrites. -->
  <Row setting="ask-about-selection">
    <Toggle
      bind:checked={cfg.askAboutSelection}
      label="Answer questions about the selected text"
      desc="With the rewrite shortcut, ask “what does this mean?” or “is this right?” and Yap answers, leaving the text as it is"
      hint="Yap only replaces the text when you clearly ask for a change, like “make it shorter”. The answer is spoken when Speak replies is on, and kept in Chat. Text you can't edit, like a web page, is always answered."
    />
  </Row>
  <!-- Its own shortcut: always answers, never replaces. -->
  <Row
    label="Ask about the selection"
    desc="Its own shortcut: select text in any app, press it and ask. It always answers and never changes the text"
    hint="Works on text you can't edit too, like a web page."
    setting="ask-shortcut"
  >
    <SettingsHotkey {cfg} target="askHotkey" clearable />
  </Row>
</Group>
<!-- Saved transforms (dictation/transforms.rs): one-key rewrites, no speaking. -->
<Group
  title="Transforms"
  desc="Rewrite the selected text in one press, without saying anything. With nothing selected, your last dictation, by the rules above."
  id="settings-transforms"
>
  <TransformsSettings {cfg} />
</Group>
<!-- Writing samples (ai/writing_samples.rs): your own writing, so rewrites
     sound like you. Here, beside the rewrites that use them most. -->
<Group
  title="Writing samples"
  desc="Paste a few things you wrote yourself, so the rewrite shortcut, Commands and transforms like Polish and Shorter sound like you. Tag each with the kind of app it suits."
  id="settings-writing-samples"
>
  <WritingSamples />
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
