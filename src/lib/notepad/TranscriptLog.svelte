<script>
  // The transcript's lines: speaker groups of chat bubbles (Wispr's grouped
  // corners) with a hover copy, named from the note's speaker map — the menu
  // on a name, or on a line's person button, renames a voice everywhere or
  // just that line, merges two voices, and undoes a guess ("Priya?" → Not
  // Priya); ▶ plays a line's stretch of the meeting (meetings/meeting_audio.rs); a
  // paused divider where a recording stopped (`Note::breaks`); a marker where
  // the person dictated mid-meeting (a `dictated` segment); a stretch that
  // failed to transcribe, with Try again. A line's words are fixed in place:
  // double-click it, or Edit line in its menu.
  import { tick } from 'svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { openContextMenu, menuKey, menuOpenFor } from '../ui/contextMenu.svelte.js';
  import { bubbleShape, highlightParts, withLineEdit } from '../core/notepadText.js';
  import { editLine } from '../core/noteEdits.js';
  import { meetingAudio, canPlay, playLine } from '../stores/meetingAudio.svelte.js';
  import SpeakerMenu from '../components/SpeakerMenu.svelte';
  import FailedChunk from '../components/FailedChunk.svelte';
  import { LABEL, copyToClipboard } from './actions.js';
  import Icon from './NotepadIcon.svelte';

  /**
   * `items`: the transcript laid out (notepadText.js `transcriptItems`);
   * `view`: the TranscriptView; `speakersNote`: why the speaker engine
   * stopped early, if it did; `changing(field)`, `onreload()` and
   * `onspeakerchanged(note)`: the notepad's own (Notepad.svelte).
   */
  let {
    note = $bindable(),
    items,
    view,
    recordingThis,
    audio,
    speakersNote,
    changing,
    onreload,
    onspeakerchanged,
  } = $props();

  const echoCount = $derived((note?.transcript || []).filter((s) => s.echo).length);
  const lineCount = $derived((note?.transcript || []).length);

  // Follow the newest line unless the person scrolled up to read.
  let logEl = $state(null);
  $effect(() => {
    void items.length;
    void lineCount;
    // (Not while a speaker's menu is open over a line.)
    if (logEl && view.followLog && !view.searchQuery && !view.speakerMenu) logEl.scrollTop = logEl.scrollHeight;
  });

  // Copy one line (from its bubble).
  async function copyLine(line) {
    if (await copyToClipboard(line.text)) {
      view.copiedLine = line.i;
      setTimeout(() => view.copiedLine === line.i && (view.copiedLine = null), 1200);
    }
  }
  // Fixing a line's words (notes/note_edits.rs): double-click it, or Edit line in
  // its menu. Enter saves, Escape puts it back, leaving it saves. It keeps
  // its time, voice and audio (▶ plays what was said), shows "edited", and
  // a misheard word it fixed is offered for the dictionary.
  let lineEditEl = $state(null);
  let lineSaving = false;
  async function startEditLine(line) {
    if (!note || view.searchQuery) return;
    view.speakerMenu = null;
    view.editingLine = { i: line.i, text: note.transcript?.[line.i]?.text ?? line.text };
    await tick();
    lineEditEl?.focus();
    lineEditEl?.select();
    fitLineEdit();
  }
  function fitLineEdit() {
    if (!lineEditEl) return;
    lineEditEl.style.height = 'auto';
    lineEditEl.style.height = `${lineEditEl.scrollHeight}px`;
  }
  async function finishEditLine(save) {
    const edit = view.editingLine;
    if (!edit || lineSaving) return;
    view.editingLine = null;
    const before = note?.transcript?.[edit.i];
    if (!save || !note || !before || edit.text.trim() === before.text) {
      refocusLine(edit.i);
      return;
    }
    lineSaving = true;
    const id = note.id;
    try {
      const r = await editLine(id, note.transcript, edit.i, edit.text, LABEL);
      if (note?.id === id && r?.changed) {
        changing('transcript');
        const t = withLineEdit(note.transcript, { index: r.index, line: r.line });
        if (t) note.transcript = t;
        else onreload();
      }
    } catch (e) {
      toast({ title: "Couldn't change the line", description: String(e), variant: 'destructive' });
    } finally {
      lineSaving = false;
      refocusLine(edit.i);
    }
  }
  function refocusLine(i) {
    setTimeout(() => lineEls[i]?.focus({ preventScroll: true }), 0);
  }
  function onLineEditKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      finishEditLine(true);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      finishEditLine(false);
    }
  }

  // A line's right-click menu (ui/contextMenu.svelte.js, as everywhere): what
  // its hover buttons do — Copy line, and Who said this? (the speaker menu
  // at the line, SpeakerMenu.svelte) — not for a line from the speakers —
  // and Edit line.
  let lineEls = $state({});
  function lineMenu(e, item, line) {
    openContextMenu(
      e,
      [
        { label: 'Edit line', icon: 'pencil', onselect: () => startEditLine(line) },
        { label: 'Copy line', icon: 'copy', onselect: () => copyLine(line) },
        canPlay(note?.id, note?.transcript?.[line.i])
          ? {
              label: meetingAudio.playing === `${note.id}:${line.i}` ? 'Stop playing' : 'Play this line',
              icon: meetingAudio.playing === `${note.id}:${line.i}` ? 'stop' : 'play',
              onselect: () => playLine(note.id, line.i, note.transcript[line.i]),
            }
          : null,
        !item.echo
          ? {
              label: 'Who said this?',
              icon: 'person',
              onselect: () => (view.speakerMenu = { key: `l${line.i}`, voice: item.speaker, lines: [line.i], start: 'lines' }),
            }
          : null,
      ],
      { label: 'Transcript line', row: lineEls[line.i] }
    );
  }
