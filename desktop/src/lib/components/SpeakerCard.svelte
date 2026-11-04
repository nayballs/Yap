<script>
  // The name card over a meeting's transcript (the notepad and the Notes
  // view): "Speaker 2 is Priya?" — Yes / Someone else / Not now. Yap
  // suggests a name from what the meeting said (someone handed over to
  // Priya and this voice answered, someone thanked her right after it
  // spoke, it introduced itself), the invite, or the AI's checked answer
  // (speakers/naming.rs); the person decides. One card at a time, surest
  // first. "Yes" makes the name theirs (locked, and a summary already
  // written says it); "Someone else" never suggests that name for the
  // voice again and opens the rename; "Not now" doesn't ask again in this
  // meeting. A voice repeating yours asks "Speaker 3 is you?" — "It was me"
  // makes its lines yours (Undo in the toast).
  //
  // Before the names, the voice flags (speakers/flags.rs; beta): "Speaker 2
  // and Speaker 4 may be one voice" — ▶ each, Merge / They're different /
  // Not now — and "Speaker 3 may be two people" — Split / It's one person /
  // Not now. Yap never merges or splits by itself. Every card's voice has
  // ▶ (a few seconds of its clearest speech, speakers/clip.rs) while the
  // meeting's audio is kept.
  import { invoke } from '@tauri-apps/api/core';
  import SpeakerMenu from './SpeakerMenu.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import {
    pendingSuggestions,
    suggestionQuestion,
    suggestionWhy,
    speakerInfo,
    defaultSpeakerName,
    pendingFlags,
    flagQuestion,
    flagWhy,
  } from '../core/notepadText.js';
  import { meetingAudio, canPlayVoice, playVoice, voiceKey } from '../stores/meetingAudio.svelte.js';

  let {
    note,
    /** The window's label: the `origin` of the change. */
    origin = '',
    onchanged = () => {},
  } = $props();

  let busy = $state(false);
  // After "Someone else": the rename for that voice.
  let renaming = $state(null);

  const flags = $derived(pendingFlags(note));
  const flag = $derived(flags[0] || null);
  const cards = $derived(flag ? [] : pendingSuggestions(note));
  const card = $derived(cards[0] || null);
  const question = $derived(card ? suggestionQuestion(note, card) : '');
  const why = $derived(card ? suggestionWhy(note, card) : '');
  // A card's ▶s: the voices it's about.
  const listen = $derived(flag ? (flag.kind === 'alike' ? [flag.voice, flag.with] : [flag.voice]) : card && !card.me ? [card.voice] : []);
  const playable = $derived(listen.filter((v) => canPlayVoice(note?.id, v)));
  const shortName = (voice) => {
    const info = speakerInfo(note?.speakers, voice);
    return info.guess ? defaultSpeakerName(voice) : info.label;
  };

  async function answerFlag(reply) {
    if (busy || !flag) return;
    const { kind, voice } = flag;
    const label = shortName(voice);
    const other = flag.kind === 'alike' ? shortName(flag.with) : '';
    busy = true;
    try {
      const updated = await invoke('speaker_flag', { noteId: note.id, speaker: voice, kind, answer: reply, origin });
      onchanged(updated);
      if (reply === 'yes') {
        toast({
          title: kind === 'alike' ? `${label} and ${other} are one speaker now` : `${label} is two speakers now`,
          description: kind === 'alike' ? 'Their lines read as one voice.' : 'The lines that sounded like someone else are a new speaker.',
          variant: 'success',
        });
      }
    } catch (e) {
      toast({ title: "Couldn't change the speakers", description: String(e), variant: 'destructive' });
    } finally {
      busy = false;
    }
  }

  async function answer(reply) {
    if (busy || !card) return;
    const voice = card.voice;
    const label = speakerInfo(note?.speakers, voice).label;
    const me = card.me;
    busy = true;
    try {
      const updated = await invoke('speaker_suggestion', { noteId: note.id, speaker: voice, answer: reply, origin });
      onchanged(updated);
      if (reply === 'no' && !me) renaming = voice;
      if (reply === 'yes' && me) {
        toast({
          title: `${label}'s lines are yours now`,
          description: 'The lines that repeated yours are hidden and left out of the summary.',
          variant: 'success',
          action: {
            label: 'Undo',
            onClick: async () => {
              try {
                onchanged(await invoke('speaker_not_me', { noteId: note.id, speaker: voice, origin }));
              } catch (e) {
                toast({ title: "Couldn't undo it", description: String(e), variant: 'destructive' });
              }
            },
          },
        });
      }
    } catch (e) {
      toast({ title: "Couldn't change the speaker", description: String(e), variant: 'destructive' });
    } finally {
      busy = false;
    }
  }
</script>

