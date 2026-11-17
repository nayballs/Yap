<script>
  // Chat's composer (after Claude Code's): + (Take a screenshot; with the
  // connectors preview on and a model that gets them, Connectors ▸ — a
  // switch per connector for this chat, then Manage connectors…, as Claude's
  // composer has: chatConnectors.svelte.js; voice replies and their volume),
  // the message, the mic, and Send once there's
  // something to send — ■ Stop while an answer comes (Esc too); under it,
  // quiet, the model the message goes to (ModelChip, whose picker opens
  // above it all; on your Claude plan, the plan's usage ring beside it:
  // PlanUsageRing), and bottom right the context ring (ContextRing: how much
  // of the model's context this chat uses, and the day's AI use in its
  // panel; not on the plan, whose ring says how full the chat is). Snip to Chat (snip.rs): the screenshots this question
  // carries sit over the box with three one-tap questions, and — a model
  // that can't read images — one line saying so, with the switch, or
  // "Send its text instead" (Windows' OCR on this PC: snip/ocr.rs); the
  // on-device model first offers its own projector, "Read screenshots on
  // this PC" (ai/local_llm/vision.rs: a download, or switched back on). A
  // picture pasted (Ctrl+V) or dropped on Chat joins them (snip/attach.rs);
  // a screenshot's right-click menu copies or saves it. + → Attach files…
  // (or any other file dropped on Chat) puts a file's chip in the box: its
  // text goes with the question (chat/attachments.rs). Nothing goes until
  // you send.
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from '../../ui/toast.svelte.js';
  import DownloadProgress from '../../ui/DownloadProgress.svelte';
  import { fromEvent } from '../../core/downloads.js';
  import { localVision, downloadVision, cancelVision, setVisionOn } from '../../stores/localVision.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { formatHotkeySpec } from '../../core/hotkeys.js';
  import { modelName, providerName } from '../../core/aiModel.js';
  import { sizeLine, imageItems, tilePlaced, shown } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';
  import { tooLongLine } from './chatBudget.js';
  import { agents } from '../../stores/agents.svelte.js';
  import { workingDirectly } from '../../core/agents.js';
  import SkillMenu from '../../components/SkillMenu.svelte';
  import { tick, untrack } from 'svelte';
  import ModelChip from './ModelChip.svelte';
  // A plain chat's + menu (16 Nov 2026, the Claude app's): Research and Style live there, their chips in the box.
  import { research, refreshResearch } from './chatResearch.svelte.js';
  import { ChatStyle } from './chatStyles.svelte.js';
  import { skills as skillList, openMoreSkills } from '../../stores/skills.svelte.js';
  import { skillMenu } from '../../core/skills.js';
  // A Code chat's workspace chip and Yap Code's Mode chip, before it, and its
  // box's edge (codeChats.svelte.js, codeChat.css).
  import CodeChips from './CodeChips.svelte';
  import CodeEdge from './CodeEdge.svelte';
  import ContextRing from './ContextRing.svelte';
  import ThinkButton from './ThinkButton.svelte';
  import ElementChip from './ElementChip.svelte';
  import FileChip from './FileChip.svelte';
  import MentionMenu from './MentionMenu.svelte';
  import { mentionedPaths } from './chatMentions.js';
  import { picked, removePicked } from '../../stores/pickedElements.svelte.js';
  import ConnectorLogo from '../../settings/connectors/ConnectorLogo.svelte';
  // A Code chat's + menu and "/" list (17 Nov 2026, the Claude Code app's: agents/menu.rs).
  import { CodeMenu, CommandPicker, commandGroups, matchCommands, pluginRows, serverRows, SERVER_STATUS, codeCommand, modelFor, roughTokens } from './codeMenu.svelte.js';
  import { loadCatalog, modelsFor } from '../../stores/modelCatalog.svelte.js';
  import { effortLabel, effortNow } from '../../core/effort.js';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import CommandMenu from './CommandMenu.svelte';
  // Toasts never cover the box (ui/toastClear.js).
  import { keepClear } from '../../ui/toastClear.js';

  /**
   * `chat`: the view's state; `onopensettings(section)`: ControlPanel's
   * Settings; `onopenusage()`: Insights' AI usage card (the context ring's
   * See all). `code`: the view's Code chats (codeChats.svelte.js).
   */
  let { chat, code = null, codeMenu: givenMenu = null, onopensettings = null, onopenusage = null } = $props();

  // **■ while the Code chat's own session works** (12 Nov 2026, as Claude
  // Code's own app has Stop where Send was): with nothing typed, the button
  // stops its work (Esc in the empty box too: ChatView). With something
  // typed it's Send as ever — the message goes to it at once, as a note.
  const working = $derived(workingDirectly(agents.list, chat.activeId));
  // **Ctrl+Enter queues it for later** (chatState `send({ later })`; the Claude app's box says so while it works):
  // said in the empty box while Claude works, or while an answer is written.
  // The files the words mention (`@path`, a Code chat's @: agents/mention.rs): a chip each over the box.
  const mentioned = $derived(code?.current ? mentionedPaths(chat.input) : []);
  const queueHint = $derived(working ? 'Enter to send, Ctrl+Enter to queue for later' : chat.sending ? 'Ctrl+Enter to queue a message for after this answer' : '');
  let stoppingWork = $state(null);
  async function stopWork() {
    const id = working?.id;
    if (!id || stoppingWork === id) return;
    stoppingWork = id;
    try {
      await invoke('agent_stop', { id });
    } catch (e) {
      toast.error(String(e));
    } finally {
      stoppingWork = null;
    }
  }

  // **The message box is several lines** (15 Nov 2026, the Claude app's box): a textarea that grows with its words
  // up to a cap (`boxCap`), then scrolls. Enter sends, Shift+Enter makes a new line, Ctrl+Enter queues it for later;
  // the "@" picker and the "/" Skills menu take their keys first, and an IME's composition is its own.
  function keydown(e) {
    if (commandKey(e) || chat.mentionKey(e) || chat.skillKey(e)) return;
    // (Enter that confirms an IME's composition: the IME's.)
    if (e.key !== 'Enter' || e.isComposing || e.keyCode === 229 || e.shiftKey) return;
    e.preventDefault();
    submit(e.ctrlKey || e.metaKey ? { later: true } : undefined);
  }
  /** One line's height (its padding in): an empty box never measures its placeholder. */
  const ONE_LINE = 32;
  /** The tallest the box grows: 40% of the window, 120 to 320 px. */
  const boxCap = () => Math.max(120, Math.min(320, Math.round(window.innerHeight * 0.4)));
  /** The box's height fitted to its words (set by hand, or dictated, pasted, put back: every way they change). */
  function fitBox() {
    const el = chat.inputEl;
    if (!el) return;
    if (!chat.input) {
      el.style.height = `${ONE_LINE}px`;
      el.style.overflowY = 'hidden';
      return;
    }
    el.style.height = 'auto';
    const cap = boxCap();
    const full = el.scrollHeight;
    el.style.height = `${Math.max(ONE_LINE, Math.min(full, cap))}px`;
    el.style.overflowY = full > cap ? 'auto' : 'hidden';
  }
  $effect(() => {
    void chat.input;
    void chat.inputEl;
    fitBox();
  });
  // A wider or narrower box wraps its words again (the panel opening, the window resized).
  $effect(() => {
    const el = chat.inputEl;
    if (!el || typeof ResizeObserver === 'undefined') return;
    let width = el.clientWidth;
    const ro = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      fitBox();
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  // **A Code chat's + menu and "/" list** (17 Nov 2026, the Claude Code app's; codeMenu.svelte.js, agents/menu.rs):
  // what its Claude Code offers in the workspace (slash commands and skills, MCP servers, plugins) and the chat's own
  // folders and servers off. "/" at the start of the box opens the commands; a pick puts `/name ` in the box.
  // (ChatView's, shared with the thread's /clear dividers and the edge's "+N folders"; its own without one.)
  // (Given once, at the start: ChatView's lives as long as this box.)
  const given = untrack(() => givenMenu);
  const ownMenu = !given;
  const codeMenu = given ?? new CodeMenu();
  const commands = new CommandPicker(codeMenu);
  $effect(() => () => {
    if (ownMenu) codeMenu.destroy();
  });
  $effect(() => {
    codeMenu.follow(chat.activeId ?? null, code?.current?.path ?? null);
  });

  // **/clear, /compact and /model are Yap's to do** in a Code chat (17 Nov 2026, the Claude Code app's; codeMenu.svelte.js
  // `codeCommand`, agents/slash.rs), picked from the menu or typed — in a Code chat that works directly; anywhere else,
  // and every other "/", the words go as typed. /model switches the chat's model chip, effort kept; with no name, the
  // picker opens.
  /** Enter or Send. */
  function submit(opts) {
    const cmd = code?.current && !chat.attached.length && !chat.files.length && !chat.quote && !chat.sending ? codeCommand(chat.input) : null;
    if (!cmd) return chat.send(opts);
    if (cmd.name === 'model') return modelCommand(cmd.arg);
    if (cmd.name === 'clear') return clearCommand(opts);
    return compactCommand(cmd.arg, opts);
  }
  /** /model [name]: the chat's model chip switched, as picking it there, its effort kept. */
  async function modelCommand(arg) {
    if (!arg) {
      chat.input = '';
      chipRef?.openPicker?.();
      return;
    }
    let status = null;
    try {
      status = await invoke('ai_model_status', { scope: 'chat' });
    } catch {
      /* said below */
    }
    const cur = status?.ready ? status.model : null;
    if (!cur) {
      toast({ title: 'No model to switch from', description: 'Pick one with /model, or in the model chip.', variant: 'destructive' });
      return;
    }
    await loadCatalog();
    const list = modelsFor(cur.provider, cur.id);
    const row = modelFor([...(list.items ?? []), ...(list.more ?? [])], arg);
    if (!row) {
      toast({ title: `No model called "${arg}" on ${providerName(cur)}`, description: 'Type /model on its own to pick one.', variant: 'destructive' });
      return;
    }
    chat.input = '';
    if (row.value === cur.id) {
      toast({ title: `${modelName(cur)} already` });
      return;
    }
    const effortBefore = effortNow(status.effort);
    try {
      let s = await invoke('chat_set_model', { provider: cur.provider, model: row.value, baseUrl: null, file: null });
      const now = s?.ready ? s.model : null;
      // (Its effort as it was: the level kept where the new model has it.)
      if (effortBefore && now && (s.effort?.levels ?? []).includes(effortBefore) && effortNow(s.effort) !== effortBefore) {
        s = await invoke('chat_set_effort', { provider: now.provider, model: now.id, level: effortBefore });
      }
      chipRef?.refresh?.();
      const kept = effortNow(s?.effort);
      toast({ title: `${modelName(now ?? { provider: cur.provider, id: row.value })} now`, description: kept ? `Effort ${effortLabel(kept)}.` : undefined });
    } catch (e) {
      chat.input = `/model ${arg}`;
      toast({ title: "Couldn't switch the model", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
  /** /clear asked about while Claude works (a turn running, or an answer being written). */
  let clearAsk = $state(false);
  let clearBusy = $state(false);
  function clearCommand(opts) {
    if (chat.activeId == null || !chat.messages.length) {
      chat.input = '';
      toast({ title: "There's nothing to clear yet" });
      return;
    }
    if (working) {
      clearAsk = true;
      return;
    }
    doClear(opts);
  }
  async function doClear(opts) {
    const conv = chat.activeId;
    clearBusy = true;
    try {
      // Asked first while it worked: that turn stopped, and waited for (10 s at most).
      const w = workingDirectly(agents.list, conv);
      if (w) {
        await invoke('agent_stop', { id: w.id });
        for (let i = 0; i < 40 && workingDirectly(agents.list, conv); i++) await new Promise((r) => setTimeout(r, 250));
      }
      const r = await invoke('code_chat_clear', { conversation: conv });
      clearAsk = false;
      // (Not a Code chat that works directly: the words go as typed.)
      if (!r?.handled) return chat.send(opts);
      chat.input = '';
      toast({ title: 'Context cleared', description: "Claude starts afresh from your next message. What's above the line stays here for you, but Claude no longer sees it." });
    } catch (e) {
      clearAsk = false;
      toast({ title: "Couldn't clear the conversation", description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      clearBusy = false;
    }
  }
  /** /compact running or just done: a quiet line over the box (`{conv, text, done}`). */
  let compactLine = $state(null);
  async function compactCommand(arg, opts) {
    const conv = chat.activeId;
    if (conv == null || !chat.messages.length) {
      chat.input = '';
      toast({ title: "There's nothing to compact yet" });
      return;
    }
    if (working) {
      toast({ title: 'Claude is working', description: 'Compact once this turn is done.' });
      return;
    }
    const words = chat.input;
    chat.input = '';
    compactLine = { conv, text: 'Compacting the conversation… Claude Code is summarising it to free up context' };
    try {
      const r = await invoke('code_chat_compact', { conversation: conv, instructions: arg || null });
      if (!r?.handled) {
        compactLine = null;
        chat.input = words;
        return chat.send(opts);
      }
      const line = { conv, done: true, text: `Compacted: about ${roughTokens(r.before)} tokens of context down to ${roughTokens(r.after)}` };
      compactLine = line;
      setTimeout(() => {
        if (compactLine === line) compactLine = null;
      }, 10_000);
    } catch (e) {
      compactLine = null;
      chat.input = words;
      toast({ title: "Couldn't compact the conversation", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
  $effect(() => {
    commands.setOn(!!code?.current);
    commands.update(chat.input);
  });
  /** The "/" list's keys, before the box's own (true: taken). */
  function commandKey(e) {
    const picked = commands.key(e);
    if (picked?.command) pickCommand(picked.command);
    return !!picked;
  }
  /** A command picked (the "/" list, Slash commands ›, a plugin's skill): `/name ` in the box, before any words there. */
  function pickCommand(c) {
    if (!c?.name) return;
    chat.plusOpen = false;
    const rest = String(chat.input || '')
      .replace(/^\s*\/[^\s]*\s*/, '')
      .trimStart();
    chat.input = `/${c.name} ${rest}`;
    commands.close();
    tick().then(() => {
      const el = chat.inputEl;
      el?.focus();
      const at = `/${c.name} `.length;
      el?.setSelectionRange?.(at, at);
    });
  }

  // The model chip (its picker opens for "Choose another model…").
  let chipRef = $state(null);
  let plusEl = $state(null);

  const voiceTitle = $derived(
    chat.speakReplies
      ? chat.speakWhen === 'always'
        ? 'Voice replies on: every answer is read aloud'
        : 'Voice replies on: answers to questions you ask by voice are read aloud'
      : 'Voice replies off'
  );

  function removeShot(id) {
    chat.removeShot(id);
    chat.inputEl?.focus();
  }

  /** The suggested model that reads images, picked here (chat_set_model). */
  async function useVisionModel() {
    const s = chat.vision.imagesSwitch;
    if (!s) return;
    try {
      await invoke('chat_set_model', { provider: s.provider, model: s.model });
      await chat.refreshVision();
      chipRef?.refresh?.();
      // Its copies of the screenshots, made now: Send stays instant.
      if (chat.attached.length) invoke('chat_images_prepare', { ids: chat.attached.map((a) => a.id) }).catch(() => {});
      chat.inputEl?.focus();
    } catch (e) {
      toast({ title: "Couldn't switch the model", description: String(e), variant: 'destructive' });
    }
  }

  /** "Read screenshots on this PC": the on-device model's projector, downloaded (or switched back on). */
  async function readHere() {
    const offer = chat.vision.imagesLocal;
    if (!offer) return;
    const ok = offer.installed ? await setVisionOn(true) : await downloadVision(offer.model);
    if (!ok && localVision.error) {
      toast({ title: "Couldn't get the screenshot reader", description: localVision.error, variant: 'destructive' });
    }
    await chat.refreshVision();
    chipRef?.refresh?.();
    chat.inputEl?.focus();
  }

  const fmtMb = (mb) => (mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`);

  /** "Send without the screenshot": for a question that doesn't need it. */
  function sendWithout() {
    chat.clearShots();
    if (chat.input.trim()) chat.send();
    else chat.inputEl?.focus();
  }

  /** "Send its text instead": read on this PC (Windows' OCR), sent in the picture's place. */
  function sendAsText() {
    if (chat.sending) return;
    chat.send({ asText: true });
  }

  /** A screenshot's right-click menu: Open, Copy image, Save image as…. */
  function shotMenu(e, img) {
    openContextMenu(e, imageItems(img, { onview: (i) => (chat.viewing = i) }), { label: 'Screenshot' });
  }

  /** + → "Take a screenshot": Yap's window leaves the shot (snip.rs). */
  function takeScreenshot() {
    chat.plusOpen = false;
    invoke('snip_start', { origin: 'chat' }).catch(() => {});
  }

  async function toggleVoiceReplies() {
    const on = !chat.speakReplies;
    chat.speakReplies = on;
    try {
      await invoke('voice_replies_set', { on });
    } catch (e) {
      chat.speakReplies = !on;
      toast({ title: "Couldn't change voice replies", description: String(e), variant: 'destructive' });
    }
  }

  function moveVolume(v) {
    chat.voiceVolume = v;
    invoke('tts_volume_set', { volume: v }).catch(() => {});
  }
  async function keepVolume() {
    try {
      await invoke('tts_volume_set', { volume: chat.voiceVolume, save: true });
    } catch (e) {
      toast({ title: "Couldn't save the voice volume", description: String(e), variant: 'destructive' });
    }
  }
  function plusOutside(e) {
    if (chat.plusOpen && plusEl && !plusEl.contains(e.target)) chat.plusOpen = false;
  }

  // Connectors ▸ (connectors/chats.rs, a plain chat's): read again as the menu opens.
  $effect(() => {
    if (chat.plusOpen && plain) chat.connectors.refresh();
  });
  async function flipConnector(c) {
    const r = await chat.connectors.toggle(c.id, !c.on);
    if (!r.ok) toast({ title: `Couldn't switch ${c.name} for this chat`, description: r.why, variant: 'destructive' });
  }
  function manageConnectors() {
    chat.plusOpen = false;
    onopensettings?.('connectors');
  }

  // **A plain chat** (16 Nov 2026, the Claude app's box): two rows — the words, then + on the left and the mic on the
  // right —, the start screen's "How can I help you today?", and the mic's tooltip names the Voice Assistant's
  // shortcut (where the empty chat's "Or press 9 in any app…" line was). A Code chat's box as ever.
  const plain = $derived(!code?.current && !code?.onCode);
  const micTitle = $derived(
    plain && chat.assistantHotkey ? `Dictate. Or press ${formatHotkeySpec(chat.assistantHotkey)} in any app to ask out loud` : 'Dictate your message'
  );

  // **A plain chat's + menu** (16 Nov 2026, the Claude desktop app's, in Yap's items): Upload or add files (Ctrl+U)
  // and Take a screenshot; Skills › (the "/" menu's list: a pick goes in the box) and Connectors › (with the preview
  // on: this chat's switches); Research (✓ when on) and Style › (the chat's style, Create & edit styles…); Voice
  // replies, their volume, Voice settings…. Icon rows, dividers, › for a submenu, ✓ for what's on. Enter, Space or
  // ↓ on + opens it at its first row, ↑ at its last; ↑ ↓ Home End move, → opens a submenu, ← or Esc closes it, Esc
  // closes the menu and gives + the focus back. On the start screen it opens downwards, as the app's does, unless
  // there's no room. What's on shows in the box as a chip (Research ×, Concise ×). A Code chat's has the same look
  // and keys with the Claude Code app's items (`codeMenuView`, 17 Nov 2026).
  let plusBtn = $state(null);
  let menuEl = $state(null);
  /** The submenu open: 'skills' | 'connectors' | 'style' (a plain chat's), 'commands' | 'plugins' (a Code chat's) | null. */
  let sub = $state(null);
  /** A Code chat's Plugins ›: the plugin whose skills are open beside it, and the search box's words. */
  let sub2 = $state(null);
  let pluginQuery = $state('');
  /** A plugin's skills open on the other side when the window has no room for them on the right. */
  let sub3Flip = $state(false);
  $effect(() => {
    sub3Flip = false;
    if (!sub2) return;
    tick().then(() => {
      const r = menuEl?.querySelector('.sub3')?.getBoundingClientRect();
      if (r && r.right > window.innerWidth - 8) sub3Flip = true;
    });
  });
  const serverList = $derived(serverRows(codeMenu.offered, codeMenu.serversOff, pluginQuery));
  const pluginList = $derived(pluginRows(codeMenu.offered, pluginQuery));
  /** Slash commands ›'s search box: by name (an alias too) and line, best first; empty, the groups. */
  let commandQuery = $state('');
  const commandList = $derived(commandQuery.trim() ? [{ label: '', items: matchCommands(codeMenu.commands, commandQuery.trim()), start: 0 }].filter((g) => g.items.length) : commandGroups(codeMenu.commands));
  /** Said where a list is empty: being looked at, Claude Code not there, or nothing yet. */
  const offeredNote = $derived(
    !codeMenu.loaded || codeMenu.looking ? 'Asking Claude Code what it offers here…' : codeMenu.error || (codeMenu.offered ? '' : "Claude Code's lists show here once it has answered in this workspace.")
  );
  async function flipServer(s) {
    const r = await codeMenu.setServer(s.name, !s.on);
    if (!r.ok) toast({ title: `Couldn't switch ${s.label} for this chat`, description: r.why, variant: 'destructive' });
  }
  /** Add folder: a folder on this PC Claude Code may use in this chat too (`--add-dir`), shown in the summary panel. */
  async function addFolder() {
    chat.plusOpen = false;
    const r = await codeMenu.addFolder();
    if (!r.ok) toast({ title: "Couldn't add that folder", description: r.why, variant: 'destructive' });
    else if (r.added) {
      const f = codeMenu.folders.at(-1);
      toast({ title: `Claude Code can use ${f?.name || 'that folder'} in this chat too`, description: 'It shows in the summary panel, where you can take it off.' });
    }
    chat.inputEl?.focus();
  }
  function managePlugins() {
    chat.plusOpen = false;
    onopensettings?.('yap-code#yap-code-plugins');
  }
  let menuDown = $state(false);
  /** Opened from the keyboard: which row takes the focus ('first' | 'last'), or null (a click). */
  let focusOnOpen = null;
  const style = new ChatStyle();
  $effect(() => {
    if (plain) style.follow(chat.activeId);
  });
  $effect(() => {
    if (plain) untrack(() => style.loadList());
  });
  // Settings → Skills → Chat styles changed them (or the style new chats start with): read again.
  $effect(() => {
    const again = () => {
      style.loadList();
      style.follow(untrack(() => chat.activeId));
    };
    window.addEventListener('yap-chat-styles-changed', again);
    return () => window.removeEventListener('yap-chat-styles-changed', again);
  });
  // Can Chat's model research now (asked again when the model changes)?
  $effect(() => {
    if (!plain) return;
    void chat.limits?.label;
    void chat.limits?.provider;
    refreshResearch();
  });
  const researchShown = $derived(plain && research.on && research.can);
  const styleShown = $derived(plain && style.current !== 'normal' && !!style.name);
  const skillGroups = $derived(plain ? skillMenu(skillList.list, '', 'meetings').groups : []);
  const researchTitle = $derived(
    !research.can && research.why
      ? research.why
      : research.on
        ? 'Research is on: each question gets a plan, many searches and a report beside the chat. Click to turn it off.'
        : 'Research: a plan, many searches and page reads, then a long report with citations'
  );

  function plusClick(e) {
    // (Enter or Space on the button: a click with no pointer.)
    focusOnOpen = !chat.plusOpen && e.detail === 0 ? 'first' : null;
    chat.plusOpen = !chat.plusOpen;
  }
  function plusKey(e) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    focusOnOpen = e.key === 'ArrowDown' ? 'first' : 'last';
    if (chat.plusOpen) focusMenu(focusOnOpen);
    else chat.plusOpen = true;
  }
  // Opened: downwards on the start screen when there's room; its first (or last) row focused when the keyboard opened it.
  // (A Code chat's: upwards, its lists read again — what its Claude Code offers, the chat's folders and servers.)
  $effect(() => {
    if (!chat.plusOpen) {
      sub = null;
      sub2 = null;
      pluginQuery = '';
      commandQuery = '';
      return;
    }
    untrack(() => {
      if (!plain && code?.current) codeMenu.load();
      menuDown = plain && chat.messages.length === 0;
      tick().then(() => {
        if (menuDown && menuEl && menuEl.getBoundingClientRect().bottom > window.innerHeight - 8) menuDown = false;
        if (focusOnOpen) focusMenu(focusOnOpen);
        focusOnOpen = null;
      });
    });
  });
  const topRows = () => [...(menuEl?.querySelectorAll('[data-mi="top"]') ?? [])];
  function focusMenu(where) {
    const rows = topRows();
    (where === 'last' ? rows.at(-1) : rows[0])?.focus();
  }
  /** Submenu `name` open (a click, the pointer on its row, →); `focus`: its first row takes the focus (the keyboard's →). */
  async function openSub(name, focus = false) {
    sub = name;
    if (focus) {
      await tick();
      menuEl?.querySelector('.subpop [data-mi="sub"]')?.focus();
    }
  }
  function closeSub() {
    const row = menuEl?.querySelector(`[data-sub="${sub}"]`);
    sub = null;
    sub2 = null;
    row?.focus();
  }
  /** A plugin's skills (a Code chat's Plugins ›): open beside the list; `focus`: its first row (the keyboard's →). */
  async function openSub2(name, focus = false) {
    sub2 = name;
    if (focus) {
      await tick();
      menuEl?.querySelector('.sub3 [data-mi="sub3"]')?.focus();
    }
  }
  function closeSub2() {
    const row = [...(menuEl?.querySelectorAll('[data-sub2]') ?? [])].find((r) => r.dataset.sub2 === sub2);
    sub2 = null;
    row?.focus();
  }
  function closeMenu() {
    chat.plusOpen = false;
    plusBtn?.focus();
  }
  function menuKey(e) {
    const inSub3 = e.target?.closest?.('.sub3');
    const inSub = inSub3 ? null : e.target?.closest?.('.subpop');
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (inSub3 && sub2) closeSub2();
      else if ((inSub || inSub3) && sub) closeSub();
      else closeMenu();
      return;
    }
    // (The volume slider keeps its own arrows; the plugins' search box its caret's.)
    if (e.target?.matches?.('input[type="range"]')) return;
    if (e.target?.matches?.('input[type="search"]') && ['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
    if (e.key === 'ArrowLeft' && inSub3) {
      e.preventDefault();
      closeSub2();
      return;
    }
    if (e.key === 'ArrowLeft' && inSub) {
      e.preventDefault();
      closeSub();
      return;
    }
    if (e.key === 'ArrowRight' && !inSub && !inSub3 && e.target?.dataset?.sub) {
      e.preventDefault();
      openSub(e.target.dataset.sub, true);
      return;
    }
    if (e.key === 'ArrowRight' && inSub && e.target?.dataset?.sub2) {
      e.preventDefault();
      openSub2(e.target.dataset.sub2, true);
      return;
    }
    const rows = inSub3 ? [...inSub3.querySelectorAll('[data-mi="sub3"]')] : inSub ? [...inSub.querySelectorAll('[data-mi="sub"]')] : topRows();
    if (!rows.length) return;
    const at = rows.indexOf(document.activeElement);
    let to = null;
    if (e.key === 'ArrowDown') to = at < 0 ? 0 : (at + 1) % rows.length;
    else if (e.key === 'ArrowUp') to = at < 0 ? rows.length - 1 : (at - 1 + rows.length) % rows.length;
    else if (e.key === 'Home') to = 0;
    else if (e.key === 'End') to = rows.length - 1;
    if (to == null) return;
    e.preventDefault();
    rows[to].focus();
  }

  /** Research from the menu (as the chip under the box was): asked first whether Chat's model can. */
  async function flipResearch() {
    await refreshResearch();
    if (!research.can) {
      research.on = false;
      toast({ title: "Research isn't available", description: research.why || "Chat's model can't research now." });
      return;
    }
    research.on = !research.on;
    chat.plusOpen = false;
    chat.inputEl?.focus();
  }
  function researchOff() {
    research.on = false;
    chat.inputEl?.focus();
  }
  async function pickStyle(id) {
    chat.plusOpen = false;
    await style.pick(id);
    chat.inputEl?.focus();
  }
  async function styleOff() {
    await style.pick('normal');
    chat.inputEl?.focus();
  }
  // Create & edit styles… (StylesDialog.svelte, its own chunk).
  let StylesDialog = $state(null);
  let stylesOpen = $state(false);
  async function openStyles() {
    chat.plusOpen = false;
    if (!StylesDialog) StylesDialog = (await import('./StylesDialog.svelte')).default;
    await style.loadList();
    stylesOpen = true;
  }
  async function stylesClosed() {
    stylesOpen = false;
    await style.loadList();
    await style.follow(chat.activeId);
    chat.inputEl?.focus({ preventScroll: true });
  }
  /** A Skill from Skills ›: in the box as "/command ", for words to follow (as Tab in the "/" menu). */
  function pickSkillHere(s) {
    chat.plusOpen = false;
    chat.pickSkill(s, false);
  }
  function moreSkills() {
    chat.plusOpen = false;
    openMoreSkills();
  }
  function uploadFiles() {
    chat.pickFiles();
  }

  // The mic: a dictation into the chat box (chat_dictate: the text comes
  // back as `yap-chat-dictation`, at the caret). Again: stop and send it in.
  function micChat() {
    chat.inputEl?.focus();
    invoke('chat_dictate').catch(() => {});
  }
</script>

<svelte:window onpointerdown={plusOutside} />

<!-- Composer (after Claude Code's): + (voice replies and their volume),
     the message, the mic, and Send once there's something to send;
     under it, quiet, whether answers are read aloud and the model the
     message goes to. The model picker opens above it all. With Yap Code on,
     every box keeps room for a Code chat's label on its edge: a chat
     turning into a Code chat (its agent starting) never moves the thread. -->
<div
  class="composer"
  class:dropping={chat.dragging}
  class:code-chat={!!code?.current}
  class:code-room={!!code?.enabled}
  class:plain
  style:--thread-gutter="{chat.threadGutter}px"
  use:keepClear
  data-model-picker-above
>
  {#if chat.dragging}
    <!-- Files dragged over Chat: a drop puts the pictures in the box. -->
    <div class="dropnote" data-testid="chat-drop-note" aria-live="polite">Drop files or pictures to ask about them</div>
  {/if}
  {#if chat.attached.length}
    <!-- Snip to Chat (snip.rs): the screenshots this question carries, three
         one-tap questions, and — a model that can't read images — one line
         saying so, with the switch. Nothing goes until you send. -->
    <div class="snipbar" data-testid="chat-attachments">
      {#if chat.visionBlocked}
        {@const here = chat.vision.imagesLocal}
        <div class="visionline" role="status" data-testid="chat-vision-line">
          {#if here && localVision.downloading}
            <!-- The on-device model's projector downloading (it carries on if you leave). -->
            <div class="vprogress" data-testid="chat-vision-download">
              <DownloadProgress
                progress={fromEvent(localVision.progress)}
                label="Downloading the screenshot reader"
                oncancel={() => cancelVision(here.model)}
              />
            </div>
          {/if}
          <span class="vtext">
            {#if here && here.failed}
              <b>{modelName(chat.vision.model)} couldn't load its screenshot reader.</b> Until Yap restarts:
            {:else if here && here.installed}
              <b>Reading screenshots on this PC is off.</b>
            {:else if here}
              <b>{modelName(chat.vision.model)} can read screenshots on this PC</b> with a {fmtMb(here.sizeMb)} download: free, and the picture never leaves it.
            {:else}
              <b>{modelName(chat.vision.model)} can't read images.</b>
              {#if !chat.vision.imagesSwitch}None of your models can: Claude, Gemini and OpenAI models can.{/if}
            {/if}
          </span>
          {#if here && !here.failed}
            <button type="button" class="vbtn primary" disabled={!!localVision.downloading} onclick={readHere}>
              {here.installed ? 'Turn it on' : `Download it (${fmtMb(here.sizeMb)})`}
            </button>
          {/if}
          {#if chat.vision.imagesSwitch}
            <button type="button" class="vbtn" class:primary={!here || here.failed} onclick={useVisionModel}>
              Use {chat.switchName} on {providerName({ provider: chat.vision.imagesSwitch.provider })}
            </button>
          {/if}
          <button
            type="button"
            class="vbtn"
            class:primary={!chat.vision.imagesSwitch && (!here || here.failed)}
            disabled={chat.sending}
            title="Windows reads the screenshot's text on this PC, and that goes to the model instead of the picture"
            onclick={sendAsText}>Send its text instead</button
          >
          <button type="button" class="vbtn" onclick={() => chipRef?.openPicker()}>
            {chat.vision.imagesSwitch ? 'Choose another model…' : 'Connect one…'}
          </button>
          <button type="button" class="vlink" onclick={sendWithout}>Send without the screenshot</button>
        </div>
      {/if}
      <div class="shotrow">
        {#each chat.attached as img (img.id)}
          <!-- A 120 px tile, as the Claude app's (chatImages.js `placed`): the
               whole picture on a fill, at exactly its device pixels (Rust's
               thumbnail, snip/thumbs.rs), the ring over its edge; the ✕ and
               its size on hover. -->
          {@const t = tilePlaced(img, screenPixels.ratio)}
          <div class="shotchip shotframe" style:width="{t.frame.w}px" style:aspect-ratio="{t.frame.w} / {t.frame.h}">
            <button
              type="button"
              class="thumb"
              title="Open the screenshot (right-click to copy or save it)"
              aria-label={`Screenshot, ${sizeLine(img)}`}
              onclick={() => (chat.viewing = img)}
              oncontextmenu={(e) => shotMenu(e, img)}
            >
              <img
                src={t.src}
                alt=""
                draggable="false"
                decoding="async"
                use:shown
                style:left={t.pic.x}
                style:top={t.pic.y}
                style:width={t.pic.w}
                style:height={t.pic.h}
              />
            </button>
            <span class="shotsize">{sizeLine(img)}</span>
            <button type="button" class="shotx" aria-label="Remove the screenshot" title="Remove" onclick={() => removeShot(img.id)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
            </button>
          </div>
        {/each}
        <!-- (No one-tap prompts beside it since 14 Nov 2026, the maintainer: "It's not needed". The box asks
             "Ask about this screenshot…", and Enter on it empty asks "What's this?".) -->
      </div>
      {#if chat.copiedShot && chat.attached.some((a) => a.id === chat.copiedShot)}
        <!-- "Copy every snip to the clipboard" (snip.rs): said once, quietly. -->
        <div class="copiedline" role="status" data-testid="chat-snip-copied">Copied · Ctrl+V to paste it anywhere</div>
      {/if}
    </div>
  {/if}
  {#if chat.files.length || chat.readingFiles.length}
    <!-- Files attached (chat/attachments.rs): a chip each — its type, name and size, as the Claude app's — ✕ to take
         it out, a click opens it. Their text goes before the question. One still being read (a scan's OCR): "Reading…". -->
    <div class="elrow filerow" data-testid="chat-files">
      {#each chat.files as f (f.id)}
        <FileChip file={f} onremove={chat.removeFile} />
      {/each}
      {#each chat.readingFiles as f (f.key)}
        <FileChip file={f} />
      {/each}
    </div>
  {/if}
  {#if mentioned.length}
    <!-- The workspace files the message mentions (@path, as Claude Code reads it): a chip each, ✕ takes it out. -->
    <div class="elrow filerow" data-testid="chat-mentions">
      {#each mentioned as p (p)}
        <span class="mentionchip" title={p}>
          <span class="mname">{p.split('/').pop() || p}</span>
          {#if p.includes('/')}<span class="mdir">{p.slice(0, p.lastIndexOf('/'))}</span>{/if}
          <button type="button" class="mx" aria-label={`Remove @${p}`} title="Remove" onclick={() => chat.dropMention(p)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
          </button>
        </span>
      {/each}
    </div>
  {/if}
  {#if picked.list.length}
    <!-- Elements picked on a page beside the chat (PageTab's Select, chat/pages/pick.rs): a chip each,
         its card on hover — as the Claude Code app attaches them to the prompt. -->
    <div class="elrow" data-testid="chat-elements">
      {#each picked.list as el (el.uid)}
        <ElementChip {el} picture={el.image} onremove={() => removePicked(el.uid)} />
      {/each}
    </div>
  {/if}
  {#if chat.inputTooLong || chat.tooLong}
    <!-- Too long for the model (chat/budget.rs): said plainly, before it's
         sent (a paste the model can't take) or after the provider said so,
         with what helps. Never the provider's own error. -->
    <div class="visionline toolong" role="alert" data-testid="chat-too-long">
      <span class="vtext">{chat.inputTooLong ? tooLongLine(chat.limits, chat.inputTokens) : chat.tooLong}</span>
      {#if !chat.inputTooLong && chat.activeId != null}
        <button type="button" class="vbtn" onclick={() => chat.newChat({ keepInput: true })}>New chat</button>
      {/if}
      {#if chat.limits?.more}
        <button type="button" class="vbtn primary" onclick={chat.giveMoreRoom}>Give it more room</button>
      {/if}
      <button type="button" class="vlink" onclick={chat.shortenPaste}>Shorten what you pasted</button>
    </div>
  {/if}
  {#if chat.quote}
    <!-- A passage of the open document ("Ask about this", "Change this…"):
         the question is about it, and the model is told which part it is. -->
    <div class="quotebar" data-testid="chat-quote">
      <svg class="quoteicon" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M9.5 6C6.5 7 5 9.4 5 12.5V18h5.5v-5.5H8c0-2 .8-3.3 2.5-4zm9 0c-3 1-4.5 3.4-4.5 6.5V18h5.5v-5.5H17c0-2 .8-3.3 2.5-4z" /></svg>
      <span class="quotetext" title={chat.quote.text}>
        <span class="quotefrom">{chat.quote.mode === 'change' ? 'Change in' : 'About'} {chat.quote.title || 'the document'}:</span>
        {chat.quote.text}
      </span>
      <button type="button" class="quotex" aria-label="Remove the quoted passage" title="Remove" onclick={chat.clearQuote}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
      </button>
    </div>
  {/if}
  {#if compactLine && compactLine.conv === chat.activeId}
    <!-- /compact (agents/slash.rs): quiet while Claude Code summarises the session, then what it came to. -->
    <div class="cmdline" class:done={compactLine.done} aria-live="polite" data-testid="code-compact-line">
      {#if !compactLine.done}<span class="cmddots" aria-hidden="true"><span></span><span></span><span></span></span>{/if}
      <span>{compactLine.text}</span>
    </div>
  {/if}
  <div class="inputbar">
    <CodeEdge {code} conversation={chat.activeId} folders={codeMenu.folders} />
    <div class="pluswrap" bind:this={plusEl}>
      <button
        class="plusbtn"
        bind:this={plusBtn}
        aria-label="Chat options"
        aria-haspopup="dialog"
        aria-expanded={chat.plusOpen}
        title={plain ? 'Add files, Skills, Research, Style and more' : code?.current ? 'Add files or a folder, slash commands, plugins and more' : chat.speakReplies ? `Voice replies on · ${Math.round(chat.voiceVolume * 100)}%` : 'Voice replies and volume'}
        onclick={plusClick}
        onkeydown={plusKey}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
        <!-- Voice replies are on: a small amber dot, the switch is inside. -->
        {#if chat.speakReplies}<span class="plusdot" aria-hidden="true"></span>{/if}
      </button>
      {#if chat.plusOpen && plain}
        {@render plainMenu()}
      {:else if chat.plusOpen}
        {@render codeMenuView()}
      {/if}
    </div>
    {#if plain}
      <!-- What's on for this chat, beside the + as the Claude app shows it: Research, a Style other than Normal; × turns
           it off (Normal). Nothing at Normal with Research off. Then the room before the buttons on the right. -->
      {#if researchShown || styleShown}
        <span class="modechips" data-testid="chat-box-modes">
          {#if researchShown}
            <span class="modechip on" data-testid="chat-research-on" title="Research is on: each question gets a plan, many searches and a report beside the chat">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6" /><path d="m15 15 5 5" /><path d="M8 10.5h5M10.5 8v5" /></svg>
              <span class="mname">Research</span>
              <button type="button" class="mcx" aria-label="Turn off Research" title="Turn off Research" onclick={researchOff}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
              </button>
            </span>
          {/if}
          {#if styleShown}
            <span class="modechip" data-testid="chat-style-on" title={`Answers in this chat are written in the ${style.name} style`}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16z" /><path d="M13.5 6.5l4 4" /></svg>
              <span class="mname">{style.name}</span>
              <button type="button" class="mcx" aria-label={`Stop using ${style.name}: back to Normal`} title="Back to Normal" onclick={styleOff}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
              </button>
            </span>
          {/if}
        </span>
      {/if}
      <span class="rowgap" aria-hidden="true"></span>
    {/if}
    <!-- "/": Skills, run across the meetings (chat/skills.rs); in a Code chat, its Claude Code's commands. -->
    <SkillMenu picker={chat.skills} onpick={chat.pickSkill} />
    <CommandMenu picker={commands} onpick={pickCommand} />
    <!-- "@" in a Code chat: its workspace's files (agents/mention.rs). -->
    <MentionMenu picker={chat.mention} onpick={chat.pickMention} />
    <textarea
      class="msginput"
      rows="1"
      bind:this={chat.inputEl}
      bind:value={chat.input}
      placeholder={chat.quote
        ? chat.quote.mode === 'change'
          ? 'How should it change?'
          : 'Ask about this passage…'
        : chat.attached.length
          ? 'Ask about this screenshot…'
          : chat.files.length
            ? chat.files.length > 1
              ? 'Ask about these files…'
              : 'Ask about this file…'
            : queueHint
            ? queueHint
            : code?.current
            ? 'Type / for commands'
            : code?.onCode
            ? 'Ask about your notes, or type / for Skills'
            : chat.messages.length
            ? 'Reply…'
            : 'How can I help you today?'}
      aria-label="Message"
      aria-busy={chat.sending}
      aria-autocomplete="list"
      aria-controls={commands.open ? 'cmd-list' : chat.mention.open ? 'mention-list' : chat.skills.open ? 'skill-list' : undefined}
      aria-activedescendant={commands.open && commands.items.length ? `cmd-opt-${commands.active}` : chat.mention.open && chat.mention.items.length ? `mention-opt-${chat.mention.active}` : chat.skills.open ? `skill-opt-${chat.skills.active}` : undefined}
      onkeydown={keydown}
      onkeyup={(e) => (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'ArrowUp' || e.key === 'ArrowDown' || e.key === 'Home' || e.key === 'End') && chat.mentionInput()}
      onclick={() => chat.mentionInput()}
      onpaste={chat.pasteImage}
      oninput={() => {
        chat.mentionInput();
        if (!chat.input.trim()) {
          chat.byVoice = false;
          chat.tooLong = '';
        }
      }}
      onfocus={() => chat.reportFocus(true)}
      onblur={() => {
        chat.reportFocus(false);
        chat.closeMention();
      }}
    ></textarea>
    <!-- Not `disabled` while an answer comes: disabling the focused box
         throws the caret out of it, and you'd have to click back in to
         type the next message. `send()` ignores Enter until the answer is
         in, so you can type ahead. -->
    <!-- Think it through (chat/think.rs): this question only. -->
    <ThinkButton {chat} />
    <button class="micbtn" title={micTitle} aria-label="Dictate your message" onclick={micChat}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
    </button>
    {#if chat.sending || chat.assistantAnswering}
      <!-- ■: stop the answer being written (Esc too), a spoken one's voice and all. -->
      <button class="sendbtn stopbtn" disabled={chat.stopping} onclick={chat.stopAnswer} aria-label="Stop the answer" title="Stop (Esc)">
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
      </button>
    {:else if chat.input.trim() || chat.attached.length || chat.files.length}
      <button
        class="sendbtn"
        disabled={chat.sending || chat.visionBlocked || chat.inputTooLong}
        onclick={() => submit()}
        aria-label="Send"
        title={chat.visionBlocked ? "This model can't read images" : chat.inputTooLong ? 'Too long for this model' : working ? 'Send (Enter) · Ctrl+Enter queues it for later' : 'Send (Enter)'}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
      </button>
    {:else if working}
      <!-- ■: stop the Code chat's own session, at work (Esc in the empty box too). -->
      <button class="sendbtn stopbtn" data-testid="chat-stop-work" disabled={stoppingWork === working.id} onclick={stopWork} aria-label="Stop the work" title="Stop the work (Esc)">
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
      </button>
    {/if}
  </div>
  <div class="composerfoot">
    <CodeChips {chat} {code} />
    <ModelChip bind:this={chipRef} {onopensettings} quiet conversationId={chat.activeId} />
    <!-- (A plain chat's Style and Research are in the + menu since 16 Nov 2026, their chips in the box while on.) -->
    <!-- How much of the model's context this chat uses (bottom right, as
         Claude's chat box shows it); its panel has the day's AI use. -->
    <span class="footgap"></span>
    <ContextRing conversationId={chat.activeId} stamp={chat.contextStamp} onnewchat={() => chat.newChat()} onseeall={() => onopenusage?.()} />
  </div>
</div>

<ConfirmDialog
  bind:open={clearAsk}
  title="Clear the conversation?"
  body="Claude is working on a turn now: clearing stops it, and Claude starts afresh from your next message. The messages stay in the thread above a line, but Claude no longer sees them."
  confirmLabel="Stop and clear"
  busyLabel="Clearing…"
  busy={clearBusy}
  onconfirm={() => doClear()}
/>

{#if StylesDialog && stylesOpen}
  <StylesDialog onclose={stylesClosed} onpick={(id) => style.pick(id)} />
{/if}

{#snippet chev()}
  <svg class="mside mchev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
{/snippet}
{#snippet tick_()}
  <svg class="mside mtick" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>
{/snippet}

{#snippet plainMenu()}
  <!-- A plain chat's + menu (the Claude desktop app's shape, Yap's own items). A dialog, not a menu: it holds the
       voice's volume slider; its rows are buttons, ↑ ↓ between them. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="pluspop plainmenu" class:down={menuDown} role="dialog" aria-label="Chat options" tabindex="-1" bind:this={menuEl} onkeydown={menuKey} data-testid="chat-plus-menu">
    <!-- Attach files (chat/attachments.rs): their text goes with the question. -->
    <button class="mrow" data-mi="top" aria-label="Upload or add files" aria-keyshortcuts="Control+U" data-testid="chat-attach-files" onclick={uploadFiles}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21.4 11.1-8.5 8.5a5.5 5.5 0 0 1-7.8-7.8l8.5-8.5a3.7 3.7 0 0 1 5.2 5.2l-8.5 8.5a1.8 1.8 0 0 1-2.6-2.6l7.8-7.8" /></svg>
      <span class="mlabel">Upload or add files</span>
      <kbd class="mside mkey">Ctrl+U</kbd>
    </button>
    <!-- Snip to Chat (snip.rs): this window leaves the shot. -->
    <button class="mrow" data-mi="top" aria-label="Take a screenshot" onclick={takeScreenshot}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="6" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><path d="M20 4 8.1 15.9" /><path d="M14.5 14.5 20 20" /><path d="M8.1 8.1 12 12" /></svg>
      <span class="mlabel">Take a screenshot</span>
      {#if chat.snipHotkey}<kbd class="mside mkey">{formatHotkeySpec(chat.snipHotkey)}</kbd>{/if}
    </button>
    <div class="optsep" role="separator"></div>
    <!-- Skills ›: the "/" menu's list (chat/skills.rs); a pick goes in the box as "/command ". -->
    <div class="optsub">
      <button
        class="mrow"
        data-mi="top"
        data-sub="skills"
        aria-haspopup="menu"
        aria-expanded={sub === 'skills'}
        aria-label="Skills"
        data-testid="chat-plus-skills"
        onclick={(e) => openSub('skills', e.detail === 0)}
        onpointerenter={() => (sub = 'skills')}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="3.5" width="17" height="17" rx="4" /><path d="m14.5 7.5-5 9" /></svg>
        <span class="mlabel">Skills</span>
        {@render chev()}
      </button>
      {#if sub === 'skills'}
        <div class="pluspop subpop skillsub" role="menu" aria-label="Skills" data-testid="chat-plus-skills-menu">
          {#each skillGroups as g (g.label)}
            {#if g.label}<div class="mhead" role="presentation">{g.label}</div>{/if}
            {#each g.items as s (s.id)}
              <button class="mrow" role="menuitem" data-mi="sub" title={s.description || s.name} onclick={() => pickSkillHere(s)}>
                <span class="mlabel"><span class="mcmd">/{s.command}</span> <span class="mquiet">{s.name}</span></span>
              </button>
            {/each}
          {:else}
            <div class="mhead" role="presentation">No Skills in the / menu</div>
          {/each}
          <div class="optsep" role="separator"></div>
          <button class="mrow" role="menuitem" data-mi="sub" onclick={moreSkills}><span class="mlabel mquiet">More Skills…</span></button>
        </div>
      {/if}
    </div>
    {#if chat.connectors.shown}
      <!-- Connectors › (connectors/chats.rs): this chat's switches, as Claude's composer has. -->
      <div class="optsub">
        <button
          class="mrow"
          data-mi="top"
          data-sub="connectors"
          aria-haspopup="true"
          aria-expanded={sub === 'connectors'}
          aria-label="Connectors"
          data-testid="chat-connectors-row"
          onclick={(e) => openSub('connectors', e.detail === 0)}
          onpointerenter={() => (sub = 'connectors')}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 7V3M15 7V3" /><path d="M6 7h12v4a6 6 0 0 1-12 0z" /><path d="M12 17v4" /></svg>
          <span class="mlabel">Connectors</span>
          <span class="mside mcount">{chat.connectors.onCount} of {chat.connectors.list.length} on</span>
          {@render chev()}
        </button>
        {#if sub === 'connectors'}
          <div class="pluspop subpop" role="group" aria-label="Connectors in this chat" data-testid="chat-connectors-menu">
            {#each chat.connectors.list as c (c.id)}
              <button class="mrow" data-mi="sub" aria-pressed={c.on} aria-label={c.name} title={c.on ? `${c.name} is on in this chat` : `${c.name} is off in this chat`} onclick={() => flipConnector(c)}>
                <ConnectorLogo logo={c.logo} name={c.name} color={c.color} size={18} />
                <span class="mlabel">{c.name}</span>
                {#if c.on}{@render tick_()}{/if}
              </button>
            {/each}
            <div class="optsep" role="separator"></div>
            <button class="mrow" data-mi="sub" onclick={manageConnectors}><span class="mlabel mquiet">Manage connectors…</span></button>
          </div>
        {/if}
      </div>
    {/if}
    <div class="optsep" role="separator"></div>
    <!-- Research (chat/research.rs): ✓ while on; greyed, with why, when Chat's model can't. -->
    <button
      class="mrow"
      class:unavail={!research.can}
      data-mi="top"
      aria-pressed={research.on && research.can}
      aria-label="Research"
      title={researchTitle}
      data-testid="chat-plus-research"
      onclick={flipResearch}
      onpointerenter={() => (sub = null)}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6" /><path d="m15 15 5 5" /><path d="M8 10.5h5M10.5 8v5" /></svg>
      <span class="mlabel">Research</span>
      {#if research.on && research.can}{@render tick_()}{/if}
    </button>
    <!-- Style › (chat/styles.rs): the chat's own, the one in use ticked; Create & edit styles…. -->
    <div class="optsub">
      <button
        class="mrow"
        data-mi="top"
        data-sub="style"
        aria-haspopup="menu"
        aria-expanded={sub === 'style'}
        aria-label={`Style: ${style.name || 'Normal'}`}
        data-testid="chat-plus-style"
        onclick={(e) => openSub('style', e.detail === 0)}
        onpointerenter={() => (sub = 'style')}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16z" /><path d="M13.5 6.5l4 4" /></svg>
        <span class="mlabel">Style</span>
        <span class="mside mcount">{style.name || 'Normal'}</span>
        {@render chev()}
      </button>
      {#if sub === 'style'}
        <div class="pluspop subpop" role="menu" aria-label="Style" data-testid="chat-plus-style-menu">
          {#each style.builtins as s (s.id)}
            <button class="mrow" role="menuitemradio" aria-checked={s.id === style.current} data-mi="sub" title={s.description || undefined} onclick={() => pickStyle(s.id)}>
              <span class="mlabel">{s.name}</span>
              {#if s.id === style.current}{@render tick_()}{/if}
            </button>
          {/each}
          {#if style.mine.length}<div class="optsep" role="separator"></div>{/if}
          {#each style.mine as s (s.id)}
            <button class="mrow" role="menuitemradio" aria-checked={s.id === style.current} data-mi="sub" onclick={() => pickStyle(s.id)}>
              <span class="mlabel">{s.name}</span>
              {#if s.id === style.current}{@render tick_()}{/if}
            </button>
          {/each}
          <div class="optsep" role="separator"></div>
          <button class="mrow" role="menuitem" data-mi="sub" onclick={openStyles}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></svg>
            <span class="mlabel">Create & edit styles…</span>
          </button>
        </div>
      {/if}
    </div>
    <div class="optsep" role="separator"></div>
    {@render voiceRows()}
  </div>
{/snippet}

{#snippet voiceRows()}
  <!-- Spoken replies on / off (Settings → Voice Assistant → Speak replies), their volume, Voice settings…. -->
  <button class="mrow" data-mi="top" aria-pressed={chat.speakReplies} aria-label="Voice replies" title={voiceTitle} onclick={toggleVoiceReplies} onpointerenter={() => (sub = null)}>
    {#if chat.speakReplies}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.4 5.6a9 9 0 0 1 0 12.8" /></svg>
    {:else}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" /><path d="m22 9-6 6M16 9l6 6" /></svg>
    {/if}
    <span class="mlabel">Voice replies</span>
    {#if chat.speakReplies}{@render tick_()}{/if}
  </button>
  <div class="mvol">
    <input
      id="chat-voice-volume"
      type="range"
      min="0"
      max="100"
      step="5"
      aria-label="Voice volume"
      value={Math.round(chat.voiceVolume * 100)}
      oninput={(e) => moveVolume(Number(e.currentTarget.value) / 100)}
      onchange={keepVolume}
    />
    <span class="volval">{Math.round(chat.voiceVolume * 100)}%</span>
  </div>
  <button
    class="mrow"
    data-mi="top"
    onclick={() => {
      chat.plusOpen = false;
      onopensettings?.('voice');
    }}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></svg>
    <span class="mlabel">Voice settings…</span>
  </button>
{/snippet}

{#snippet plugIcon()}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 7V3M15 7V3" /><path d="M6 7h12v4a6 6 0 0 1-12 0z" /><path d="M12 17v4" /></svg>
{/snippet}

{#snippet codeMenuView()}
  <!-- **A Code chat's + menu** (17 Nov 2026, the Claude Code app's shape, Yap's items: agents/menu.rs): Add files or
       photos (Ctrl+U), Take a screenshot, Add folder (a folder Claude Code may use in this chat too), Slash commands ›
       (what its Claude Code offers here: a pick goes in the box), Plugins › (Search plugins, the session's MCP servers
       and connectors with a switch each for this chat, the plugins with their skills, Manage plugins…); then Voice
       replies, their volume, Voice settings…. The plain chat's look and keys. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="pluspop plainmenu" role="dialog" aria-label="Chat options" tabindex="-1" bind:this={menuEl} onkeydown={menuKey} data-testid="code-plus-menu">
    <!-- Attach files (chat/attachments.rs): their text goes with the question; pictures to Claude Code. -->
    <button class="mrow" data-mi="top" aria-label="Add files or photos" aria-keyshortcuts="Control+U" data-testid="chat-attach-files" onclick={uploadFiles} onpointerenter={() => (sub = null)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21.4 11.1-8.5 8.5a5.5 5.5 0 0 1-7.8-7.8l8.5-8.5a3.7 3.7 0 0 1 5.2 5.2l-8.5 8.5a1.8 1.8 0 0 1-2.6-2.6l7.8-7.8" /></svg>
      <span class="mlabel">Add files or photos</span>
      <kbd class="mside mkey">Ctrl+U</kbd>
    </button>
    <!-- Snip to Chat (snip.rs): this window leaves the shot. -->
    <button class="mrow" data-mi="top" aria-label="Take a screenshot" onclick={takeScreenshot} onpointerenter={() => (sub = null)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="6" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><path d="M20 4 8.1 15.9" /><path d="M14.5 14.5 20 20" /><path d="M8.1 8.1 12 12" /></svg>
      <span class="mlabel">Take a screenshot</span>
      {#if chat.snipHotkey}<kbd class="mside mkey">{formatHotkeySpec(chat.snipHotkey)}</kbd>{/if}
    </button>
    {#if code?.current}
      <!-- Add folder (menu.rs): `--add-dir` on this chat's runs; never a system folder or Yap's own data. -->
      <button
        class="mrow"
        data-mi="top"
        aria-label="Add folder"
        data-testid="code-add-folder"
        title="Let Claude Code use another folder on this PC in this chat too"
        onclick={addFolder}
        onpointerenter={() => (sub = null)}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
        <span class="mlabel">Add folder</span>
        {#if codeMenu.folders.length}<span class="mside mcount">{codeMenu.folders.length} added</span>{/if}
      </button>
      <!-- Slash commands ›: what this chat's Claude Code offers after "/" (the box's "/" list too). -->
      <div class="optsub">
        <button
          class="mrow"
          data-mi="top"
          data-sub="commands"
          aria-haspopup="menu"
          aria-expanded={sub === 'commands'}
          aria-label="Slash commands"
          data-testid="code-plus-commands"
          onclick={(e) => openSub('commands', e.detail === 0)}
          onpointerenter={() => (sub = 'commands')}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="3.5" width="17" height="17" rx="4" /><path d="m14.5 7.5-5 9" /></svg>
          <span class="mlabel">Slash commands</span>
          {@render chev()}
        </button>
        {#if sub === 'commands'}
          <div class="pluspop subpop skillsub searchsub" role="menu" aria-label="Slash commands" data-testid="code-plus-commands-menu">
            <!-- Search commands (17 Nov 2026, as Plugins has): by name and line; ↓ to the list, Enter the first. -->
            <input
              class="msearch"
              type="search"
              data-mi="sub"
              placeholder="Search commands"
              aria-label="Search commands"
              bind:value={commandQuery}
              onkeydown={(e) => {
                if (e.key === 'Enter' && !e.isComposing) {
                  e.preventDefault();
                  const first = commandList.find((g) => g.items.length)?.items[0];
                  if (first) pickCommand(first);
                }
              }}
            />
            <div class="mscroll">
              {#each commandList as g, gi (g.label || gi)}
                {#if g.label}<div class="mhead" role="presentation">{g.label}</div>{/if}
                {#each g.items as c (c.name)}
                  <button class="mrow" role="menuitem" data-mi="sub" title={c.description || c.name} onclick={() => pickCommand(c)}>
                    <span class="mlabel"><span class="mcmd">/{c.name}</span> <span class="mquiet">{c.description}</span></span>
                  </button>
                {/each}
              {:else}
                <div class="mhead mnote" role="status">{commandQuery ? `No command holds "${commandQuery}"` : offeredNote || 'Claude Code offers no commands here.'}</div>
              {/each}
            </div>
          </div>
        {/if}
      </div>
      <!-- Plugins ›: Search plugins; the session's MCP servers and connectors, a switch each for this chat; its
           plugins with their skills; Manage plugins… (Settings → Yap Code → Plugins). Nothing installed from Yap. -->
      <div class="optsub">
        <button
          class="mrow"
          data-mi="top"
          data-sub="plugins"
          aria-haspopup="menu"
          aria-expanded={sub === 'plugins'}
          aria-label="Plugins"
          data-testid="code-plus-plugins"
          onclick={(e) => openSub('plugins', e.detail === 0)}
          onpointerenter={() => (sub = 'plugins')}
        >
          {@render plugIcon()}
          <span class="mlabel">Plugins</span>
          {@render chev()}
        </button>
        {#if sub === 'plugins'}
          <div class="pluspop subpop pluginsub" role="menu" aria-label="Plugins" data-testid="code-plus-plugins-menu">
            <input class="msearch" type="search" data-mi="sub" placeholder="Search plugins" aria-label="Search plugins" bind:value={pluginQuery} oninput={() => (sub2 = null)} />
            <div class="mscroll">
              {#each serverList as s (s.name)}
                <button
                  class="mrow"
                  role="menuitemcheckbox"
                  aria-checked={s.on}
                  data-mi="sub"
                  aria-label={s.label}
                  title={s.on ? `${s.label}: on in this chat. Click to switch its tools off here.` : `${s.label}: off in this chat`}
                  data-testid="code-plus-server"
                  onclick={() => flipServer(s)}
                  onpointerenter={() => (sub2 = null)}
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4" width="17" height="6.5" rx="2" /><rect x="3.5" y="13.5" width="17" height="6.5" rx="2" /><path d="M7 7.25h.01M7 16.75h.01" /></svg>
                  <span class="mlabel">{s.label}{#if SERVER_STATUS[s.status]}<span class="mquiet">{` · ${SERVER_STATUS[s.status]}`}</span>{/if}</span>
                  <span class="mside switch" class:on={s.on} aria-hidden="true"><span></span></span>
                </button>
              {/each}
              {#if serverList.length && pluginList.length}<div class="optsep" role="separator"></div>{/if}
              {#each pluginList as p (p.id || p.name)}
                {#if p.skills.length > 1}
                  <button
                    class="mrow two"
                    role="menuitem"
                    data-mi="sub"
                    data-sub2={p.name}
                    aria-haspopup="menu"
                    aria-expanded={sub2 === p.name}
                    aria-label={`${p.name}, ${p.skills.length} skills`}
                    data-testid="code-plus-plugin"
                    onclick={() => (sub2 === p.name ? (sub2 = null) : openSub2(p.name))}
                    onpointerenter={() => (sub2 = p.name)}
                  >
                    {@render plugIcon()}
                    <span class="mtext"><span class="mlabel">{p.name}</span><span class="mline">{p.skills.length} skills</span></span>
                    {@render chev()}
                  </button>
                {:else}
                  <button
                    class="mrow two"
                    role="menuitem"
                    data-mi="sub"
                    aria-label={p.name}
                    title={p.line || p.name}
                    data-testid="code-plus-plugin"
                    onclick={() => (p.skills[0] ? pickCommand(p.skills[0]) : managePlugins())}
                    onpointerenter={() => (sub2 = null)}
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5z" /><path d="M5 19.5A1.5 1.5 0 0 0 6.5 21H19" /></svg>
                    <span class="mtext"><span class="mlabel">{p.name}</span>{#if p.line}<span class="mline">{p.line}</span>{/if}</span>
                  </button>
                {/if}
              {/each}
              {#if !serverList.length && !pluginList.length}
                <div class="mhead mnote" role="status">{pluginQuery ? `Nothing called "${pluginQuery}"` : offeredNote || 'No MCP servers or plugins here.'}</div>
              {/if}
            </div>
            <div class="optsep" role="separator"></div>
            <button class="mrow" role="menuitem" data-mi="sub" data-testid="code-manage-plugins" onclick={managePlugins} onpointerenter={() => (sub2 = null)}>
              {@render plugIcon()}
              <span class="mlabel">Manage plugins…</span>
            </button>
            {#if sub2}
              {@const open = pluginList.find((p) => p.name === sub2)}
              {#if open}
                <div class="pluspop subpop skillsub sub3" class:flip={sub3Flip} role="menu" aria-label={`${open.name}'s skills`} data-testid="code-plus-plugin-skills">
                  {#each open.skills as c (c.name)}
                    <button class="mrow" role="menuitem" data-mi="sub3" title={c.description || c.name} onclick={() => pickCommand(c)}>
                      <span class="mlabel"><span class="mcmd">/{c.name}</span> <span class="mquiet">{c.description}</span></span>
                    </button>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/if}
      </div>
    {/if}
    <div class="optsep" role="separator"></div>
    {@render voiceRows()}
  </div>
{/snippet}

<style>
  .volval {
    width: 34px;
    text-align: right;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    color: var(--yap-muted);
  }

  /* On the messages' readable column (ChatThread: --yap-thread-width,
     centred in a wide thread), moved by half the thread's scrollbar so the
     two share one centre; the thread's whole width in a narrow one. */
  .composer {
    flex: 0 0 auto;
    position: relative;
    left: calc(-1 * min(var(--thread-gutter, 0px) / 2, max(0px, (100% - var(--yap-thread-width)) / 2)));
    box-sizing: border-box;
    width: 100%;
    max-width: var(--yap-thread-width);
    margin: 10px auto 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .composer.code-room > .inputbar {
    margin-top: 9px;
  }
  .inputbar {
    position: relative;
    display: flex;
    align-items: flex-end;
    gap: 6px;
    min-height: 48px;
    padding: 6px 8px 6px 6px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 14px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .inputbar:focus-within {
    border-color: var(--yap-border-hover);
  }
  .pluswrap {
    position: relative;
    flex: 0 0 auto;
  }
  .plusbtn {
    position: relative;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: 9px;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .plusbtn svg {
    width: 16px;
    height: 16px;
  }
  .plusbtn:hover,
  .plusbtn[aria-expanded='true'] {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .plusbtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .pluspop {
    position: absolute;
    left: 0;
    bottom: calc(100% + 10px);
    z-index: 20;
    width: 280px;
    padding: 6px;
    border: 1px solid var(--yap-border);
    border-radius: 12px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
  }
  .switch {
    flex: 0 0 auto;
    width: 28px;
    height: 16px;
    padding: 2px;
    border-radius: 999px;
    background: var(--yap-raised);
    transition: background var(--yap-dur);
  }
  .switch span {
    display: block;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    transition: transform var(--yap-dur);
  }
  .switch.on {
    background: var(--yap-primary);
  }
  .switch.on span {
    transform: translateX(12px);
  }
  /* A submenu: its flyout beside the menu, as Claude's composer opens it. */
  .optsub {
    position: relative;
  }
  .subpop {
    left: calc(100% + 12px);
    bottom: -6px;
    width: 240px;
    max-height: 320px;
    overflow-y: auto;
  }
  .optsep {
    height: 1px;
    margin: 2px 6px;
    background: var(--yap-border-subtle);
  }
  /* **A Code chat's Plugins ›** (17 Nov 2026, the Claude Code app's): the search box on top, the servers and
     plugins scrolling under it, Manage plugins… at the foot, a plugin's skills beside it (`.sub3`, outside the
     scroller so it isn't cut). A row with a second line: a plugin's "12 skills" or its one line. */
  .plainmenu .subpop.pluginsub {
    width: 272px;
  }
  /* (Slash commands › too, since 17 Nov 2026: a search box over a list that scrolls.) */
  .plainmenu .subpop.pluginsub,
  .plainmenu .subpop.searchsub {
    max-height: none;
    overflow: visible;
  }
  .msearch {
    display: block;
    box-sizing: border-box;
    width: 100%;
    height: 30px;
    margin: 0 0 4px;
    padding: 0 8px;
    border: none;
    border-bottom: 1px solid var(--yap-border-subtle);
    border-radius: 8px 8px 0 0;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  .msearch::placeholder {
    color: var(--yap-muted-55);
  }
  .msearch:focus {
    outline: none;
    border-bottom-color: var(--yap-primary-line);
  }
  .mscroll {
    max-height: 300px;
    overflow-y: auto;
  }
  .mrow.two {
    min-height: 40px;
  }
  .mtext {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .mtext .mlabel {
    flex: none;
  }
  .mline {
    overflow: hidden;
    color: var(--yap-muted);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .plainmenu .subpop.sub3 {
    top: -1px;
    bottom: auto;
    left: calc(100% + 8px);
  }
  .plainmenu .subpop.sub3.flip {
    left: auto;
    right: calc(100% + 8px);
  }
  .mnote {
    font-weight: 500;
    line-height: 1.4;
  }
  .composerfoot {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 0 4px;
  }
  .footgap {
    flex: 1 1 auto;
  }
  .plusdot {
    position: absolute;
    top: 5px;
    right: 5px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--yap-primary);
    box-shadow: 0 0 0 2px var(--yap-s2);
  }
  .micbtn {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 9px;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
    flex: 0 0 auto;
  }
  .micbtn:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .micbtn:focus-visible,
  .sendbtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .micbtn svg {
    width: 15px;
    height: 15px;
  }
  /* Several lines (a textarea): one line high to start, growing with its words up to its cap (fitBox), then
     scrolling; the buttons beside it at its foot. */
  .msginput {
    flex: 1 1 auto;
    min-width: 0;
    height: 32px;
    min-height: 32px;
    box-sizing: border-box;
    padding: 6px 4px;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
    line-height: 20px;
    resize: none;
    overflow-y: hidden;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    scrollbar-width: thin;
  }
  /* (The buttons, 30 px, a pixel up from the foot: centred on one line, as before.) */
  .inputbar > :global(:not(.msginput):not(textarea)) {
    margin-bottom: 1px;
  }
  /* **A plain chat's box** (16 Nov 2026, the Claude app's proportions): the words on top, room for a second line,
     then a row with + on the left and Think, the mic and Send on the right. */
  .composer.plain > .inputbar {
    flex-wrap: wrap;
    row-gap: 2px;
    padding: 8px 8px 7px;
    border-radius: 16px;
  }
  .composer.plain .msginput {
    order: -1;
    flex: 1 0 100%;
    min-height: 46px;
    padding: 5px 6px;
  }
  .composer.plain .rowgap {
    flex: 1 1 0;
    min-width: 0;
  }
  /* What's on for this chat, beside the +: "Research ×", "Concise ×" (the Claude app's mode chips in its box). */
  .modechips {
    align-self: center;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin-left: 2px;
  }
  .modechip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    height: 26px;
    padding: 0 3px 0 8px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }
  .modechip > svg {
    flex: none;
    width: 13px;
    height: 13px;
    color: var(--yap-muted);
  }
  .modechip.on {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary-wash);
  }
  .modechip.on > svg {
    color: var(--yap-primary);
  }
  .modechip .mname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mcx {
    flex: none;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .mcx:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .mcx:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 0;
  }
  .mcx svg {
    width: 9px;
    height: 9px;
  }

  /* **A plain chat's + menu** (the Claude desktop app's): one line a row, an icon each, the shortcut, › or ✓ at the
     right; dividers between the parts. Upwards from the + in a thread, downwards on the start screen. */
  .plainmenu {
    width: 252px;
    padding: 5px;
    border-radius: 12px;
  }
  .plainmenu:focus {
    outline: none;
  }
  .plainmenu.down {
    top: calc(100% + 8px);
    bottom: auto;
  }
  .plainmenu .subpop {
    width: 232px;
    padding: 5px;
  }
  .plainmenu.down .subpop {
    top: -5px;
    bottom: auto;
  }
  .skillsub {
    width: 280px !important;
    max-height: 360px;
  }
  .mrow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 32px;
    padding: 5px 8px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .mrow:hover,
  .mrow[aria-expanded='true'] {
    background: var(--yap-s3);
  }
  .mrow:focus-visible {
    outline: none;
    background: var(--yap-s3);
    box-shadow: inset 0 0 0 1.5px var(--yap-primary-line);
  }
  .mrow > svg:first-child {
    flex: none;
    width: 16px;
    height: 16px;
    color: var(--yap-fg-80);
  }
  .mrow.unavail {
    color: var(--yap-muted);
  }
  .mrow.unavail > svg:first-child {
    opacity: 0.6;
  }
  .mlabel {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mside {
    flex: none;
  }
  .mkey {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
  }
  .mcount {
    max-width: 90px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-muted);
    font-size: 11.5px;
  }
  .mchev {
    width: 14px;
    height: 14px;
    margin-right: -2px;
    color: var(--yap-muted);
  }
  .mtick {
    width: 16px;
    height: 16px;
    color: var(--yap-primary);
  }
  .mhead {
    padding: 6px 8px 3px;
    color: var(--yap-muted);
    font-size: 11px;
    font-weight: 600;
  }
  .mcmd {
    font-weight: 600;
  }
  .mquiet {
    color: var(--yap-muted);
  }
  .mvol {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 8px 6px 34px;
  }
  .mvol input {
    flex: 1 1 auto;
    min-width: 0;
    accent-color: var(--yap-primary);
  }
  .msginput:focus {
    outline: none;
  }
  .msginput::placeholder {
    color: var(--yap-muted-55);
  }
  .sendbtn {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    cursor: pointer;
    flex: 0 0 auto;
  }
  .sendbtn:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .sendbtn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .sendbtn svg {
    width: 15px;
    height: 15px;
  }

  /* ---- Snip to Chat: screenshots waiting in the box ---- */
  .snipbar {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .shotrow {
    display: flex;
    align-items: flex-end;
    flex-wrap: wrap;
    gap: 8px;
    /* Room for a tile's ✕, which sits out over its corner. */
    padding: 8px 8px 0 0;
  }
  /* The passage a question is about (the document panel's selection). */
  .quotebar {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-bottom: 6px;
    padding: 7px 6px 7px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-left: 3px solid var(--yap-primary-line);
    border-radius: 10px;
    background: var(--yap-s1);
    color: var(--yap-fg-80);
    font-size: 12.5px;
    line-height: 1.45;
  }
  .quoteicon {
    flex: 0 0 auto;
    width: 13px;
    height: 13px;
    margin-top: 2px;
    color: var(--yap-primary);
  }
  .quotetext {
    flex: 1 1 auto;
    min-width: 0;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .quotefrom {
    font-weight: 600;
    color: var(--yap-fg);
  }
  .quotex {
    flex: 0 0 auto;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .quotex:hover {
    background: var(--yap-s3, var(--yap-s2));
    color: var(--yap-fg);
  }
  .quotex:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .quotex svg {
    width: 11px;
    height: 11px;
  }
  /* A waiting screenshot's tile (the frame itself: app.css .shotframe). */
  .elrow {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 12px 0;
  }
  .shotchip {
    flex: 0 0 auto;
  }
  /* A mentioned workspace file (@path): its name, its folder quiet, ✕. */
  .mentionchip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    min-width: 0;
    padding: 3px 4px 3px 9px;
    border: 1px solid var(--yap-border);
    border-radius: 9px;
    background: var(--yap-s1);
    font-size: 12.5px;
  }
  .mname {
    font-weight: 600;
    white-space: nowrap;
  }
  .mdir {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-muted);
    font-size: 11.5px;
  }
  .mx {
    width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .mx:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .mx svg {
    width: 9px;
    height: 9px;
  }
  /* The files attached: room for each chip's ✕ over its corner. */
  .filerow {
    gap: 10px;
    padding: 10px 10px 2px 2px;
  }
  .thumb {
    position: absolute;
    inset: 0;
    display: block;
    padding: 0;
    border: none;
    border-radius: inherit;
    overflow: hidden;
    background: none;
    cursor: zoom-in;
  }
  .thumb:focus-visible {
    outline: none;
  }
  .shotchip:has(.thumb:focus-visible) {
    box-shadow: var(--yap-focus);
  }
  /* Its size, quiet, on hover (as the Claude app's badge sits). */
  .shotsize {
    position: absolute;
    left: 6px;
    bottom: 6px;
    z-index: 2;
    padding: 1px 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    box-shadow: inset 0 0 0 var(--yap-hairline) var(--yap-shot-ring);
    color: var(--yap-muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transition: opacity var(--yap-dur);
  }
  .copiedline {
    margin-top: 6px;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  /* /compact's quiet line over the box: three soft dots while it runs. */
  .cmdline {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .cmdline.done {
    color: var(--yap-fg-80);
  }
  .cmddots {
    display: inline-flex;
    gap: 3px;
  }
  .cmddots span {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: currentColor;
    animation: cmddot 1.2s ease-in-out infinite;
  }
  .cmddots span:nth-child(2) {
    animation-delay: 0.2s;
  }
  .cmddots span:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes cmddot {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .cmddots span {
      animation: none;
      opacity: 0.6;
    }
  }
  /* ✕: a small round button out over the tile's corner, on hover or
     keyboard focus (always on a touch screen), as the Claude app's. */
  .shotx {
    position: absolute;
    top: -7px;
    right: -7px;
    z-index: 3;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: var(--yap-s2);
    box-shadow: inset 0 0 0 var(--yap-hairline) var(--yap-shot-ring), var(--yap-shadow-sm);
    color: var(--yap-muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--yap-dur);
  }
  /* A bigger target than it looks. */
  .shotx::before {
    content: '';
    position: absolute;
    inset: -3px;
    border-radius: 50%;
  }
  .shotx:hover {
    color: var(--yap-fg);
  }
  .shotx:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .shotx svg {
    width: 10px;
    height: 10px;
  }
  .shotchip:hover .shotx,
  .shotchip:focus-within .shotx,
  .shotchip:hover .shotsize,
  .shotchip:focus-within .shotsize {
    opacity: 1;
  }
  @media (hover: none), (pointer: coarse) {
    .shotx {
      opacity: 1;
    }
  }
  .visionline {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    padding: 7px 10px;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r);
    background: var(--yap-primary-wash);
    font-size: 12px;
    color: var(--yap-fg);
  }
  .vtext {
    flex: 1 1 220px;
  }
  /* The screenshot reader's download: the line's whole width. */
  .vprogress {
    flex: 1 1 100%;
  }
  .vbtn {
    padding: 4px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .vbtn:hover {
    border-color: var(--yap-border-hover, var(--yap-border));
  }
  .vbtn.primary {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .vbtn.primary:hover {
    background: var(--yap-ink-hover, var(--yap-ink));
  }
  .vbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* Files dragged over Chat: the box says a drop lands here. */
  .composer.dropping .inputbar {
    border-color: var(--yap-primary-line);
    border-style: dashed;
    background: var(--yap-primary-wash);
  }
  .dropnote {
    padding: 6px 10px;
    border: 1px dashed var(--yap-primary-line);
    border-radius: var(--yap-r);
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
    font-size: 12px;
    font-weight: 600;
    text-align: center;
  }
  .vlink {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link, var(--yap-muted));
    font-size: 11.5px;
    text-decoration: underline;
    cursor: pointer;
  }
</style>
