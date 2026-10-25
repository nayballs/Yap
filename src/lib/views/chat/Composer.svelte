<script>
  // Chat's composer (after Claude Code's): + (Take a screenshot, voice
  // replies and their volume), the message, the mic, and Send once there's
  // something to send — ■ Stop while an answer comes (Esc too); under it,
  // quiet, the model the message goes to (ModelChip, whose picker opens
  // above it all). Snip to Chat (snip.rs): the screenshots this question
  // carries sit over the box with three one-tap questions, and — a model
  // that can't read images — one line saying so, with the switch, or
  // "Send its text instead" (Windows' OCR on this PC: snip/ocr.rs). A
  // picture pasted (Ctrl+V) or dropped on Chat joins them (snip/attach.rs);
  // a screenshot's right-click menu copies or saves it. Nothing goes until
  // you send.
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from '../../ui/toast.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { formatHotkeySpec } from '../../core/hotkeys.js';
  import { modelName, providerName } from '../../core/aiModel.js';
  import { chatImageUrl, SNIP_PROMPTS, sizeLine, imageItems } from './chatImages.js';
  import ModelChip from './ModelChip.svelte';

  /** `chat`: the view's state; `onopensettings(section)`: ControlPanel's Settings. */
  let { chat, onopensettings = null } = $props();

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
    chat.attached = chat.attached.filter((a) => a.id !== id);
    chat.inputEl?.focus();
  }

  /** One of the three prompts: asked at once, with the screenshot. */
  function askAbout(prompt) {
    if (chat.sending || chat.visionBlocked) return;
    chat.input = prompt;
    chat.send();
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

  /** "Send without the screenshot": for a question that doesn't need it. */
  function sendWithout() {
    chat.attached = [];
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
    openContextMenu(e, imageItems(img, { onview: (id) => (chat.viewing = id) }), { label: 'Screenshot' });
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
     message goes to. The model picker opens above it all. -->
<div class="composer" class:dropping={chat.dragging} data-model-picker-above>
  {#if chat.dragging}
    <!-- Files dragged over Chat: a drop puts the pictures in the box. -->
    <div class="dropnote" data-testid="chat-drop-note" aria-live="polite">Drop pictures to ask about them</div>
  {/if}
  {#if chat.attached.length}
    <!-- Snip to Chat (snip.rs): the screenshots this question carries, three
         one-tap questions, and — a model that can't read images — one line
         saying so, with the switch. Nothing goes until you send. -->
    <div class="snipbar" data-testid="chat-attachments">
      {#if chat.visionBlocked}
        <div class="visionline" role="status" data-testid="chat-vision-line">
          <span class="vtext">
            <b>{modelName(chat.vision.model)} can't read images.</b>
            {#if !chat.vision.imagesSwitch}None of your models can: Claude, Gemini and OpenAI models can.{/if}
          </span>
          {#if chat.vision.imagesSwitch}
            <button type="button" class="vbtn primary" onclick={useVisionModel}>
              Use {chat.switchName} on {providerName({ provider: chat.vision.imagesSwitch.provider })}
            </button>
          {/if}
          <button
            type="button"
            class="vbtn"
            class:primary={!chat.vision.imagesSwitch}
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
          <div class="shotchip">
            <button
              type="button"
              class="thumb"
              title="Open the screenshot (right-click to copy or save it)"
              aria-label={`Screenshot, ${sizeLine(img)}`}
              onclick={() => (chat.viewing = img.id)}
              oncontextmenu={(e) => shotMenu(e, img)}
            >
              <img src={chatImageUrl(img.id)} alt="" />
            </button>
            <span class="shotsize">{sizeLine(img)}</span>
            <button type="button" class="shotx" aria-label="Remove the screenshot" title="Remove" onclick={() => removeShot(img.id)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
            </button>
          </div>
        {/each}
        <div class="prompts" role="group" aria-label="Ask about the screenshot">
          {#each SNIP_PROMPTS as p (p)}
            <button
              type="button"
              class="promptchip"
              disabled={chat.sending || chat.visionBlocked}
              title={chat.visionBlocked ? "This model can't read images" : `Ask “${p}”`}
              onclick={() => askAbout(p)}>{p}</button
            >
          {/each}
        </div>
      </div>
    </div>
  {/if}
  <div class="inputbar">
    <div class="pluswrap" bind:this={plusEl}>
      <button
        class="plusbtn"
        aria-label="Chat options"
        aria-haspopup="dialog"
        aria-expanded={chat.plusOpen}
        title={chat.speakReplies ? `Voice replies on · ${Math.round(chat.voiceVolume * 100)}%` : 'Voice replies and volume'}
        onclick={() => (chat.plusOpen = !chat.plusOpen)}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
        <!-- Voice replies are on: a small amber dot, the switch is inside. -->
        {#if chat.speakReplies}<span class="plusdot" aria-hidden="true"></span>{/if}
      </button>
      {#if chat.plusOpen}
        <div class="pluspop" role="dialog" aria-label="Chat options">
          <!-- Snip to Chat (snip.rs): this window leaves the shot. -->
          <button class="optrow" aria-label="Take a screenshot" onclick={takeScreenshot}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="6" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><path d="M20 4 8.1 15.9" /><path d="M14.5 14.5 20 20" /><path d="M8.1 8.1 12 12" /></svg>
            <span class="opttext">
              <span class="optname">Take a screenshot</span>
              <span class="optdesc">Drag a box over any screen{chat.snipHotkey ? ` · ${formatHotkeySpec(chat.snipHotkey)}` : ''}</span>
            </span>
          </button>
          <div class="optsep"></div>
          <!-- Spoken replies on / off (Settings → Voice Assistant → Speak replies). -->
          <button class="optrow" aria-pressed={chat.speakReplies} aria-label="Voice replies" title={voiceTitle} onclick={toggleVoiceReplies}>
            {#if chat.speakReplies}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" fill="currentColor" fill-opacity="0.15" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.4 5.6a9 9 0 0 1 0 12.8" /></svg>
            {:else}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" /><path d="m22 9-6 6M16 9l6 6" /></svg>
            {/if}
            <span class="opttext">
              <span class="optname">Voice replies</span>
              <span class="optdesc">{chat.speakReplies ? (chat.speakWhen === 'always' ? 'Every answer is read aloud' : 'Answers to spoken questions are read aloud') : 'Answers stay quiet'}</span>
            </span>
            <span class="switch" class:on={chat.speakReplies} aria-hidden="true"><span></span></span>
          </button>
          <div class="optvol">
            <label class="vollabel" for="chat-voice-volume">Voice volume</label>
            <div class="volrow">
              <input
                id="chat-voice-volume"
                type="range"
                min="0"
                max="100"
                step="5"
                value={Math.round(chat.voiceVolume * 100)}
                oninput={(e) => moveVolume(Number(e.currentTarget.value) / 100)}
                onchange={keepVolume}
              />
              <span class="volval">{Math.round(chat.voiceVolume * 100)}%</span>
            </div>
          </div>
          <div class="optsep"></div>
          <button
            class="optlink"
            onclick={() => {
              chat.plusOpen = false;
              onopensettings?.('voice');
            }}>Voice settings…</button
          >
        </div>
      {/if}
    </div>
    <input
      class="msginput"
      bind:this={chat.inputEl}
      bind:value={chat.input}
      placeholder={chat.attached.length ? 'Ask about this screenshot…' : 'Ask about your notes, or anything else'}
      aria-label="Message"
      aria-busy={chat.sending}
      onkeydown={(e) => e.key === 'Enter' && !e.isComposing && chat.send()}
      onpaste={chat.pasteImage}
      oninput={() => {
        if (!chat.input.trim()) chat.byVoice = false;
      }}
      onfocus={() => chat.reportFocus(true)}
      onblur={() => chat.reportFocus(false)}
    />
    <!-- Not `disabled` while an answer comes: disabling the focused box
         throws the caret out of it, and you'd have to click back in to
         type the next message. `send()` ignores Enter until the answer is
         in, so you can type ahead. -->
    <button class="micbtn" title="Dictate your message" aria-label="Dictate your message" onclick={micChat}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
    </button>
    {#if chat.sending}
      <!-- ■: stop the answer being written (Esc too). -->
      <button class="sendbtn stopbtn" disabled={chat.stopping} onclick={chat.stopAnswer} aria-label="Stop the answer" title="Stop (Esc)">
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
      </button>
    {:else if chat.input.trim() || chat.attached.length}
      <button
        class="sendbtn"
        disabled={chat.sending || chat.visionBlocked}
        onclick={chat.send}
        aria-label="Send"
        title={chat.visionBlocked ? "This model can't read images" : 'Send (Enter)'}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
      </button>
    {/if}
  </div>
  <div class="composerfoot">
    <ModelChip bind:this={chipRef} {onopensettings} quiet />
  </div>
</div>

<style>
  .vollabel {
    display: block;
    margin-bottom: 6px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .volrow {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .volrow input {
    flex: 1 1 auto;
    min-width: 0;
    accent-color: var(--yap-primary);
  }
  .volval {
    width: 34px;
    text-align: right;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    color: var(--yap-muted);
  }

  .composer {
    flex: 0 0 auto;
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .inputbar {
    display: flex;
    align-items: center;
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
  .plusbtn:focus-visible,
  .optrow:focus-visible,
  .optlink:focus-visible {
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
  .optrow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .optrow:hover {
    background: var(--yap-s3);
  }
  .optrow > svg {
    width: 16px;
    height: 16px;
    flex: 0 0 auto;
    color: var(--yap-muted);
  }
  .optrow[aria-pressed='true'] > svg {
    color: var(--yap-primary);
  }
  .opttext {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .optname {
    font-size: 12.5px;
    font-weight: 600;
  }
  .optdesc {
    font-size: 11.5px;
    color: var(--yap-muted);
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
  .optvol {
    padding: 6px 8px 8px 34px;
  }
  .optsep {
    height: 1px;
    margin: 2px 6px;
    background: var(--yap-border-subtle);
  }
  .optlink {
    width: 100%;
    padding: 7px 8px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .optlink:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .composerfoot {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 0 4px;
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
  .msginput {
    flex: 1 1 auto;
    min-width: 0;
    height: 32px;
    padding: 0 4px;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
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
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .shotchip {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 26px 3px 3px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg, 10px);
    background: var(--yap-s2);
  }
  .thumb {
    display: block;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-sm, 6px);
    overflow: hidden;
    background: var(--yap-s1);
    cursor: zoom-in;
  }
  .thumb img {
    display: block;
    height: 52px;
    max-width: 120px;
    object-fit: cover;
  }
  .shotsize {
    font-size: 10.5px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .shotx {
    position: absolute;
    top: 4px;
    right: 4px;
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
  .shotx:hover {
    background: var(--yap-s3, var(--yap-s1));
    color: var(--yap-fg);
  }
  .shotx svg {
    width: 11px;
    height: 11px;
  }
  .prompts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .promptchip {
    padding: 5px 11px;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r-full, 999px);
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
    font-size: 12px;
    font-weight: 550;
    cursor: pointer;
  }
  .promptchip:hover:not(:disabled) {
    background: var(--yap-primary-tint);
  }
  .promptchip:disabled {
    opacity: 0.5;
    cursor: default;
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
