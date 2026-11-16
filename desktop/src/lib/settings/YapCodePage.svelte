<script>
  // **Settings → Assistant → Yap Code** (desktop/src-tauri/src/agents.rs): Chat and
  // the Voice Assistant hand a task to the person's own Claude Code, in a
  // folder they chose, and it reports back.
  // - The **Yap Code** switch (`agentsEnabled`); Claude Code on this PC, found and
  //   signed in, or Sign in (ClaudeCodeStatus); **Clean up what you say in
  //   Code chats** (`agentsCleanSpeech`, CleanSpeech).
  // - **Pay with**: Your Claude plan / API key (`agentsPay`), saying which is
  //   in use, and for the key whether Language Models' Claude tab has one.
  // - **Defaults**: model and effort (`agentsModel`, `agentsEffort`: what an
  //   agent runs on when Chat doesn't size it for the task, agents/sizing.rs),
  //   **Most Chat may choose** (`agentsCeilingModel`, `agentsCeilingEffort`:
  //   "No limit" by default; the person's own words still go above it), the
  //   Mode a new chat starts in (`agentsMode`; Bypass permissions asks once).
  // - **Open file links in** (`agentsOpenFiles`, agents/editors.rs, 16 Nov 2026):
  //   Yap's file view, VS Code, Cursor, another editor found on this PC
  //   (`code_editors`, looked for afresh when the page opens), or Windows'
  //   default app. VS Code and Cursor are always listed, greyed out when
  //   they aren't here; a chosen editor that's gone stays shown, said so.
  // - **Workspaces** (the workspaces, `workspaces_*`): name, path and a
  //   cap ("Never above:"), Add workspace… (the folder picker), Remove (its
  //   Code chats are plain chats again, said here); their own Claude
  //   settings and MCP servers run too; the Bypass warning where Bypass can
  //   run. **Animate Code chats** (`agentsAnimate`).
  // - **Ask before starting an agent** (`agentsAsk`), **Read agent reports aloud**
  //   (`agentsSpeak`), and the limits: one agent a folder, 3 at once. No
  //   Undo (16 Nov 2026, as the Claude app): the person asks Claude to undo
  //   a change, or rewinds to before their message in a Code chat.
  // - **History** (`agent_list`): each agent's status, folder, when and cost
  //   (plan and key told apart), and Open (Chat on it).
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto(target): another Settings page; onshowview(view): leaves Settings.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import ClaudeCodeStatus from './ClaudeCodeStatus.svelte';
  import CleanSpeech from './CleanSpeech.svelte';
  import { sharedKey } from '../core/providers.js';
  import { EFFORT_LABELS } from '../core/effort.js';
  import { agents, initAgents, refreshAgents } from '../stores/agents.svelte.js';
  import { statusWord, statusTone, costLine, MODE_LABELS, modelLabel, BYPASS_WARNING } from '../core/agents.js';
  import { FEATURE_NAME, CODE_NAME, WORKSPACES, samePath, AGENT_NOTICES_LABEL, AGENT_NOTICES_DESC } from '../core/codeChats.js';

  let { cfg, goto = null, onshowview = null } = $props();

  const BYPASS = 'bypassPermissions';
  const WARNING = BYPASS_WARNING;

  // What an agent runs on by default: the plan's models (ai/claude_plan.rs
  // `MODELS`), the most capable first.
  const MODELS = ['claude-fable-5-2', 'claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5'];
  const modelOptions = $derived(
    [...MODELS, ...(cfg.agentsModel && !MODELS.includes(cfg.agentsModel) ? [cfg.agentsModel] : [])].map((m) => ({ value: m, label: modelLabel(m) }))
  );
  const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'].map((e) => ({ value: e, label: EFFORT_LABELS[e] }));
  // "Most Chat may choose": No limit, or a model (then the most effort beside it).
  // (The top model as a ceiling is no limit at all, so it isn't listed.)
  const CEILINGS = [{ value: '', label: 'No limit' }, ...MODELS.slice(1).map((m) => ({ value: m, label: modelLabel(m) }))];
  const MODES = ['auto', 'manual', 'acceptEdits', 'plan', BYPASS].map((m) => ({ value: m, label: MODE_LABELS[m] + (m === 'auto' ? ' (Recommended)' : '') }));
  const CAPS = [
    { value: '', label: 'No limit' },
    { value: 'auto', label: 'Auto' },
    { value: 'acceptEdits', label: 'Accept edits' },
    { value: 'manual', label: 'Manual' },
    { value: 'plan', label: 'Plan' },
  ];
  const ASK = [
    { value: 'new-or-web', label: 'When it matters (Recommended)' },
    { value: 'always', label: 'Always' },
    { value: 'never', label: 'Never' },
  ];
  const SPEAK = [
    { value: 'voice', label: 'When I asked by voice' },
    { value: 'always', label: 'Always' },
    { value: 'never', label: 'Never' },
  ];
  const PAY = [
    { value: 'plan', label: 'Your Claude plan' },
    { value: 'key', label: 'API key' },
  ];

  // "Open file links in": the editors on this PC (agents/editors.rs `code_editors`).
  const ALWAYS = [
    ['vscode', 'VS Code'],
    ['cursor', 'Cursor'],
  ];
  let found = $state({ loaded: false, list: [], choice: 'yap', choiceName: null });
  const openOptions = $derived.by(() => {
    const here = (id) => found.list.some((e) => e.id === id);
    const chosen = cfg.agentsOpenFiles || 'yap';
    const out = [{ value: 'yap', label: "Yap's file view" }];
    for (const [id, name] of ALWAYS) {
      const gone = found.loaded && !here(id);
      out.push({ value: id, label: gone ? `${name} (not on this PC)` : name, disabled: gone && chosen !== id });
    }
    for (const e of found.list) if (!ALWAYS.some(([id]) => id === e.id)) out.push({ value: e.id, label: e.name });
    // The chosen editor, no longer on this PC: still shown, so the box says what's saved.
    if (!out.some((o) => o.value === chosen) && chosen !== 'default') out.push({ value: chosen, label: `${(found.choice === chosen && found.choiceName) || 'That editor'} (not on this PC)` });
    out.push({ value: 'default', label: "Windows' default app" });
    return out;
  });
  const openLine = $derived.by(() => {
    const chosen = cfg.agentsOpenFiles || 'yap';
    if (chosen === 'yap') return 'Read only, beside the chat, at the line. Right-click a link to open it in an editor, show it in its folder or copy its path.';
    if (chosen === 'default') return "Each file in the app Windows opens it with. A file Windows would run rather than open, such as a script or a web page, opens in Yap's file view instead.";
    const name = found.list.find((e) => e.id === chosen)?.name;
    if (found.loaded && !name) return "That editor isn't on this PC, so links open in Yap's file view until it's back.";
    return `At the line, in ${name || 'that editor'}. A spreadsheet still opens as a sheet in Yap. Right-click a link to open it in Yap instead.`;
  });
  async function loadOpenWith() {
    try {
      const v = await invoke('code_editors', { refresh: true });
      found = { loaded: true, list: v?.editors || [], choice: v?.choice || 'yap', choiceName: v?.choiceName || null };
    } catch {
      found = { ...found, loaded: true };
    }
  }

  let folders = $state([]);
  let bypassConfirmed = $state(false);
  let confirmBypass = $state(false);
  let modeBefore = 'auto';
  let showAll = $state(false);

  const hasKey = $derived(!!sharedKey(cfg, 'anthropic'));
  const payLine = $derived(
    cfg.agentsPay === 'key'
      ? hasKey
        ? "Agents are charged to the API key on Language Models' Claude tab."
        : "There's no Claude API key in Language Models yet: add one on its Claude tab, or agents can't start."
      : "Agents use your Claude plan's limits, through Claude Code's own sign-in. Each agent shows what it would have cost on the API."
  );
  const ceilingLine = $derived(
    `Chat picks each agent's model and effort for its task, never above this. Your own words ("use Opus", "think hard") still go higher.` +
      (cfg.agentsPay === 'key' ? ' On an API key, bigger models cost more per run.' : '')
  );
  const history = $derived(showAll ? agents.list : agents.list.slice(0, 20));

  async function loadFolders() {
    try {
      folders = (await invoke('workspaces_list')) || [];
    } catch {
      folders = [];
    }
  }

  async function loadMode() {
    try {
      bypassConfirmed = !!(await invoke('agent_mode_get', { conversation: null }))?.bypassConfirmed;
    } catch {
      /* keep */
    }
  }

  onMount(() => {
    initAgents();
    refreshAgents();
    loadFolders();
    loadMode();
    loadOpenWith();
    modeBefore = cfg.agentsMode || 'auto';
  });

  // The default Mode: Bypass asks once (as the chip does).
  function onMode(v) {
    if (v === BYPASS && !bypassConfirmed) {
      cfg.agentsMode = modeBefore;
      confirmBypass = true;
      return;
    }
    modeBefore = v;
  }

  async function okBypass() {
    try {
      await invoke('agent_bypass_confirmed');
    } catch {
      return;
    }
    bypassConfirmed = true;
    confirmBypass = false;
    cfg.agentsMode = BYPASS;
    modeBefore = BYPASS;
  }

  // Add workspace…: Windows' folder picker, opened in Rust (`workspaces_pick`),
  // the folder picked added there.
  async function addFolder() {
    try {
      const f = await invoke('workspaces_pick');
      if (!f) return;
      await loadFolders();
      toast({ title: `“${f.name}” added`, description: `Agents can work in it now, and ${CODE_NAME} chats can use it.`, variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't add that workspace", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }

  // Remove: its Code chats are plain chats again (agents/code_chat.rs), said here, once.
  async function removeFolder(f) {
    let chats = 0;
    try {
      const code = await invoke('code_chats_list');
      chats = Object.values(code?.chats ?? {}).filter((w) => samePath(w.path, f.path)).length;
    } catch {
      /* said without the count */
    }
    try {
      await invoke('workspaces_remove', { name: f.name });
      const moved = chats ? ` ${chats === 1 ? 'Its Code chat is a plain chat' : `Its ${chats} Code chats are plain chats`} now.` : '';
      toast({ title: `“${f.name}” removed`, description: `An agent will ask before working there again.${moved} Its agents stay in History.` });
    } catch (e) {
      toast({ title: "Couldn't remove it", description: String(e?.message ?? e), variant: 'destructive' });
    }
    loadFolders();
  }

  async function setCap(f, cap) {
    try {
      const next = await invoke('workspaces_set_cap', { name: f.name, cap: cap || null });
      folders = folders.map((x) => (x.name === f.name ? next : x));
    } catch (e) {
      toast({ title: "Couldn't change it", description: String(e?.message ?? e), variant: 'destructive' });
      loadFolders();
    }
  }

  function openAgent(j) {
    agents.openWanted = { id: j.id, conversation: j.conversation ?? null };
    onshowview?.('chat');
  }

  const when = (secs) =>
    secs ? new Date(secs * 1000).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: 'numeric', minute: '2-digit' }) : '';
</script>

<div class="page-h">
  <h1>{FEATURE_NAME}</h1>
  <p>Send an agent, your own Claude Code, from Chat or the Voice Assistant. It works in a workspace you chose, and tells you what it did.</p>
</div>

<Group title={FEATURE_NAME} id="settings-yap-code">
  <Row setting="yap-code-enabled">
    <Toggle bind:checked={cfg.agentsEnabled} label={FEATURE_NAME} desc="Let Chat and the Voice Assistant send agents, your own Claude Code, to work in workspaces you choose." />
  </Row>
  <ClaudeCodeStatus />
  <!-- A Code chat that works directly (agents/direct.rs, 11 Nov 2026): as
       Claude Code's own app, with no model of Chat's wording a brief. -->
  <Row setting="yap-code-direct">
    <Toggle
      bind:checked={cfg.agentsDirect}
      label="Work directly in {CODE_NAME} chats"
      desc="On your Claude plan, what you type in a {CODE_NAME} chat goes straight to Claude Code in that workspace. It does the work itself and answers in its own words, as Claude Code's own app does. Off: Chat hands each request to an agent and tells you what came of it."
    />
  </Row>
  <!-- What's said into a Code chat, cleaned up as an AI prompt, word for
       word (ai/cleanup_styles.rs `cleans_question`); plain chats have their
       own switch in Settings → Voice Assistant. -->
  <CleanSpeech
    {cfg}
    {goto}
    field="agentsCleanSpeech"
    setting="yap-code-clean-speech"
    label="Clean up what you say in {CODE_NAME} chats"
    desc="Fillers and false starts taken out of spoken questions and requests, your words kept. Uses your dictation cleanup's model."
  />
</Group>

<Group title="Paying for agents" id="settings-yap-code-pay">
  <Row label="Pay with" desc={payLine} setting="yap-code-pay">
    <Segmented bind:value={cfg.agentsPay} options={PAY} label="Pay with" />
  </Row>
  {#if cfg.agentsPay === 'key' && !hasKey}
    <Row>
      <p class="set-note">
        <button type="button" class="set-link" onclick={() => goto?.('cleanup#chat')}>Add an API key on Language Models' Claude tab</button>
      </p>
    </Row>
  {/if}
</Group>

<Group title="Defaults" desc="Chat sizes each agent to its task; these are what an agent runs on when it doesn't, unless you ask for something else." id="settings-yap-code-defaults">
  <Row label="Model" desc="The Claude model an agent runs on" setting="yap-code-model">
    <Select bind:value={cfg.agentsModel} options={modelOptions} ariaLabel="Model" />
  </Row>
  <Row label="Effort" desc="Higher effort thinks longer: more thorough, slower, and it uses more of your plan" setting="yap-code-effort">
    <Select bind:value={cfg.agentsEffort} options={EFFORTS} ariaLabel="Effort" />
  </Row>
  <Row label="Most Chat may choose" desc={ceilingLine} setting="yap-code-ceiling">
    <div class="frow" data-testid="yap-code-ceiling">
      <Select bind:value={cfg.agentsCeilingModel} options={CEILINGS} ariaLabel="Most Chat may choose" />
      {#if cfg.agentsCeilingModel}
        <Select bind:value={cfg.agentsCeilingEffort} options={EFFORTS} ariaLabel="Most effort Chat may choose" />
      {/if}
    </div>
  </Row>
  <Row label="Mode" desc="The Mode a new code chat starts in. Change it for one chat beside Chat's message box." setting="yap-code-mode">
    <Select bind:value={cfg.agentsMode} options={MODES} ariaLabel="Mode" onchange={onMode} wide />
  </Row>
</Group>

<Group title="Files in {CODE_NAME} chats" desc="A file's path in a {CODE_NAME} chat's answer, or one you @-mentioned, is a link to that file." id="settings-yap-code-files">
  <Row label="Open file links in" desc={openLine} setting="yap-code-open-files" stack>
    <Select bind:value={cfg.agentsOpenFiles} options={openOptions} ariaLabel="Open file links in" wide />
  </Row>
</Group>

<Group title={WORKSPACES} desc="The folders on this PC where agents may work and Code chats are kept. An agent asks first in a folder that isn't here yet." id="settings-workspaces">
  {#each folders as f (f.path)}
    <Row label={f.name} setting="yap-code-workspace" stack>
      {#snippet desc()}
        <span class="fpath" title={f.path}>{f.path}</span>
        {#if bypassConfirmed && (!f.cap || f.cap === BYPASS)}
          <span class="fwarn" data-testid="yap-code-bypass-warning">Bypass permissions can run here: {WARNING}</span>
        {/if}
      {/snippet}
      <div class="frow" data-testid="yap-code-workspace" data-name={f.name}>
        <span class="caplabel">Never above:</span>
        <Select value={f.cap || ''} options={CAPS} ariaLabel={`Never above, in ${f.name}`} onchange={(v) => setCap(f, v)} />
        <Button variant="ghost" size="sm" onclick={() => removeFolder(f)}>Remove</Button>
      </div>
    </Row>
  {:else}
    <Row>
      <p class="set-note">No workspaces yet. Add one, or name a folder's full path when you ask Chat for an agent.</p>
    </Row>
  {/each}
  <Row label="Add workspace…" desc="Pick a folder on this PC agents may work in" setting="yap-code-add-workspace">
    <Button variant="secondary" size="sm" onclick={addFolder}>Add workspace…</Button>
  </Row>
  <Row>
    <p class="set-note small">A workspace's own Claude settings and MCP servers also run.</p>
  </Row>
  <Row setting="yap-code-animate">
    <Toggle bind:checked={cfg.agentsAnimate} label="Animate {CODE_NAME} chats" desc="A slow light moves along a {CODE_NAME} chat's message box while its agent runs. Never with reduced motion." />
  </Row>
</Group>

<Group title="Asking and reports" id="settings-yap-code-asking">
  <Row label="Ask before sending an agent" desc="A card in Chat, or a question out loud, before an agent starts. When it matters: in a workspace no agent has used yet, or after the chat read the web. Never in Bypass permissions." setting="yap-code-ask" stack>
    <Select bind:value={cfg.agentsAsk} options={ASK} ariaLabel="Ask before sending an agent" wide />
  </Row>
  <Row label="Read agent reports aloud" desc="When an agent ends, its report in the chat's own voice" setting="yap-code-speak">
    <Select bind:value={cfg.agentsSpeak} options={SPEAK} ariaLabel="Read agent reports aloud" />
  </Row>
  <!-- The same switch as General → Notifications' (config `notifyAgents`). -->
  <Row setting="yap-code-notify">
    <Toggle bind:checked={cfg.notifyAgents} label={AGENT_NOTICES_LABEL} desc={AGENT_NOTICES_DESC} />
  </Row>
  <Row>
    <p class="set-note" data-setting="yap-code-limits">One agent at a time per workspace, at most 3 at once. To take a change back, ask Claude, or rewind to before your message: Claude Code's own checkpoints put its edits back.</p>
  </Row>
</Group>

<Group title="History" id="settings-yap-code-history">
  {#each history as j (j.id)}
    <div class="hrow" data-testid="yap-code-history-row" data-status={j.status} data-setting="yap-code-history">
      <span class="hdot tone-{statusTone(j.status)}" aria-hidden="true"></span>
      <div class="htext">
        <p class="htop"><span class="hfolder">{j.folder}</span><span class="hstatus">{statusWord(j.status)}</span><span class="hwhen">{when(j.createdTs)}</span></p>
        <p class="hcost" data-testid="yap-code-history-cost">{costLine(j)}</p>
      </div>
      <div class="hbtns">
        <Button variant="ghost" size="sm" onclick={() => openAgent(j)}>Open</Button>
      </div>
    </div>
  {:else}
    <Row>
      <p class="set-note" data-setting="yap-code-history">No agents yet. Ask Chat to send one to a workspace above.</p>
    </Row>
  {/each}
  {#if agents.list.length > 20 && !showAll}
    <Row>
      <button type="button" class="set-link" onclick={() => (showAll = true)}>Show all {agents.list.length}</button>
    </Row>
  {/if}
</Group>

<p class="set-note foot" data-testid="yap-code-connections-note">Agents connect to Anthropic through Claude Code itself, so they aren't in Network connections.</p>

<ConfirmDialog bind:open={confirmBypass} title="Use Bypass permissions?" body={WARNING} confirmLabel="Use Bypass permissions" onconfirm={okBypass} />

<style>
  .fpath {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fwarn {
    display: block;
    margin-top: 2px;
    color: var(--yap-danger);
    font-size: 12px;
  }
  .frow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .caplabel {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .small {
    font-size: 12px;
  }
  .hrow {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
  }
  .hdot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-muted);
  }
  .hdot.tone-live {
    background: var(--yap-primary);
  }
  .hdot.tone-ask {
    background: var(--yap-warning);
  }
  .hdot.tone-done {
    background: var(--yap-success);
  }
  .hdot.tone-failed {
    background: var(--yap-danger);
  }
  .htext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .htop,
  .hcost {
    margin: 0;
  }
  .htop {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    font-size: 13px;
  }
  .hfolder {
    font-weight: 600;
    color: var(--yap-fg);
  }
  .hstatus,
  .hwhen {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .hcost {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .hbtns {
    flex: 0 0 auto;
    display: flex;
    gap: 4px;
  }
  .foot {
    margin-top: -12px;
  }
</style>