{#snippet plays()}
  {#if playable.length}
    <div class="plays">
      {#each playable as v (v)}
        {@const on = meetingAudio.playing === voiceKey(note.id, v)}
        <button
          class="play"
          class:on
          aria-label={on ? `Stop playing ${shortName(v)}` : `Play ${shortName(v)}'s voice`}
          title={on ? 'Stop' : 'A few seconds of their clearest speech'}
          onclick={() => playVoice(note.id, v)}
        >
          {#if on}
            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13l10.5-6.5z" /></svg>
          {/if}
          <span>{shortName(v)}</span>
        </button>
      {/each}
    </div>
  {/if}
{/snippet}

{#if flag && !renaming}
  <section class="namecard flag" aria-label="Speakers that may be wrong" aria-live="polite">
    <span class="ico" aria-hidden="true">
      {#if flag.kind === 'alike'}
        <svg viewBox="0 0 24 24"><circle cx="8.5" cy="9" r="3" /><circle cx="15.5" cy="9" r="3" /><path d="M3 19a5.5 5.5 0 0 1 11 0M10 19a5.5 5.5 0 0 1 11 0" /></svg>
      {:else}
        <svg viewBox="0 0 24 24"><circle cx="12" cy="8" r="3.5" /><path d="M5 19.5a7 7 0 0 1 14 0" /><path d="M12 3v18" stroke-dasharray="2 2.5" /></svg>
      {/if}
    </span>
    <div class="body">
      <p class="q">
        {flagQuestion(note, flag)}
        {#if flags.length > 1}<span class="count">1 of {flags.length}</span>{/if}
      </p>
      <p class="why">{flagWhy(flag)}</p>
      {@render plays()}
      <div class="actions">
        <button class="yes" disabled={busy} onclick={() => answerFlag('yes')}>{flag.kind === 'alike' ? 'Merge' : 'Split'}</button>
        <button class="ghost" disabled={busy} onclick={() => answerFlag('no')}>{flag.kind === 'alike' ? "They're different" : "It's one person"}</button>
        <button class="later" disabled={busy} onclick={() => answerFlag('later')}>Not now</button>
      </div>
    </div>
  </section>
{:else if card || renaming}
  <section class="namecard" aria-label="Who's speaking" aria-live="polite">
    {#if renaming}
      <div class="body">
        <p class="q">Who is {speakerInfo(note?.speakers, renaming).label}?</p>
        <span class="menuwrap">
          <SpeakerMenu
            {note}
            voice={renaming}
            start="all"
            {origin}
            onclose={() => (renaming = null)}
            onchanged={(n) => {
              onchanged(n);
              renaming = null;
            }}
          />
        </span>
      </div>
    {:else if card}
      <span class="ico" aria-hidden="true">
        <svg viewBox="0 0 24 24"><circle cx="10" cy="8" r="3.5" /><path d="M3.5 19.5a6.5 6.5 0 0 1 13 0" /><path d="M17.5 6.5a2.3 2.3 0 1 1 3 2.2c-.6.2-.9.6-.9 1.2v.4" /><path d="M19.6 13.2v.1" /></svg>
      </span>
      <div class="body">
        <p class="q">
          {question}
          {#if cards.length > 1}<span class="count">1 of {cards.length}</span>{/if}
        </p>
        {#if why}<p class="why">{why}</p>{/if}
        {@render plays()}
        <div class="actions">
          <button class="yes" disabled={busy} onclick={() => answer('yes')}>{card.me ? 'It was me' : 'Yes'}</button>
          <button class="ghost" disabled={busy} onclick={() => answer('no')}>{card.me ? 'No' : 'Someone else'}</button>
          <button class="later" disabled={busy} onclick={() => answer('later')}>Not now</button>
        </div>
      </div>
    {/if}
  </section>
{/if}

<style>
  .namecard {
    display: flex;
    gap: 10px;
    margin: 0 0 12px;
    padding: 11px 12px;
    border: 1px solid var(--yap-paper-s2, var(--yap-border));
    border-radius: 12px;
    background: var(--yap-paper-s1, var(--yap-s2));
    color: var(--yap-paper-ink, var(--yap-fg));
    font-size: 14px;
    line-height: 1.4;
  }
  .ico {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--yap-paper, var(--yap-s1));
    border: 1px solid var(--yap-paper-s2, var(--yap-border));
  }
  .ico svg {
    width: 17px;
    height: 17px;
    fill: none;
    stroke: var(--yap-paper-text-2, var(--yap-muted));
    stroke-width: 1.7;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
  }
  .q {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0;
    font-weight: 600;
  }
  .count {
    margin-left: auto;
    font-size: 12px;
    font-weight: 400;
    color: var(--yap-paper-date, var(--yap-muted));
    white-space: nowrap;
  }
  .why {
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-paper-text-2, var(--yap-muted));
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 6px;
  }
  .actions button {
    height: 28px;
    padding: 0 11px;
    border-radius: 8px;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .actions button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .yes {
    border: none;
    background: var(--yap-paper-ink, var(--yap-ink));
    color: var(--yap-paper, #fff);
  }
  .plays {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 4px;
  }
  .play {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 9px 0 7px;
    border: 1px solid var(--yap-paper-s2, var(--yap-border));
    border-radius: 999px;
    background: var(--yap-paper, var(--yap-s1));
    color: var(--yap-paper-ink, var(--yap-fg));
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .play svg {
    width: 12px;
    height: 12px;
    fill: currentColor;
  }
  .play:hover,
  .play.on {
    border-color: var(--yap-paper-s2-strong, var(--yap-border));
    background: var(--yap-paper-s2, var(--yap-s2));
  }
  .ghost {
    border: 1px solid var(--yap-paper-s2-strong, var(--yap-border));
    background: var(--yap-paper, transparent);
    color: var(--yap-paper-ink, var(--yap-fg));
  }
  .later {
    border: none;
    background: none;
    color: var(--yap-paper-text-2, var(--yap-muted));
  }
  .later:hover:not(:disabled) {
    color: var(--yap-paper-ink, var(--yap-fg));
  }
  .menuwrap {
    position: relative;
    display: block;
    min-height: 2px;
  }
  .menuwrap :global(.spkmenu) {
    top: 2px;
  }
</style>