</script>

<div
  class="lines"
  role="log"
  aria-label="Transcript"
  bind:this={logEl}
  onscroll={(e) => {
    // The element scrolled, not `logEl`: a scroll queued as the
    // log is swapped out arrives after bind:this let go (null).
    const el = e.currentTarget;
    view.followLog = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  }}
>
  {#each items as item (item.key)}
    {#if item.kind === 'group'}
      <div class="group {item.source === 'you' ? 'you' : 'them'}" class:echo={item.echo}>
        {#if item.echo}
          <span class="who">{item.label} · from the speakers</span>
        {:else}
          <span class="spkwrap">
            <button
              class="who"
              class:guess={item.guess}
              aria-haspopup="menu"
              aria-expanded={view.speakerMenu?.key === item.key}
              title={item.guess ? 'A guess: click to confirm or change' : 'Rename, merge or correct this speaker'}
              onclick={() => view.toggleSpeakerMenu(item.key, item.speaker, item.lines.map((l) => l.i))}
            >{item.label}</button>
            {#if view.speakerMenu?.key === item.key}
              <SpeakerMenu
                {note}
                voice={view.speakerMenu.voice}
                lines={view.speakerMenu.lines}
                origin={LABEL}
                onclose={() => (view.speakerMenu = null)}
                onchanged={onspeakerchanged}
              />
            {/if}
          </span>
        {/if}
        <div class="bubbles">
          {#each item.lines as line, j (line.i)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="row"
              class:playable={canPlay(note.id, note.transcript?.[line.i])}
              class:menuopen={view.speakerMenu?.key === `l${line.i}` || menuOpenFor(lineEls[line.i])}
              oncontextmenu={(e) => lineMenu(e, item, line)}
              onkeydown={(e) => menuKey(e, (k) => lineMenu(k, item, line))}
            >
              <!-- Double-click to fix its words (Edit line in its menu from the keyboard). -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="bubble {bubbleShape(j, item.lines.length)}"
                class:editing={view.editingLine?.i === line.i}
                ondblclick={() => view.editingLine?.i !== line.i && startEditLine(line)}
              >
                {#if view.editingLine?.i === line.i}
                  <textarea
                    class="lineedit"
                    rows="1"
                    aria-label="Edit line: Enter saves, Escape cancels"
                    bind:this={lineEditEl}
                    bind:value={view.editingLine.text}
                    oninput={fitLineEdit}
                    onkeydown={onLineEditKeydown}
                    onblur={() => finishEditLine(true)}
                  ></textarea>
                {:else}
                  {#each highlightParts(line.text, view.searchQuery) as part, k (k)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}{#if line.edited}<span
                      class="lineedited"
                      title={`You edited this line. Yap heard: “${line.original}”`}
                    >edited</span>{/if}
                {/if}
              </div>
              {#if !item.echo}
                <button
                  class="linewho"
                  aria-label="Who said this?"
                  onclick={() => view.toggleSpeakerMenu(`l${line.i}`, item.speaker, [line.i], 'lines')}
                >
                  <Icon name="person" />
                </button>
              {/if}
              {#if canPlay(note.id, note.transcript?.[line.i])}
                <!-- ▶ this line's stretch of the meeting, both sides (meetings/meeting_audio.rs). -->
                <button
                  class="lineplay"
                  class:on={meetingAudio.playing === `${note.id}:${line.i}`}
                  aria-label={meetingAudio.playing === `${note.id}:${line.i}` ? 'Stop playing' : 'Play this line'}
                  onclick={() => playLine(note.id, line.i, note.transcript[line.i])}
                >
                  <Icon name={meetingAudio.playing === `${note.id}:${line.i}` ? 'stop' : 'play'} />
                </button>
              {/if}
              <button class="linecopy" aria-label="Copy line" bind:this={lineEls[line.i]} onclick={() => copyLine(line)}>
                <Icon name={view.copiedLine === line.i ? 'check' : 'copy'} />
              </button>
              {#if view.speakerMenu?.key === `l${line.i}`}
                <SpeakerMenu
                  {note}
                  voice={view.speakerMenu.voice}
                  lines={view.speakerMenu.lines}
                  start="lines"
                  align="right"
                  origin={LABEL}
                  onclose={() => (view.speakerMenu = null)}
                  onchanged={onspeakerchanged}
                />
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {:else if item.kind === 'pause'}
      <div class="divider" role="separator" aria-label="Recording paused here">
        <span class="rule"></span><Icon name="pause" /><span class="rule"></span>
      </div>
    {:else if item.kind === 'mic'}
      <div class="divider dictated" role="separator" aria-label="Your mic was disconnected">
        <span class="rule"></span><Icon name="mic" /><span class="dtext"
          >Your mic was disconnected{item.next ? ` · now ${item.next}` : ' · none connected'}</span
        ><span class="rule"></span>
      </div>
    {:else if item.kind === 'failed'}
      <FailedChunk
        noteId={note.id}
        seg={note.transcript[item.i]}
        look="paper"
        why={recordingThis ? 'After the meeting' : audio && !audio.recordings ? "This meeting's audio isn't kept" : null}
      />
    {:else}
      <div
        class="divider dictated"
        role="separator"
        aria-label="You dictated here"
        title="Yap kept what you dictated out of the meeting's transcript and summaries"
      >
        <span class="rule"></span><Icon name="mic" /><span class="dtext">You dictated here · left out of the notes</span><span class="rule"></span>
      </div>
    {/if}
  {/each}
  {#if echoCount && !view.searchQuery}
    <button class="echonote" onclick={() => (view.showEcho = !view.showEcho)}>
      {view.showEcho ? 'Hide' : 'Show'} {echoCount} {echoCount === 1 ? 'line' : 'lines'} your mic picked up from the speakers
    </button>
  {/if}
  {#if speakersNote && speakersNote.noteId === note.id && !view.searchQuery}
    <p class="spknote" role="note" aria-live="polite">{speakersNote.message}</p>
  {/if}
</div>

<style>
  button {
    font: inherit;
    cursor: pointer;
  }

  .lines {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    display: flex;
    flex-direction: column;
    gap: 16px;
    margin-top: 14px;
    padding: 12px 20px 164px 36px;
  }
  /* Thin scrollbars, as on the page they sit on (8px, a soft thumb). */
  .lines::-webkit-scrollbar {
    width: 8px;
  }
  .lines::-webkit-scrollbar-thumb {
    border-radius: 4px;
    background: color-mix(in srgb, var(--yap-paper-ink) 16%, transparent);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bubbles {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .who {
    font-size: 15px;
    font-weight: 550;
    line-height: 18px;
  }
  .spkwrap {
    position: relative;
    align-self: flex-start;
  }
  button.who {
    padding: 0 4px;
    margin: 0 -4px;
    border: none;
    border-radius: 5px;
    background: none;
    color: inherit;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }
  button.who:hover,
  button.who[aria-expanded='true'] {
    background: var(--yap-paper-s2);
  }
  /* A guess ("Priya?"): the name, a little quieter. */
  button.who.guess {
    font-style: italic;
  }
  .group.them .who {
    color: var(--them);
  }
  .row {
    position: relative;
    padding-right: 50px;
  }
  .bubble {
    width: fit-content;
    max-width: 100%;
    box-sizing: border-box;
    padding: 8px 12px;
    background: var(--yap-paper-s1);
    font-family: var(--yap-font-reading);
    font-size: calc(15px * var(--yap-reading-scale));
    font-weight: 500;
    line-height: calc(1.5 * var(--yap-reading-leading));
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .bubble.first {
    border-radius: 12px 12px 12px 4px;
  }
  /* A line being fixed: its words in a box the bubble's size and type. */
  .bubble.editing {
    width: 100%;
    background: var(--yap-paper-card);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .lineedit {
    display: block;
    width: 100%;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    line-height: inherit;
    resize: none;
    overflow: hidden;
  }
  .lineedit:focus {
    outline: none;
  }
  /* "edited": the person fixed this line (its title says what Yap heard). */
  .lineedited {
    margin-left: 6px;
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-paper-faint);
    cursor: help;
  }
  .bubble.mid {
    border-radius: 4px 12px 12px 4px;
  }
  .bubble.last {
    border-radius: 4px 12px 12px 12px;
  }
  .group.echo .bubble {
    opacity: 0.55;
    font-style: italic;
  }
  .bubble mark {
    border-radius: 3px;
    background: var(--yap-primary-tint); /* Yap amber, not a yellow marker */
    color: inherit;
  }
  .linecopy {
    position: absolute;
    top: 50%;
    right: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-text-2);
    opacity: 0;
    transform: translateY(-50%);
    transition: opacity 0.12s;
  }
  .linecopy :global(svg),
  .linewho :global(svg) {
    width: 14px;
    height: 14px;
  }
  .linewho {
    position: absolute;
    top: 50%;
    right: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-text-2);
    opacity: 0;
    transform: translateY(-50%);
    transition: opacity 0.12s;
  }
  .row:hover .linewho,
  .row.menuopen .linewho,
  .linewho:focus-visible {
    opacity: 1;
  }
  .linewho:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .row:hover .linecopy,
  .linecopy:focus-visible {
    opacity: 1;
  }
  /* ▶ a line (meetings/meeting_audio.rs), left of the person and copy buttons. */
  .row.playable {
    padding-right: 76px;
  }
  .lineplay {
    position: absolute;
    top: 50%;
    right: 52px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-text-2);
    opacity: 0;
    transform: translateY(-50%);
    transition: opacity 0.12s;
  }
  .lineplay :global(svg) {
    width: 12px;
    height: 12px;
  }
  .row:hover .lineplay,
  .lineplay:focus-visible,
  .lineplay.on {
    opacity: 1;
  }
  .lineplay:hover,
  .lineplay.on {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .linecopy:hover {
    background: var(--yap-paper-s2);
  }
  /* Where a recording stopped (⏸), or the person dictated mid-meeting. */
  .divider {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    box-sizing: border-box;
    padding: 10px 0;
    color: var(--yap-paper-rule);
  }
  .divider .rule {
    flex: 1;
    border-top: 1px dashed currentColor;
  }
  .divider :global(svg) {
    width: 12px;
    height: 12px;
  }
  .divider.dictated :global(svg) {
    stroke-width: 2.2;
  }
  .dtext {
    font-size: 12px;
    line-height: 16px;
    color: var(--yap-paper-date);
    white-space: nowrap;
  }
  .echonote {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .echonote:hover {
    color: var(--yap-paper-ink);
    text-decoration: underline;
  }
  /* The speaker engine stopped early: quiet, under the last line. */
  .spknote {
    align-self: center;
    margin: 0;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
</style>
