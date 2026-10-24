<script>
  // The Transcript tab: the timer box (the meeting's time, search, copy all,
  // and a tip about the live transcript), what's under way on its audio — a
  // transcription again or a refine (MeetingJob, with Cancel) or what a
  // crash left (Transcribe it) —, "Speaker 2 is Priya?" (SpeakerCard), and
  // the lines (TranscriptLog), or why there are none yet.
  import { tick } from 'svelte';
  import { clock, transcriptItems, transcriptText } from '../core/notepadText.js';
  import { meetingAudio, jobState, lastPart, recoverMeeting } from '../stores/meetingAudio.svelte.js';
  import MeetingJob from '../components/MeetingJob.svelte';
  import SpeakerCard from '../components/SpeakerCard.svelte';
  import { LABEL, copyToClipboard } from './actions.js';
  import TranscriptLog from './TranscriptLog.svelte';
  import Icon from './NotepadIcon.svelte';

  /**
   * `view`: the TranscriptView (Notepad.svelte keeps it across tabs);
   * `elapsed`: the recording's seconds; `liveTranscript`: Settings →
   * Meetings → Show live transcript.
   */
  let {
    note = $bindable(),
    view,
    recordingThis,
    elapsed,
    liveTranscript,
    speakersNote,
    changing,
    onreload,
    onspeakerchanged,
  } = $props();

  const items = $derived(
    transcriptItems(note?.transcript, note?.breaks, { showEcho: view.showEcho, query: view.searchQuery, speakers: note?.speakers })
  );
  const lineCount = $derived((note?.transcript || []).length);
  const meetingSecs = $derived.by(() => {
    const t = (note?.transcript || []).filter((s) => !s.dictated);
    return t.length > 1 ? t[t.length - 1].ts - t[0].ts : 0;
  });
  // The meeting's audio kept on disk (meetings/meeting_audio.rs): ▶ on its lines,
  // what a crash left of it.
  const audio = $derived(note ? meetingAudio.byNote[note.id] : null);

  let searchEl = $state(null);
  async function toggleSearch() {
    if (view.searchOpen) {
      view.closeSearch();
      return;
    }
    view.searchOpen = true;
    await tick();
    searchEl?.focus();
  }

  // Copy the whole transcript (the button's ✓ says it: no toast on top).
  async function copyTranscript() {
    if (await copyToClipboard(transcriptText(note?.transcript, note?.speakers))) {
      view.transcriptCopied = true;
      setTimeout(() => (view.transcriptCopied = false), 1200);
    }
  }
</script>

