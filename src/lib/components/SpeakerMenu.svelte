<script>
  // The menu on a speaker's name in a meeting transcript (the notepad and
  // the Notes view): rename the voice everywhere, or say who said just this
  // line (or this run of lines); merge two voices into one; and, on a guess
  // ("Priya?", from the calendar invite), "It's Priya" or "Not Priya". Each
  // change goes to Rust (speakers.rs: the user's choice is locked against
  // every automatic relabel), which tells every window (`yap-note-changed`
  // with `lineSpeakers`); `onchanged` gets the note back for this one.
  //
  // Sits in a positioned wrapper next to the button that opened it; a press
  // outside that wrapper, or Escape, closes it.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick, untrack } from 'svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { YOU, speakerInfo, noteVoices } from '../core/notepadText.js';

  let {
    note,
    /** The voice (resolved id) the menu is about. */
    voice,
    /** Transcript indices "just this line" means: one line, or a run. */
    lines = [],
    /** The window's label: the `origin` of the change. */
    origin = '',
    /** Open straight at "Rename just this line". */
    start = 'menu',
    align = 'left',
    onclose = () => {},
    onchanged = () => {},
  } = $props();

  let root = $state(null);
  let input = $state(null);
  let mode = $state(untrack(() => start)); // 'menu' | 'all' | 'lines' | 'merge'
  let name = $state('');
  let busy = $state(false);

  const info = $derived(speakerInfo(note?.speakers, voice));
  const isYou = $derived(voice === YOU);
  // A name Yap suggests but didn't put on (speakers/naming.rs: the card's).
  const suggested = $derived.by(() => {
    const s = note?.speakers?.[voice]?.suggestion;
    return s && !s.me && s.name && !(info.named && info.name.toLowerCase() === s.name.toLowerCase()) ? s.name : '';
  });
  // "It was me" for a voice the separation told apart (or a person named on
  // some lines), never for "Them" (everyone on the call).
  const canBeMe = $derived(!isYou && voice !== 'them');
  const lineCount = $derived(lines.length);
  const linesLabel = $derived(lineCount === 1 ? 'just this line' : `just these ${lineCount} lines`);
  // Other voices this one could be (never You).
  const targets = $derived(
    noteVoices(note)
      .filter((id) => id !== voice && id !== YOU)
      .map((id) => speakerInfo(note?.speakers, id))
  );
  // Names to pick from: the attendees and the voices' names.
  const picks = $derived.by(() => {
    const out = [];
    const add = (n) => {
      const t = (n || '').trim();
      if (t && !out.some((o) => o.toLowerCase() === t.toLowerCase()) && t.toLowerCase() !== info.name.toLowerCase()) {
        out.push(t);
      }
    };
    if (mode === 'lines' && !isYou) add('You');
    for (const p of note?.participants || []) add(p);
    for (const id of noteVoices(note)) {
      const s = speakerInfo(note?.speakers, id);
      if (s.named) add(s.name);
    }
    return out.slice(0, 6);
  });

  async function begin(next) {
    mode = next;
    name = next === 'all' && info.named ? info.name : '';
    await tick();
    input?.focus();
    input?.select();
  }

  async function run(command, args) {
    if (busy) return;
    busy = true;
    try {
      const updated = await invoke(command, { noteId: note.id, origin, ...args });
      onchanged(updated);
      onclose();
    } catch (e) {
      toast({ title: "Couldn't change the speaker", description: String(e), variant: 'destructive' });
    } finally {
      busy = false;
    }
  }

  function save(picked) {
    const n = (picked ?? name).trim();
    if (!n) return;
    if (mode === 'lines') run('speaker_rename_lines', { lines, name: n });
    else run('speaker_rename', { speaker: voice, name: n });
  }

  function onWindowPointerdown(e) {
    if (root && !root.parentElement?.contains(e.target)) onclose();
  }
  function onWindowKeydown(e) {
    if (e.key !== 'Escape') return;
    // Ours: the windows' own Escape (the Ask sheet, search) stays put.
    e.stopPropagation();
    if (mode !== 'menu' && start === 'menu') mode = 'menu';
    else onclose();
  }

  onMount(() => {
    if (start !== 'menu') begin(start);
    // In a scrolling transcript, the whole menu in view.
    tick().then(() => root?.scrollIntoView?.({ block: 'nearest' }));
  });
</script>

<svelte:window onpointerdowncapture={onWindowPointerdown} onkeydowncapture={onWindowKeydown} />

<div
  class="spkmenu"
  class:right={align === 'right'}
  bind:this={root}
  role={mode === 'menu' || mode === 'merge' ? 'menu' : 'dialog'}
  aria-label={mode === 'menu' ? `Speaker: ${info.label}` : mode === 'merge' ? `Merge ${info.label} with` : 'Rename speaker'}
