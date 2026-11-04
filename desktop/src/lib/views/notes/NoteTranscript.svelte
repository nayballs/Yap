<script>
  // A meeting note's transcript (You/Them chat bubbles, OpenWhispr's
  // MeetingTranscriptChat style), live while recording and kept afterwards:
  // a run of one speaker's lines (named from the note's speaker map,
  // notepadText.js), the menu on a name or a line to rename, merge or
  // correct who said it, a line's words fixed in place (notes/note_edits.rs:
  // double-click, Enter or F2), ▶ a line's stretch of the meeting
  // (meetings/meeting_audio.rs), "You dictated here" markers, a stretch that failed,
  // and the rolling AI notes so far (meetings/meeting_summary.rs) beside it; over it,
  // Action plan, and what's under way on its audio.
  import { tick } from 'svelte';
  import { toast } from '../../ui/toast.svelte.js';
  import { openContextMenu, menuKey, menuOpenFor } from '../../ui/contextMenu.svelte.js';
  import { copyText } from '../../core/itemMenus.js';
  import { transcriptItems, withLineEdit, splitWordAt, canRevert, lineRef } from '../../core/notepadText.js';
  import { editLine, revertLine, deleteLine, splitLine } from '../../core/noteEdits.js';
  import { meetingAudio, jobState, lastPart, recoverMeeting, canPlay, playLine } from '../../stores/meetingAudio.svelte.js';
  import SpeakerMenu from '../../components/SpeakerMenu.svelte';
  import SpeakerCard from '../../components/SpeakerCard.svelte';
  import MeetingJob from '../../components/MeetingJob.svelte';
  import FailedChunk from '../../components/FailedChunk.svelte';
  import { LABEL } from './notesState.svelte.js';
  import { clock } from './noteFormat.js';

  /** `notes`: the view's state (notesState.svelte.js). */
  let { notes } = $props();

  // "You" chunks that were the call coming through the speakers (meetings/meeting.rs
  // echo check): kept, hidden unless asked for. A dictation made mid-meeting
  // is a `dictated` marker (its words were left out of "You"): shown as a
  // quiet "You dictated here" line.
  const echoCount = $derived((notes.selected?.transcript || []).filter((s) => s.echo).length);
  // The transcript as runs of one speaker (named from the note's speaker
  // map, notepadText.js): a bubble per line, the name on the first.
  const transcriptRuns = $derived(
    transcriptItems(notes.selected?.transcript, [], { showEcho: notes.showEcho, speakers: notes.selected?.speakers })
  );
  // "AI notes up to 40:12": where the digests reach, from the meeting's start.
  const digestedUpTo = $derived.by(() => {
    const d = notes.selected?.digests;
    const t0 = notes.selected?.transcript?.[0]?.ts;
    if (!d?.length || t0 == null) return null;
    return clock(d[d.length - 1].endTs - t0);
  });
  function digestRange(d) {
    const t0 = notes.selected?.transcript?.[0]?.ts ?? d.startTs;
    return `${clock(d.startTs - t0)}–${clock(d.endTs - t0)}`;
  }

  function toggleSpeakerMenu(key, voice, lines, start = 'menu') {
    notes.speakerMenu = notes.speakerMenu?.key === key ? null : { key, voice, lines, start };
  }
  // Fixing a line's words (notes/note_edits.rs), as in the meeting notepad:
  // double-click it, Enter or F2 on it, or Edit line in its menu; Enter
  // saves, Escape puts it back, leaving it saves.
  let lineEditEl = $state(null);
  let lineSaving = false;
  /** `split`: Split line from the menu (the words stay; Enter splits at the cursor). */
  async function startEditLine(line, split = false) {
    if (!notes.selected) return;
    notes.speakerMenu = null;
    // `ref`: which line this is, saved whatever moves it meanwhile (the
    // voice separation splitting a line before it, the other window).
    notes.editingLine = { i: line.i, text: notes.selected.transcript?.[line.i]?.text ?? line.text, split, ref: lineRef(notes.selected.transcript, line.i) };
    await tick();
    lineEditEl?.focus();
    if (!split) lineEditEl?.select();
    fitLineEdit();
  }
  // Revert, delete and split (core/noteEdits.js), as in the meeting notepad.
  const lineCtx = {
    note: () => notes.selected,
    origin: LABEL,
    changing: (...f) => notes.changing(...f),
    reload: () => notes.reloadSelected(),
  };
  /** Split the line being edited at the cursor (its words saved first if they changed). */
  async function splitAtCaret() {
    const edit = notes.editingLine;
    if (!edit || !lineEditEl || lineSaving) return;
    const k = splitWordAt(edit.text, lineEditEl.selectionStart ?? 0);
    if (k == null) {
      toast({ title: 'Put the cursor between two words', description: 'The second line starts at the cursor.' });
      return;
    }
    const changed = !edit.split && edit.text.trim() !== (notes.selected?.transcript?.[edit.i]?.text ?? '');
    if (changed) await finishEditLine(true);
    else notes.editingLine = null;
    await splitLine(lineCtx, edit.i, k);
    setTimeout(() => notes.lineEls[edit.i]?.focus({ preventScroll: true }), 0);
  }
  function fitLineEdit() {
    if (!lineEditEl) return;
    lineEditEl.style.height = 'auto';
    lineEditEl.style.height = `${lineEditEl.scrollHeight}px`;
  }
  async function finishEditLine(save) {
    const edit = notes.editingLine;
    if (!edit || lineSaving) return;
    notes.editingLine = null;
    const before = notes.selected?.transcript?.[edit.i];
    if (!save || !notes.selected || !before || edit.text.trim() === before.text) {
      setTimeout(() => notes.lineEls[edit.i]?.focus({ preventScroll: true }), 0);
      return;
    }
    lineSaving = true;
    const id = notes.selected.id;
    try {
      const r = await editLine(id, notes.selected.transcript, edit.i, edit.text, LABEL, edit.ref);
      if (notes.selected?.id === id && r?.changed) {
        notes.changing('transcript');
        const t = withLineEdit(notes.selected.transcript, { index: r.index, line: r.line });
        if (t) notes.selected.transcript = t;
        else notes.reloadSelected();
      }
    } catch (e) {
      toast({ title: "Couldn't change the line", description: String(e), variant: 'destructive' });
    } finally {
      lineSaving = false;
      setTimeout(() => notes.lineEls[edit.i]?.focus({ preventScroll: true }), 0);
    }
  }
  function onLineEditKeydown(e) {
    if (e.key === 'Enter' && !e.isComposing && (e.shiftKey || notes.editingLine?.split)) {
      // Shift+Enter (or Enter in Split line): two lines, at the cursor.
      e.preventDefault();
      splitAtCaret();
    } else if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      finishEditLine(true);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      finishEditLine(false);
    }
  }
  // A focused line: Enter or F2 edits it; its menu key opens its menu.
  function onBubbleKeydown(e, run, line) {
    if (e.target !== e.currentTarget) return;
    if ((e.key === 'Enter' || e.key === 'F2') && !run.echo) {
      e.preventDefault();
      startEditLine(line);
    } else menuKey(e, (k) => lineMenu(k, run, line));
  }

  // A transcript line's right-click menu (ui/contextMenu.svelte.js), as the
  // meeting notepad's: Edit line, Copy line, Who said this? (the speaker
  // menu at the line) — not for a line from the speakers.
  function lineMenu(e, run, line) {
    const selected = notes.selected;
    openContextMenu(
      e,
      [
        { label: 'Edit line', icon: 'pencil', onselect: () => startEditLine(line) },
        canRevert(selected?.transcript?.[line.i])
          ? { label: 'Revert to what Yap heard', icon: 'undo', onselect: () => revertLine(lineCtx, line.i) }
          : null,
        // (Split and Delete: not while it records — the recorder holds places in the transcript.)
        { label: 'Split line…', icon: 'split', disabled: notes.recordingThisNote, onselect: () => startEditLine(line, true) },
        {
          label: 'Copy line',
          icon: 'copy',
          onselect: () => copyText(line.text),
        },
        canPlay(selected?.id, selected?.transcript?.[line.i])
          ? {
              label: meetingAudio.playing === `${selected.id}:${line.i}` ? 'Stop playing' : 'Play this line',
              icon: meetingAudio.playing === `${selected.id}:${line.i}` ? 'stop' : 'play',
              onselect: () => playLine(selected.id, line.i, selected.transcript[line.i]),
            }
          : null,
        !run.echo
          ? {
              label: 'Who said this?',
              icon: 'person',
              onselect: () => (notes.speakerMenu = { key: `l${line.i}`, voice: run.speaker, lines: [line.i], start: 'lines' }),
            }
          : null,
        { separator: true },
        { label: 'Delete line', icon: 'trash', danger: true, disabled: notes.recordingThisNote, onselect: () => deleteLine(lineCtx, line.i) },
      ],
      { label: 'Transcript line', row: notes.lineEls[line.i] }
    );
  }
