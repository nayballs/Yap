<script>
  // **Settings → Assistant → Yap Code** (desktop/src-tauri/src/agents.rs): Chat and
  // the Voice Assistant hand a task to the person's own Claude Code, in a
  // folder they chose, and it reports back.
  // - The **Yap Code** switch (`agentsEnabled`); Claude Code on this PC, found and
  //   signed in, or Sign in (ClaudeCodeStatus); **Clean up what you say in
  //   Code chats** (`agentsCleanSpeech`, CleanSpeech).
  // - **Pay with**: Your Claude plan / API key (`agentsPay`), saying which is
  //   in use, and for the key whether Language Models' Claude tab has one.
  // - **Defaults**: model and effort (`agentsModel`, `agentsEffort`), the Mode a
  //   new chat starts in (`agentsMode`; Bypass permissions asks once).
  // - **Workspaces** (the workspaces, `workspaces_*`): name, path and a
  //   cap ("Never above:"), Add workspace… (the folder picker), Remove (its
  //   Code chats are plain chats again, said here); their own Claude
  //   settings and MCP servers run too; the Bypass warning where Bypass can
  //   run. **Animate Code chats** (`agentsAnimate`).
  // - **Ask before starting an agent** (`agentsAsk`), **Read agent reports aloud**
  //   (`agentsSpeak`), and the limits: backups 7 days, one agent a folder, 3
  //   at once; Undo needs Claude Code's hooks (a managed policy can turn
  //   them off: then no backups, and the agent's tab says so).
  // - **History** (`agent_list`): each agent's status, folder, when and cost
  //   (plan and key told apart), Open (Chat on it) and Undo.
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
  import { statusWord, statusTone, costLine, MODE_LABELS, modelLabel, canUndo } from '../core/agents.js';
  import { FEATURE_NAME, CODE_NAME, WORKSPACES, samePath } from '../core/codeChats.js';

  let { cfg, goto = null, onshowview = null } = $props();

  const BYPASS = 'bypassPermissions';
  const WARNING = 'Claude Code can run anything on your PC without asking. Use it only in folders you trust.';

  const MODELS = ['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5'];
  const modelOptions = $derived(
    [...MODELS, ...(cfg.agentsModel && !MODELS.includes(cfg.agentsModel) ? [cfg.agentsModel] : [])].map((m) => ({ value: m, label: modelLabel(m) }))
  );
  const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'].map((e) => ({ value: e, label: EFFORT_LABELS[e] }));
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

  let folders = $state([]);
  let bypassConfirmed = $state(false);
  let confirmBypass = $state(false);
  let modeBefore = 'auto';
  let undoing = $state(null);
  let undoOpen = $state(false);
  let undoBusy = $state(false);
  let showAll = $state(false);

  const hasKey = $derived(!!sharedKey(cfg, 'anthropic'));
  const payLine = $derived(
    cfg.agentsPay === 'key'
      ? hasKey
        ? "Agents are charged to the API key on Language Models' Claude tab."
        : "There's no Claude API key in Language Models yet: add one on its Claude tab, or agents can't start."
      : "Agents use your Claude plan's limits, through Claude Code's own sign-in. Each agent shows what it would have cost on the API."
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

  async function undo() {
    const j = undoing;
    if (!j) return;
    undoBusy = true;
    try {
      const r = await invoke('agent_undo', { id: j.id });
      const back = (r?.restored?.length ?? 0) + (r?.deleted?.length ?? 0);
      toast({ title: back ? `Undone: ${back === 1 ? '1 file' : `${back} files`} put back` : 'Nothing needed putting back', variant: 'success' });
      refreshAgents();
    } catch (e) {
      toast({ title: "Couldn't undo the agent's changes", description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      undoBusy = false;
      undoOpen = false;
    }
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

<Group title="Defaults" desc="What an agent runs on unless you ask for something else." id="settings-yap-code-defaults">
  <Row label="Model" desc="The Claude model an agent runs on" setting="yap-code-model">
    <Select bind:value={cfg.agentsModel} options={modelOptions} ariaLabel="Model" />
  </Row>
  <Row label="Effort" desc="Higher effort thinks longer: more thorough, slower, and it uses more of your plan" setting="yap-code-effort">
    <Select bind:value={cfg.agentsEffort} options={EFFORTS} ariaLabel="Effort" />
  </Row>
  <Row label="Mode" desc="The Mode a new code chat starts in. Change it for one chat beside Chat's message box." setting="yap-code-mode">
    <Select bind:value={cfg.agentsMode} options={MODES} ariaLabel="Mode" onchange={onMode} wide />
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
  <Row label="Ask before sending an agent" desc="A card in Chat, or a question out loud, before an agent starts. When it matters: in a workspace no agent has used yet, or after the chat read the web." setting="yap-code-ask" stack>
    <Select bind:value={cfg.agentsAsk} options={ASK} ariaLabel="Ask before sending an agent" wide />
  </Row>
  <Row label="Read agent reports aloud" desc="When an agent ends, its report in the chat's own voice" setting="yap-code-speak">
    <Select bind:value={cfg.agentsSpeak} options={SPEAK} ariaLabel="Read agent reports aloud" />
  </Row>
  <Row>
    <p class="set-note" data-setting="yap-code-limits">Backups of changed files are kept 7 days. One agent at a time per workspace, at most 3 at once.</p>
  </Row>
  <Row>
    <p class="set-note small" data-setting="yap-code-backups-hooks">Undo needs Claude Code's hooks, which back up each file before an agent changes it. If a policy set by your organisation turns hooks off, no backups are made, and the agent's tab says so.</p>
  </Row>
</Group>

<Group title="History" id="settings-yap-code-history">
  {#each history as j (j.id)}
    <div class="hrow" data-testid="yap-code-history-row" data-status={j.status} data-setting="yap-code-history">
      <span class="hdot tone-{statusTone(j.status)}" aria-hidden="true"></span>
      <div class="htext">
        <p class="htop"><span class="hfolder">{j.folder}</span><span class="hstatus">{statusWord(j.status)}</span><span class="hwhen">{when(j.createdTs)}</span></p>
        <p class="hcost" data-testid="yap-code-history-cost">{costLine(j)}{j.undone ? ' · Undone' : ''}</p>
      </div>
      <div class="hbtns">
        <Button variant="ghost" size="sm" onclick={() => openAgent(j)}>Open</Button>
        {#if canUndo(j)}
          <Button
            variant="ghost"
            size="sm"
            onclick={() => {
              undoing = j;
              undoOpen = true;
            }}>Undo</Button
          >
        {/if}
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
<ConfirmDialog
  bind:open={undoOpen}
  title="Undo the agent's changes?"
  body={undoing ? `Every file the agent in ${undoing.folder} changed goes back to how it was before it, and the files it made are deleted.` : ''}
  confirmLabel="Undo the agent's changes"
  busyLabel="Undoing…"
  busy={undoBusy}
  onconfirm={undo}
/>

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