>
  {#if mode === 'menu'}
    <p class="head">
      {info.label}
      {#if info.guess}
        <span class="guess">{note?.speakers?.[voice]?.source === 'calendar' ? 'A guess from the calendar invite' : "Yap's guess"}</span>
      {/if}
    </p>
    {#if info.guess}
      <button role="menuitem" class="item" disabled={busy} onclick={() => run('speaker_rename', { speaker: voice, name: info.name })}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
        <span>It's {info.name}</span>
      </button>
      <button role="menuitem" class="item" disabled={busy} onclick={() => run('speaker_not', { speaker: voice })}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7l10 10M17 7L7 17" /></svg>
        <span>Not {info.name}</span>
      </button>
    {/if}
    {#if suggested}
      <button role="menuitem" class="item" disabled={busy} onclick={() => run('speaker_suggestion', { speaker: voice, answer: 'yes' })}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
        <span>It's {suggested}</span>
      </button>
    {/if}
    {#if !isYou}
      <button role="menuitem" class="item" onclick={() => begin('all')}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 19.5l1-4L15.8 5.2a2 2 0 0 1 2.9 0l.1.1a2 2 0 0 1 0 2.9L8.5 18.5z" /></svg>
        <span>Rename {info.name} everywhere…</span>
      </button>
    {/if}
    {#if lineCount}
      <button role="menuitem" class="item" onclick={() => begin('lines')}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="8" r="3.5" /><path d="M5 19.5a7 7 0 0 1 14 0" /></svg>
        <span>Rename {linesLabel}…</span>
      </button>
    {/if}
    {#if !isYou && targets.length}
      <button role="menuitem" class="item" onclick={() => (mode = 'merge')}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 4.5v4a5 5 0 0 0 5 5h7" /><path d="M6 19.5v-4a5 5 0 0 1 5-5" /><path d="M15 10.5l3 3-3 3" /></svg>
        <span>Merge with…</span>
      </button>
    {/if}
    {#if canBeMe}
      <button
        role="menuitem"
        class="item"
        disabled={busy}
        title="Your own voice reached the call audio: these lines become yours, and repeats of your lines are hidden"
        onclick={() => run('speaker_is_me', { speaker: voice })}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="8" r="3.5" /><path d="M5 19.5a7 7 0 0 1 14 0" /><path d="M16.5 4.5l1.5 1.5 3-3" /></svg>
        <span>It was me</span>
      </button>
    {/if}
  {:else if mode === 'merge'}
    <p class="head">{info.label} is the same person as…</p>
    {#each targets as t (t.id)}
      <button role="menuitem" class="item" disabled={busy} onclick={() => run('speaker_merge', { from: voice, into: t.id })}>
        <span class="dot" aria-hidden="true"></span><span>{t.label}</span>
      </button>
    {/each}
  {:else}
    <form
      class="rename"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <label class="head" for="spkname">
        {mode === 'lines' ? `Who said ${lineCount === 1 ? 'this line' : 'these lines'}?` : `Rename ${info.name} everywhere`}
      </label>
      <input
        id="spkname"
        bind:this={input}
        bind:value={name}
        placeholder="Name"
        maxlength="60"
        autocomplete="off"
        spellcheck="false"
        aria-label="Speaker name"
      />
      {#if picks.length}
        <div class="picks">
          {#each picks as p (p)}
            <button type="button" class="pick" disabled={busy} onclick={() => save(p)}>{p}</button>
          {/each}
        </div>
      {/if}
      <p class="note">
        {mode === 'lines'
          ? 'Only these words change speaker. Yap never moves them again.'
          : 'Every line of this voice, now and later. Yap never renames it again.'}
      </p>
      <div class="actions">
        <button type="button" class="ghost" onclick={() => (start === 'menu' ? (mode = 'menu') : onclose())}>Cancel</button>
        <button type="submit" class="primary" disabled={busy || !name.trim()}>Save</button>
      </div>
    </form>
  {/if}
</div>

<style>
  .spkmenu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 60;
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 264px;
    box-sizing: border-box;
    padding: 6px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-popover);
    font-family: inherit;
    font-size: 14px;
    font-weight: 400;
    letter-spacing: normal;
    text-transform: none;
    text-align: left;
    animation: spkin 0.08s ease-out;
    cursor: default;
  }
  .spkmenu.right {
    left: auto;
    right: 0;
  }
  @keyframes spkin {
    from {
      opacity: 0;
      translate: 0 -4px;
    }
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 4px 10px 6px;
    font-size: 13px;
    font-weight: 600;
    line-height: 18px;
  }
  .guess {
    font-size: 12px;
    font-weight: 400;
    color: var(--yap-paper-date);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 34px;
    box-sizing: border-box;
    padding: 7px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    line-height: 19px;
    text-align: left;
    cursor: pointer;
  }
  .item span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item svg {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    fill: none;
    stroke: var(--yap-paper-text-2);
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .item:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .item:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--them, var(--yap-paper-text-2));
  }
  .rename {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 2px 4px 4px;
  }
  .rename .head {
    margin: 4px 2px 0;
  }
  .rename input {
    height: 34px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-s2-strong, var(--yap-paper-s2));
    border-radius: 8px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    outline: none;
  }
  .rename input:focus {
    border-color: var(--yap-paper-ring, var(--yap-primary));
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--yap-paper-ring, var(--yap-primary)) 22%, transparent);
  }
  .picks {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .pick {
    max-width: 100%;
    height: 26px;
    padding: 0 9px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 13px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pick:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .note {
    margin: 0 2px;
    font-size: 12px;
    line-height: 16px;
    color: var(--yap-paper-date);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
  .ghost,
  .primary {
    height: 30px;
    padding: 0 12px;
    border-radius: 8px;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .ghost {
    border: 1px solid var(--yap-paper-s2);
    background: none;
    color: var(--yap-paper-ink);
  }
  .primary {
    border: none;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
  }
  .primary:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