</script>

<div class="transcript">
  <div class="thead">
    {#if notes.selected.digests?.length}
      <!-- Transcript ↔ the rolling AI notes (meetings/meeting_summary.rs) -->
      <span class="tviews">
        <button class="tview" class:on={notes.transcriptView === 'live'} onclick={() => (notes.transcriptView = 'live')}>
          Transcript
        </button>
        <button class="tview" class:on={notes.transcriptView === 'notes'} onclick={() => (notes.transcriptView = 'notes')}>
          AI notes so far
        </button>
      </span>
    {:else}
      <span class="tcap">Meeting transcript</span>
    {/if}
    {#if notes.recordingThisNote}
      <span class="tlive"><span class="recdot"></span> listening — transcribes every ~15 s</span>
    {/if}
    <span class="tspacer"></span>
    {#if digestedUpTo}
      <span class="tdigest" title="Yap summarises the meeting as it goes, so the action plan is ready in seconds however long it runs">
        AI notes up to {digestedUpTo}
      </span>
    {/if}
    {#if !notes.recordingThisNote && notes.selected.transcript?.length}
      <button
        class="planbtn"
        onclick={(e) => notes.summariseMeeting(e.currentTarget)}
        disabled={notes.enhancing.has(notes.selected.id) ||
          notes.jobRunning ||
          (notes.meeting.recording && notes.meeting.noteId === notes.selected.id)}
        title="Who does what by when, with decisions and open questions"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 11l3 3 8-8" /><path d="M20 12v7a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h9" /></svg>
        Action plan
      </button>
    {/if}
  </div>
  {#if jobState(notes.selected.id)}
    <!-- Its audio being transcribed (meetings/meeting_audio.rs), with Cancel; a refine's "Update summary". -->
    <MeetingJob noteId={notes.selected.id} look="app" />
  {:else if meetingAudio.byNote[notes.selected.id]?.recoverableSecs && !notes.recordingThisNote}
    <!-- What a crash left of it: the audio is kept. -->
    <div class="trecover" role="status">
      <span>
        Yap closed while this meeting was recording. The last
        {lastPart(meetingAudio.byNote[notes.selected.id].recoverableSecs)} transcribed; the audio is kept.
      </span>
      <button class="planbtn" onclick={() => recoverMeeting(notes.selected.id)}>Transcribe it</button>
    </div>
  {/if}
  {#if notes.selected.transcript?.length}
    <!-- "Speaker 2 is Priya?" (speakers/naming.rs) -->
    <SpeakerCard note={notes.selected} origin={LABEL} onchanged={notes.onSpeakerChanged} />
  {/if}
  {#if notes.recordingThisNote && !notes.selected.transcript?.length}
    <p class="thint">
      Your mic is <strong>You</strong>, the call audio is <strong>Them</strong>. Use
      headphones so your mic doesn't also hear the call.
    </p>
  {/if}
  {#if notes.transcriptView === 'notes' && notes.selected.digests?.length}
    <div class="tscroll" role="region" aria-label="AI notes so far">
      {#each notes.selected.digests as d, i (i)}
        <div class="part">
          <span class="partrange">{digestRange(d)}</span>
          <ul>
            {#each d.keyPoints || [] as p, j (j)}<li>{p}</li>{/each}
            {#each d.decisions || [] as p, j (j)}<li><strong>Decided:</strong> {p}</li>{/each}
            {#each d.actions || [] as a, j (j)}
              <li class="ptask">☐ {a.owner}: {a.task}{a.due ? ` (due ${a.due})` : ''}</li>
            {/each}
          </ul>
        </div>
      {/each}
    </div>
  {:else}
    <div
      class="tscroll"
      role="log"
      aria-label="Meeting transcript"
      bind:this={notes.logEl}
      onscroll={() => (notes.followLog = notes.logEl.scrollHeight - notes.logEl.scrollTop - notes.logEl.clientHeight < 40)}
    >
      {#each transcriptRuns as run (run.key)}
        {#if run.kind === 'dictated'}
          <div class="dictated" title="Yap kept what you dictated out of the meeting's transcript and summaries">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v3" /></svg>
            <span>You dictated here · left out of the notes</span>
          </div>
        {:else if run.kind === 'asleep'}
          <div class="dictated" role="separator" aria-label="Your PC was asleep">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M19.5 14.5A7.5 7.5 0 0 1 9.5 4.5a7.5 7.5 0 1 0 10 10z" /></svg>
            <span>Your PC was asleep for {run.words}</span>
          </div>
        {:else if run.kind === 'mic'}
          <div class="dictated" role="separator" aria-label="Your mic was disconnected">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v3" /></svg>
            <span>Your mic was disconnected{run.next ? ` · now ${run.next}` : ' · none connected'}</span>
          </div>
        {:else if run.kind === 'failed'}
          <FailedChunk
            noteId={notes.selected.id}
            seg={notes.selected.transcript[run.i]}
            why={notes.recordingThisNote
              ? 'After the meeting'
              : meetingAudio.byNote[notes.selected.id] && !meetingAudio.byNote[notes.selected.id].recordings
                ? "This meeting's audio isn't kept"
                : null}
          />
        {:else if run.kind === 'group'}
          {#each run.lines as line, j (line.i)}
            <!-- Focusable for its menu from the keyboard (the ContextMenu key, Shift+F10). -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="bubble {run.source}"
              class:echo={run.echo}
              class:cont={j > 0}
              class:menuopen={menuOpenFor(notes.lineEls[line.i]) || notes.speakerMenu?.key === `l${line.i}`}
              bind:this={notes.lineEls[line.i]}
              class:editing={notes.editingLine?.i === line.i}
              class:hilite={notes.hilite === line.i}
              role="article"
              tabindex="0"
              aria-label={`${run.label}: ${line.text}${line.edited ? ' (edited)' : ''}`}
              aria-keyshortcuts="Enter F2"
              oncontextmenu={(e) => lineMenu(e, run, line)}
              ondblclick={(e) => !e.target.closest('button, textarea') && notes.editingLine?.i !== line.i && startEditLine(line)}
              onkeydown={(e) => onBubbleKeydown(e, run, line)}
            >
              {#if j === 0}
                {#if run.echo}
                  <span class="who">{run.label} · from the speakers</span>
                {:else}
                  <span class="spkwrap">
                    <button
                      class="who"
                      class:guess={run.guess}
                      aria-haspopup="menu"
                      aria-expanded={notes.speakerMenu?.key === run.key}
                      title={run.guess ? 'A guess: click to confirm or change' : 'Rename, merge or correct this speaker'}
                      onclick={() => toggleSpeakerMenu(run.key, run.speaker, run.lines.map((l) => l.i))}
                    >{run.label}</button>
                    {#if notes.speakerMenu?.key === run.key}
                      <SpeakerMenu
                        note={notes.selected}
                        voice={notes.speakerMenu.voice}
                        lines={notes.speakerMenu.lines}
                        origin={LABEL}
                        align={run.source === 'you' ? 'right' : 'left'}
                        onclose={() => (notes.speakerMenu = null)}
                        onchanged={notes.onSpeakerChanged}
                      />
                    {/if}
                  </span>
                {/if}
              {/if}
              {#if notes.editingLine?.i === line.i}
                <textarea
                  class="lineedit"
                  rows="1"
                  readonly={notes.editingLine.split}
                  aria-label={notes.editingLine.split
                    ? 'Split line: put the cursor where the new line starts, then press Enter. Escape cancels'
                    : 'Edit line: Enter saves, Shift+Enter splits it at the cursor, Escape cancels'}
                  bind:this={lineEditEl}
                  bind:value={notes.editingLine.text}
                  oninput={fitLineEdit}
                  onkeydown={onLineEditKeydown}
                  onblur={() => finishEditLine(!notes.editingLine?.split)}
                ></textarea>
                {#if notes.editingLine.split}
                  <span class="splithint" aria-hidden="true">Click where the new line starts, then press Enter</span>
                {/if}
              {:else}
                <p>
                  {line.text}{#if line.edited}<span
                      class="lineedited"
                      title={line.original && line.original !== line.text
                        ? `You edited this line. Yap heard: “${line.original}”`
                        : 'You split this line'}>edited</span
                    >{/if}
                </p>
              {/if}
              {#if canPlay(notes.selected.id, notes.selected.transcript[line.i])}
                <!-- ▶ this line's stretch of the meeting, both sides (meetings/meeting_audio.rs). -->
                <button
                  class="lineplay"
                  class:on={meetingAudio.playing === `${notes.selected.id}:${line.i}`}
                  aria-label={meetingAudio.playing === `${notes.selected.id}:${line.i}` ? 'Stop playing' : 'Play this line'}
                  title={meetingAudio.playing === `${notes.selected.id}:${line.i}` ? 'Stop' : 'Play this line'}
                  onclick={() => playLine(notes.selected.id, line.i, notes.selected.transcript[line.i])}
                >
                  {#if meetingAudio.playing === `${notes.selected.id}:${line.i}`}
                    <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" /></svg>
                  {:else}
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13l10.5-6.5z" fill="currentColor" /></svg>
                  {/if}
                </button>
              {/if}
              {#if notes.speakerMenu?.key === `l${line.i}`}
                <!-- "Who said this?" from the line's menu: the speaker menu at the line. -->
                <SpeakerMenu
                  note={notes.selected}
                  voice={notes.speakerMenu.voice}
                  lines={notes.speakerMenu.lines}
                  start="lines"
                  origin={LABEL}
                  align={run.source === 'you' ? 'right' : 'left'}
                  onclose={() => (notes.speakerMenu = null)}
                  onchanged={notes.onSpeakerChanged}
                />
              {/if}
            </div>
          {/each}
        {/if}
      {/each}
    </div>
    {#if echoCount}
      <button class="echonote" onclick={() => (notes.showEcho = !notes.showEcho)}>
        {notes.showEcho ? 'Hide' : 'Show'}
        {echoCount}
        {echoCount === 1 ? 'line' : 'lines'} your mic picked up from the speakers (left out of
        summaries — headphones avoid this)
      </button>
    {/if}
  {/if}
</div>

<style>
  .recdot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-danger);
    animation: recpulse 1.2s ease-in-out infinite;
  }
  @keyframes recpulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  /* meeting transcript */
  .transcript {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 10px 12px;
    max-height: 240px;
  }
  .thead {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 24px;
  }
  .tspacer {
    flex: 1;
  }
  .tviews {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
  }
  .tview {
    height: 22px;
    padding: 0 9px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .tview.on {
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
  }
  .tdigest {
    font-size: 11px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .planbtn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 11px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .planbtn:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .planbtn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .planbtn svg {
    width: 12px;
    height: 12px;
  }
  .part {
    font-size: 12px;
    line-height: 1.5;
  }
  .partrange {
    font-size: 10.5px;
    font-weight: 700;
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
  }
  .part ul {
    margin: 2px 0 4px;
    padding-left: 18px;
  }
  .part li.ptask {
    list-style: none;
    margin-left: -14px;
  }
  .tcap {
    font-size: 10.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .tlive {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--yap-danger);
  }
  .thint {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.5;
  }
  .tscroll {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bubble {
    position: relative;
    max-width: 82%;
    border-radius: 10px;
    padding: 6px 10px;
    font-size: 12px;
    line-height: 1.5;
  }
  .bubble:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .bubble.menuopen {
    box-shadow: 0 0 0 2px var(--yap-border-hover);
  }
  /* A line being fixed (notes/note_edits.rs), and one opened from the Tasks view. */
  .bubble.editing {
    width: 82%;
    box-shadow: 0 0 0 2px var(--yap-focus-ring);
  }
  .bubble.hilite {
    box-shadow: 0 0 0 2px var(--yap-primary-line);
    transition: box-shadow 0.4s ease;
  }
  .lineedit {
    display: block;
    box-sizing: border-box;
    width: 100%;
    margin: 2px 0 0;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font-family: var(--yap-font-reading);
    font-size: inherit;
    line-height: inherit;
    resize: none;
    overflow: hidden;
  }
  .lineedit:focus {
    outline: none;
  }
  /* Split line: where to click (the words stay as they are). */
  .splithint {
    display: block;
    margin-top: 3px;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .lineedited {
    margin-left: 6px;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-muted-70);
    cursor: help;
  }
  /* The line itself is reading text (Settings → General → Appearance); its
     speaker label and menu keep Yap's font. */
  .bubble p {
    margin: 0;
    overflow-wrap: anywhere;
    font-family: var(--yap-font-reading);
    font-size: calc(12px * var(--yap-reading-scale));
    line-height: calc(1.5 * var(--yap-reading-leading));
  }
  .bubble .who {
    display: block;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    margin-bottom: 1px;
    color: var(--yap-muted-55);
  }
  /* The next lines of one speaker's run sit close, without the name. */
  .bubble.cont {
    margin-top: -3px;
  }
  .spkwrap {
    position: relative;
    display: inline-block;
  }
  .bubble button.who {
    padding: 0 3px;
    margin: 0 -3px 1px;
    border: none;
    border-radius: 4px;
    background: none;
    font-family: inherit;
    line-height: inherit;
    cursor: pointer;
  }
  .bubble button.who:hover,
  .bubble button.who[aria-expanded='true'] {
    background: var(--yap-s2);
  }
  .bubble button.who.guess {
    font-style: italic;
  }
  .bubble.you {
    align-self: flex-end;
    background: var(--yap-primary-wash);
  }
  .bubble.you .who {
    color: var(--yap-primary);
  }
  .bubble.them {
    align-self: flex-start;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
  }
  .bubble.echo {
    opacity: 0.55;
    border: 1px dashed var(--yap-border);
  }
  /* ▶ a line (meetings/meeting_audio.rs): beside it, on hover or focus. */
  .lineplay {
    position: absolute;
    top: 3px;
    right: -25px;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 50%;
    background: var(--yap-s3);
    color: var(--yap-fg-62);
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.12s;
  }
  .bubble.you .lineplay {
    right: auto;
    left: -25px;
  }
  .lineplay svg {
    width: 10px;
    height: 10px;
  }
  .bubble:hover .lineplay,
  .bubble:focus-within .lineplay,
  .lineplay.on {
    opacity: 1;
  }
  .lineplay:hover,
  .lineplay.on {
    color: var(--yap-primary);
  }
  /* What a crash left of its audio (a job's line is MeetingJob.svelte). */
  .trecover {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--yap-s2);
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--yap-fg-62);
  }
  .trecover span {
    flex: 1;
  }
  /* "You dictated here": a dictation made mid-meeting, left out of "You". */
  .dictated {
    align-self: stretch;
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 1px 0;
    font-size: 10.5px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .dictated::before,
  .dictated::after {
    content: '';
    flex: 1 1 auto;
    border-top: 1px dashed var(--yap-border);
  }
  .dictated svg {
    flex: 0 0 auto;
    width: 11px;
    height: 11px;
  }
  .echonote {
    align-self: flex-start;
    border: none;
    background: none;
    padding: 0;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }
  .echonote:hover {
    color: var(--yap-fg);
    text-decoration: underline;
  }
</style>
