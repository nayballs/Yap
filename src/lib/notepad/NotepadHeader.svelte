<script>
  // The notepad's header, Wispr's buttons and spacing (52 px): back (this note
  // in Yap's Notes), ⋯ (copy as Markdown or text, copy the consent message,
  // audio settings while recording, save as .md, transcribe again, find
  // speakers, delete),
  // split the screen with the call (a glass preview on hover, meetings/notepad.rs),
  // Share (a local popover: copy, save, email) with a one-click copy, and the
  // window buttons. The window is undecorated: the header is its drag region.
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { openExternalLink } from '../core/externalLinks.js';
  import { noteMarkdown, noteText, mailtoUrl } from '../core/notepadText.js';
  import { refreshAudio, cantTranscribeAgain, askTranscribeAgain, cantFindSpeakers, findSpeakers } from '../stores/meetingAudio.svelte.js';
  import { copyToClipboard, openSettings, openNoteInYap } from './actions.js';
  import Icon from './NotepadIcon.svelte';

  /**
   * `menu`: the popover open ('more' | 'share' | 'consent' | null), one at a
   * time across the notepad; `ontogglemenu(which)` opens or closes one (the
   * consent message is saved as its popover closes); `consent`: the
   * ConsentMessage; `onflushsave` saves what's being typed first;
   * `onaskdelete` opens the delete dialog.
   */
  let {
    note,
    recordingThis,
    lineCount,
    menu = $bindable(),
    consent,
    ontogglemenu,
    onflushsave,
    onaskdelete,
  } = $props();

  const appWindow = getCurrentWindow();
  let maximized = $state(false);
  let linkCopied = $state(false);

  async function copyNote(asText) {
    menu = null;
    onflushsave();
    if (await copyToClipboard(asText ? noteText(note) : noteMarkdown(note))) {
      toast({ title: asText ? 'Notes copied as text' : 'Notes copied as Markdown', variant: 'success' });
    }
  }
  // The link half of Share: the notes, copied in one click.
  async function copyLink() {
    onflushsave();
    if (await copyToClipboard(noteMarkdown(note))) {
      linkCopied = true;
      setTimeout(() => (linkCopied = false), 1500);
    }
  }
  async function saveNote() {
    menu = null;
    onflushsave();
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const name = (note.title?.trim() || 'Meeting notes').replace(/[\\/:*?"<>|]/g, '-');
      const path = await save({ defaultPath: `${name}.md`, filters: [{ name: 'Markdown', extensions: ['md'] }] });
      if (!path) return;
      await invoke('note_export', { id: note.id, path });
      toast({ title: 'Notes saved', description: path, variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't save the notes", description: String(e), variant: 'destructive' });
    }
  }
  function emailNote() {
    menu = null;
    onflushsave();
    openExternalLink(mailtoUrl(note));
  }
  function copySavedConsent() {
    menu = null;
    consent.copy(true);
  }
  function audioSettings() {
    menu = null;
    openSettings('dictation#microphone');
  }
  function askDelete() {
    menu = null;
    onaskdelete();
  }
  // ⋯ → Transcribe again (asked first in the window's confirm dialog).
  async function transcribeAgain() {
    menu = null;
    if (!note) return;
    onflushsave();
    await refreshAudio(note.id);
    askTranscribeAgain(note, { title: note.title?.trim() || '' });
  }
  // ⋯ → Find speakers (speakers/rerun.rs): progress and Cancel on the
  // transcript's job line.
  async function findThem() {
    menu = null;
    if (!note) return;
    onflushsave();
    await findSpeakers(note.id);
  }
  /** Back: the note in the main window's Notes view. */
  function openInYap() {
    if (!note) return;
    onflushsave();
    openNoteInYap(note.id);
  }

  // Split the screen with the call now; hovering shows where it would go.
  function previewSplit(show) {
    invoke('notepad_split_preview', { show }).catch(() => {});
  }
  async function splitScreen() {
    previewSplit(false);
    try {
      await invoke('notepad_split');
    } catch (e) {
      toast({ kind: 'error', title: "Couldn't split the screen", description: String(e) });
    }
  }

  async function refreshMaximized() {
    try {
      maximized = await appWindow.isMaximized();
    } catch {
      /* keep */
    }
  }

  onMount(() => {
    refreshMaximized();
    let unlisten = null;
    let gone = false;
    appWindow
      .onResized(() => refreshMaximized())
      .then((u) => (gone ? u() : (unlisten = u)));
    return () => {
      gone = true;
      unlisten?.();
      previewSplit(false);
    };
  });
</script>

<header class="hdr" data-tauri-drag-region>
  {#if note}
    <button class="iconbtn back tipstart" aria-label="Open in Yap" data-tip="Open in Yap" onclick={openInYap}>
      <Icon name="back" />
    </button>
  {/if}
  <span class="hdrspace" data-tauri-drag-region></span>
  <div class="hdrright">
    {#if note}
      <div class="popwrap">
        <button
          class="iconbtn"
          class:open={menu === 'more'}
          aria-label="More"
          aria-haspopup="menu"
          aria-expanded={menu === 'more'}
          data-tip={menu === 'more' ? null : 'More'}
          onclick={() => ontogglemenu('more')}
        >
          <Icon name="more" />
        </button>
        {#if menu === 'more'}
          <div class="pop menu" role="menu" aria-label="More">
            <button role="menuitem" class="item" onclick={() => copyNote(false)}>
              <Icon name="markdown" /><span>Copy as Markdown</span>
            </button>
            <button role="menuitem" class="item" onclick={() => copyNote(true)}>
              <Icon name="text" /><span>Copy as text</span>
            </button>
            <button role="menuitem" class="item" onclick={copySavedConsent}>
              <Icon name="chat" /><span>Copy consent message</span>
            </button>
            {#if recordingThis}
              <button role="menuitem" class="item" onclick={audioSettings}>
                <Icon name="audio" /><span>Audio settings</span>
              </button>
            {/if}
            <button role="menuitem" class="item" onclick={saveNote}>
              <Icon name="save" /><span>Save as .md</span>
            </button>
            {#if !recordingThis && (note.noteType === 'meeting' || lineCount)}
              <button
                role="menuitem"
                class="item"
                onclick={transcribeAgain}
                disabled={!!cantTranscribeAgain(note, recordingThis)}
                title={cantTranscribeAgain(note, recordingThis) || 'With the speech model you use now'}
              >
                <Icon name="redo" /><span>Transcribe again</span>
              </button>
              <button
                role="menuitem"
                class="item"
                onclick={findThem}
                disabled={!!cantFindSpeakers(note, recordingThis)}
                title={cantFindSpeakers(note, recordingThis) || 'Tell the call’s voices apart again, from the meeting’s audio'}
              >
                <Icon name="person" /><span>Find speakers</span>
              </button>
            {/if}
            <button role="menuitem" class="item danger" onclick={askDelete}>
              <Icon name="trash" /><span>Delete</span>
            </button>
          </div>
        {/if}
      </div>
    {/if}
    <button
      class="iconbtn"
      aria-label="Split screen with meeting"
      data-tip="Split screen with meeting"
      onclick={splitScreen}
      onpointerenter={() => previewSplit(true)}
      onpointerleave={() => previewSplit(false)}
    >
      <Icon name="split" />
    </button>
    {#if note}
      <div class="popwrap">
        <div class="sharegroup">
          <button
            class="sharebtn"
            class:open={menu === 'share'}
            aria-haspopup="dialog"
            aria-expanded={menu === 'share'}
            onclick={() => ontogglemenu('share')}
          >
            <Icon name="share" />Share
          </button>
          <button class="linkbtn" aria-label="Copy notes" data-tip={linkCopied ? 'Copied' : 'Copy notes'} onclick={copyLink}>
            <span class="swap" class:on={linkCopied}>
              <span class="a"><Icon name="link" /></span>
              <span class="b"><Icon name="check" /></span>
            </span>
          </button>
        </div>
        {#if menu === 'share'}
          <div class="pop share" role="dialog" aria-label="Share notes">
            <p class="pophead">Share notes</p>
            <p class="popsub">Your notes stay on this PC. Copy them, save a file or start an email.</p>
            <button class="item" onclick={() => copyNote(false)}><Icon name="markdown" /><span>Copy as Markdown</span></button>
            <button class="item" onclick={() => copyNote(true)}><Icon name="text" /><span>Copy as text</span></button>
            <button class="item" onclick={saveNote}><Icon name="save" /><span>Save as .md</span></button>
            <button class="item" onclick={emailNote}><Icon name="mail" /><span>Email</span></button>
          </div>
        {/if}
      </div>
    {/if}
    <div class="winbtns">
      <button class="winbtn" aria-label="Minimize" onclick={() => appWindow.minimize()}><Icon name="min" /></button>
      <button class="winbtn max" aria-label={maximized ? 'Restore' : 'Maximize'} onclick={() => appWindow.toggleMaximize()}>
        <Icon name={maximized ? 'restore' : 'max'} />
      </button>
      <button class="winbtn close" aria-label="Close notepad" onclick={() => appWindow.close()}><Icon name="close" /></button>
    </div>
  </div>
</header>

<style>
  /* A bare element selector, so every button's own class (its size and
     weight) wins over this. */
  button {
    font: inherit;
    cursor: pointer;
  }

  .hdr {
    position: relative;
    z-index: 30;
    flex: 0 0 52px;
    display: flex;
    align-items: center;
    padding: 0 12px;
    user-select: none;
    -webkit-user-select: none;
  }
  .hdrspace {
    flex: 1;
    align-self: stretch;
  }
  .hdrright {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .iconbtn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--yap-paper-ink);
    transition:
      background-color 0.15s,
      opacity 0.15s;
  }
  .iconbtn :global(svg) {
    width: 16px;
    height: 16px;
  }
  .iconbtn:hover,
  .iconbtn.open {
    background: var(--yap-paper-s2-strong);
  }
  .iconbtn.back {
    background: var(--yap-paper-s2);
    transition: background-color 0.4s var(--yap-ease-spring);
  }
  .iconbtn.back:hover {
    background: var(--yap-paper-s2-strong);
  }
  .sharegroup {
    display: flex;
    gap: 2px;
  }
  .sharebtn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 32px;
    padding: 0 12px;
    border: none;
    border-radius: 8px 0 0 8px;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    transition: background-color 0.4s var(--yap-ease-spring);
  }
  .sharebtn :global(svg) {
    width: 14px;
    height: 14px;
  }
  .sharebtn:hover,
  .sharebtn.open,
  .linkbtn:hover {
    background: var(--yap-paper-s2-strong);
  }
  .linkbtn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: 0 8px 8px 0;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    transition: background-color 0.1s;
  }
  .swap {
    position: relative;
    width: 16px;
    height: 16px;
  }
  .swap > span {
    position: absolute;
    inset: 0;
    display: flex;
    transition: opacity 0.1s;
  }
  .swap :global(svg) {
    width: 16px;
    height: 16px;
  }
  .swap .b,
  .swap.on .a {
    opacity: 0;
  }
  .swap.on .b {
    opacity: 1;
  }
  .winbtns {
    display: flex;
  }
  .winbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--yap-paper-ink);
    transition: background-color 0.15s;
  }
  .winbtn :global(svg) {
    width: 14px;
    height: 14px;
  }
  .winbtn.max :global(svg) {
    width: 13px;
    height: 13px;
  }
  .winbtn.close :global(svg) {
    width: 16px;
    height: 16px;
  }
  .winbtn:hover {
    background: var(--yap-paper-s2);
  }
  .winbtn.close:hover {
    background: #c42b1c;
    color: #fff;
  }

  /* ---- ⋯ menu and the Share popover ---- */
  .popwrap {
    position: relative;
  }
  .pop {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 184px;
    box-sizing: border-box;
    padding: 6px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper);
    box-shadow: var(--yap-shadow-popover);
    animation: popin 0.08s ease-out;
  }
  @keyframes popin {
    from {
      opacity: 0;
      translate: 0 -4px;
    }
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    box-sizing: border-box;
    padding: 8px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-ink);
    font-size: 15px;
    line-height: 20px;
    text-align: left;
    white-space: nowrap;
  }
  .item :global(svg) {
    width: 14px;
    height: 14px;
    color: var(--yap-paper-text-2);
  }
  .item:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .item:disabled {
    opacity: 0.45;
  }
  .item.danger,
  .item.danger :global(svg) {
    color: var(--yap-paper-danger);
  }
  .pop.share {
    width: 272px;
    padding-top: 12px;
  }
  .pophead {
    margin: 0 10px 2px;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
  }
  .popsub {
    margin: 0 10px 8px;
    font-size: 12px;
    line-height: 17px;
    color: var(--yap-paper-date);
  }
</style>