{#if recordingThis || lineCount}
<div class="timerbox">
  <span class="clockico"><Icon name="clock" /></span>
  <span class="elapsed">{recordingThis ? clock(elapsed) : clock(meetingSecs)}</span>
  <span class="grow"></span>
  <div class="tboxbtns">
    <button class="tbtn search" aria-label="Search the transcript" aria-expanded={view.searchOpen} onclick={toggleSearch}>
      <Icon name="search" />
    </button>
    <input
      class="tsearch"
      class:open={view.searchOpen}
      bind:this={searchEl}
      bind:value={view.query}
      placeholder="Search"
      aria-label="Search the transcript"
      tabindex={view.searchOpen ? 0 : -1}
      onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), view.closeSearch())}
    />
    <button
      class="tbtn"
      aria-label="Copy transcript"
      data-copied={view.transcriptCopied ? '' : undefined}
      onclick={copyTranscript}
      disabled={!lineCount}
    >
      <Icon name={view.transcriptCopied ? 'check' : 'copy'} />
    </button>
  </div>
  {#if !view.hintGone}
    <div class="hintwrap">
      <div class="hint">
        <p>Yap transcribes this on your PC as people talk: lines land about every 15 seconds, and the last few when you stop.</p>
        <button class="hintx" aria-label="Dismiss the tip" onclick={() => view.dismissHint()}><Icon name="x" /></button>
      </div>
    </div>
  {/if}
</div>
{/if}
{#if note && jobState(note.id)}
  <!-- Its audio being transcribed (meetings/meeting_audio.rs), with Cancel; a refine's "Update summary". -->
  <MeetingJob noteId={note.id} look="paper" />
{:else if audio?.recoverableSecs && !recordingThis}
  <!-- What a crash left of it: the audio is kept. -->
  <div class="recoverbar" role="status">
    <p>
      Yap closed while this meeting was recording. The last {lastPart(audio.recoverableSecs)}
      transcribed; the audio is kept.
    </p>
    <button class="recoverbtn" onclick={() => recoverMeeting(note.id)}>Transcribe it</button>
  </div>
{/if}
{#if note && !view.searchQuery}
  <!-- "Speaker 2 is Priya?" (speakers/naming.rs) -->
  <div class="namecardwrap">
    <SpeakerCard {note} origin={LABEL} onchanged={onspeakerchanged} />
  </div>
{/if}
{#if recordingThis && !liveTranscript}
  <div class="tempty">
    <p class="t1">Live transcript is off</p>
    <p class="t2">Yap is still transcribing on this PC. The transcript shows here when you stop.</p>
  </div>
{:else if view.searchQuery && !items.length}
  <div class="tempty">
    <p class="t1">Nothing matches</p>
    <p class="t2">No line says “{view.searchQuery}”.</p>
  </div>
{:else if !items.length}
  <div class="tempty">
    {#if recordingThis}
      <p class="t1">Yap is listening</p>
      <p class="t2">Say hello: your first line lands here in about 15 seconds. Your mic is You, the call is Them.</p>
    {:else}
      <p class="t1">No transcript yet</p>
      <p class="t2">Press Start and Yap transcribes the meeting here, on this PC.</p>
    {/if}
  </div>
{:else}
  <TranscriptLog bind:note {items} {view} {recordingThis} {audio} {speakersNote} {changing} {onreload} {onspeakerchanged} />
{/if}

<style>
  button {
    font: inherit;
    cursor: pointer;
  }
  .grow {
    flex: 1;
  }

  /* The timer box (clock, time, search, copy) and its tip strip. */
  .timerbox {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    column-gap: 7px;
    margin: 24px 40px 0 32px;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-muted);
  }
  .clockico {
    display: inline-flex;
  }
  .clockico :global(svg) {
    width: 15px;
    height: 15px;
  }
  .elapsed {
    font-size: 12px;
    font-weight: 600;
    line-height: 20px;
    letter-spacing: 0.9px;
    font-variant-numeric: tabular-nums;
  }
  .tboxbtns {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .tbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-faint);
    transition:
      background-color 0.15s,
      color 0.15s;
  }
  .tbtn :global(svg) {
    width: 16px;
    height: 16px;
  }
  .tbtn.search :global(svg) {
    width: 20px;
    height: 20px;
  }
  .tbtn:hover:not(:disabled),
  .tbtn[aria-expanded='true'] {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-text-2);
  }
  .tbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .tsearch {
    width: 0;
    box-sizing: border-box;
    padding: 1px 0;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 15px;
    line-height: 20px;
    outline: none;
    transition:
      width 0.2s ease-out,
      padding 0.2s ease-out;
  }
  .tsearch.open {
    width: 180px;
    padding: 1px 2px;
  }
  .tsearch::placeholder {
    color: var(--yap-paper-faint);
  }
  .hintwrap {
    flex: 0 0 calc(100% + 24px);
    margin: 0 -12px -10px;
    padding-top: 7px;
    font-size: 12px;
    line-height: 20px;
  }
  .hint {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 6px 12px;
    border-radius: 0 0 8px 8px;
    background: var(--yap-paper-s2);
  }
  .hint p {
    flex: 1;
    margin: 0;
  }
  .hintx {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 24px;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
  }
  .hintx :global(svg) {
    width: 14px;
    height: 14px;
  }
  .hintx:hover {
    background: var(--yap-paper-s2-strong);
    color: var(--yap-paper-ink);
  }

  /* "Speaker 2 is Priya?" over the lines. */
  .namecardwrap {
    flex: 0 0 auto;
    margin: 12px 40px 0 32px;
  }
  .namecardwrap :global(.namecard) {
    margin: 0;
  }
  /* What a crash left of its audio (a job's line is MeetingJob.svelte). */
  .recoverbar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 12px 40px 0 32px;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--yap-paper-s1);
  }
  .recoverbar p {
    flex: 1;
    margin: 0;
    font-size: 13px;
    line-height: 18px;
    color: var(--yap-paper-text-2);
  }
  .recoverbtn {
    flex: 0 0 auto;
    height: 30px;
    padding: 0 12px;
    border: none;
    border-radius: 999px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .tempty {
    margin: 72px auto 0;
    max-width: 380px;
    padding: 0 20px;
    text-align: center;
  }
  .tempty .t1 {
    margin: 0;
    font-size: 13px;
    color: var(--yap-paper-faint);
  }
  .tempty .t2 {
    margin: 6px 0 0;
    font-size: 17px;
    line-height: 24px;
    color: var(--yap-paper-date);
  }
</style>
