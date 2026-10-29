<script>
  // One message in Chat's thread: over an answer, what its tools did as
  // chips ("Edited "Launch plan": …"; a globe for the web, a shield for an
  // action refused after reading it; the call under way while it streams);
  // the bubble — an answer's markdown, written as it streams (half-written
  // markdown closed off, a soft caret at its end) with its citations as
  // pills, or a question with its screenshots and the mic if it was asked
  // by voice; "Stopped" under a question stopped before any answer came;
  // a web answer's Sources; and under an answer, ▶ Read aloud and the model
  // that wrote it. Its right-click menu: Copy / Copy as Markdown / Read
  // aloud, a question's Copy.
  import { renderMarkdown } from '../../core/markdown.js';
  import { modelLine, whereItRuns } from '../../core/aiModel.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { closePartial, settle } from '../../core/streaming.js';
  import { menuKey, openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { chatImageUrl, removedLine, sizeLine, imageItems, splitScreenText } from './chatImages.js';
  import { thoughtLabel } from './chatThink.svelte.js';

  /**
   * `m`: the message; `last`: the thread's last; `sources`: what its
   * citations point at (chatState's `citedSources`); `reading`: being read
   * aloud; `keepDays`: how long screenshots are kept; `onview(id)` opens a
   * screenshot large, `onmenu(e)` its right-click menu, `onread()` ▶,
   * `onsource(s)` opens a passage of the person's meetings at its line (an
   * answer asked across meetings: search/ask.rs).
   */
  let { m, last, sources, reading, keepDays, onview, onmenu, onread, onsource = null } = $props();

  // A chip about the web (web.rs) or the safety rule (chat/tools.rs).
  const WEB_CHIP = /^(Searched the web|Couldn't search the web|Didn't search for|Read [^"]|Couldn't read [^"]|Didn't read |Answered from )/;
  // The calendar's (chat/tools/calendar.rs).
  const CALENDAR_CHIP = /^(Checked your calendar|Found \d+ meetings? with|No meetings with|Free |Busy |No calendar connected|Couldn't read the (day|time) )/;
  const chipKind = (t) =>
    /: not after reading the web$/.test(t) ? 'guard' : WEB_CHIP.test(t) ? 'web' : CALENDAR_CHIP.test(t) ? 'calendar' : 'tool';

  // A question whose screenshot went as its text (snip/ocr.rs): its own
  // words, and the text read from the picture, folded under them.
  const said = $derived(m.role === 'user' ? splitScreenText(m.text) : { question: m.text, screenText: null });

  /** A screenshot's right-click menu (its own: not the question's). */
  function shotMenu(e, img) {
    e.stopPropagation();
    openContextMenu(e, imageItems(img, { onview }), { label: 'Screenshot' });
  }

  // A passage of the person's meetings and notes, or a meeting on their
  // calendar (chat/tools/calendar.rs), not a web page.
  const isPassage = (s) => (s?.note != null || !!s?.event) && !s?.url;
  const fromMeetings = $derived((m.sources?.length ?? 0) > 0 && m.sources.every(isPassage));
  const fromCalendar = $derived(fromMeetings && m.sources.every((s) => !!s.event));
  const sourcesLabel = $derived(fromCalendar ? 'From your calendar' : fromMeetings ? 'From your meetings' : 'Sources');

  // A passage pill in the answer ("[2]"): opened in Yap at its line.
  function clickAnswer(e) {
    const a = e.target instanceof Element ? e.target.closest('a.md-passage') : null;
    if (!a) return;
    e.preventDefault();
    const n = Number(a.textContent);
    const s = (sources || []).find((x) => x.n === n);
    if (s) onsource?.(s);
  }
</script>

{#if m.role === 'assistant' && (m.tools?.length || m.doing)}
  <!-- Tool-activity chips (OpenWhispr shows tool calls above the answer) -->
  <div class="toolchips">
    {#each m.tools as t, j (j)}
      {@const kind = chipKind(t)}
      <span class="toolchip {kind}">
        {#if kind === 'web'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18z" /></svg>
        {:else if kind === 'calendar'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="5" width="17" height="15" rx="2.5" /><path d="M3.5 10h17" /><path d="M8 3v4M16 3v4" /></svg>
        {:else if kind === 'guard'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 4.5 6v5.5c0 4.6 3.1 8.2 7.5 9.5 4.4-1.3 7.5-4.9 7.5-9.5V6z" /><path d="m9 12 2 2 4-4" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.7 6.3a4.5 4.5 0 0 0 5.8 5.8L17 15.6a4.5 4.5 0 0 1-5.8-5.8z" transform="rotate(90 12 12)" /><path d="m3 21 6.5-6.5" /><path d="M14.7 6.3 21 3l-3.3 6.3" /></svg>
        {/if}
        {t}
      </span>
    {/each}
    {#if m.streaming && m.doing}
      <!-- The tool call under way. -->
      <span class="toolchip doing" data-testid="chat-doing" aria-live="polite">
        <span class="spin" aria-hidden="true"></span>
        {m.doing}
      </span>
    {/if}
  </div>
{/if}
{#if m.role === 'assistant' && !(m.streaming && !m.text) && (m.thinking || m.liveThinking)}
  <!-- Thought through (Think, chat/think.rs): the thinking, folded above
       the answer; only the time when the model doesn't send it back. -->
  {@const thought = (m.thinking?.text ?? m.liveThinking ?? '').trim()}
  {#if thought}
    <details class="thought" data-testid="chat-thought">
      <summary>{m.streaming ? 'Thought it through' : thoughtLabel(m.thinking?.ms)}</summary>
      <div class="thoughttext">{thought}</div>
    </details>
  {:else}
    <p class="thoughtline" data-testid="chat-thought">{thoughtLabel(m.thinking?.ms)}</p>
  {/if}
{/if}
{#if m.role === 'assistant' && m.streaming && !m.text}
  <!-- Tools run, or a dictation is being waited out, before the first words. -->
  <div class="cbubble assistant" aria-busy="true">
    <p class="cbody thinking">{m.waiting ? 'Waiting for your dictation to finish…' : 'Thinking…'}</p>
    {#if m.liveThinking}
      <!-- Its thinking as it comes (Think), folded. -->
      <details class="thought live" data-testid="chat-thinking">
        <summary>Show its thinking</summary>
        <div class="thoughttext">{m.liveThinking}</div>
      </details>
    {/if}
  </div>
{:else}
<!-- Focusable for its menu from the keyboard (the ContextMenu key, Shift+F10). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="cbubble {m.role}"
  class:reading
  class:streaming={m.streaming}
  role="article"
  tabindex="0"
  aria-label={m.role === 'assistant' ? 'Answer' : 'Your question'}
  aria-busy={m.streaming ? 'true' : undefined}
  data-testid={m.streaming ? 'chat-streaming' : undefined}
  data-msg-id={m.id || undefined}
  use:settle={m.revisedFrom || 0}
  oncontextmenu={onmenu}
  onkeydown={(e) => e.target === e.currentTarget && menuKey(e, onmenu)}
>
  {#if m.role === 'assistant'}
    <!-- (Its passage pills are links: Enter on one clicks it.) -->
    <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="cbody" onclick={clickAnswer}>{@html renderMarkdown(m.streaming ? closePartial(m.text) : m.text, { sources })}</div>
    {#if m.streaming && m.waiting}
      <p class="cwait" aria-live="polite">Waiting for your dictation to finish…</p>
    {/if}
  {:else}
    {#if m.images?.length}
      <!-- The screenshots it asked about (snip.rs); one past "Keep chat
           screenshots" says so in its place. -->
      <div class="shots" role="group" aria-label="Screenshots">
        {#each m.images as img (img.id)}
          {#if img.gone}
            <span class="shot gone" data-testid="chat-shot-gone">{removedLine(keepDays)}</span>
          {:else}
            <button
              type="button"
              class="shot"
              class:astext={img.asText}
              title={img.asText ? 'Sent as its text (read on this PC)' : 'Open the screenshot'}
              aria-label={`Screenshot, ${sizeLine(img)}${img.asText ? ', sent as its text' : ''}`}
              onclick={() => onview(img.id)}
              oncontextmenu={(e) => shotMenu(e, img)}
            >
              <img src={chatImageUrl(img.id)} alt="" style={`aspect-ratio: ${img.w} / ${img.h}`} />
              {#if img.asText}<span class="astextnote">Sent as text</span>{/if}
            </button>
          {/if}
        {/each}
      </div>
    {/if}
    <p class="cbody">
      {#if m.voice}
        <span class="viavoice" title="Asked by voice" aria-label="Asked by voice">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
        </span>
      {/if}{said.question}
    </p>
    {#if said.screenText}
      <!-- What Windows read from the screenshot, sent in its place. -->
      <details class="screentext" data-testid="chat-screen-text">
        <summary>Text from the screenshot</summary>
        <pre>{said.screenText}</pre>
      </details>
    {/if}
  {/if}
</div>
{/if}
{#if m.role === 'user' && m.stopped && last}
  <!-- Stopped before any of the answer came. -->
  <p class="stoppednote">Stopped</p>
{/if}
{#if m.role === 'assistant' && m.sources?.length}
  <!-- Where an answer came from: a web page opens in the browser; a passage
       of the person's meetings opens its meeting at the line. -->
  <div class="sources" role="group" aria-label={sourcesLabel}>
    <span class="srclabel">{sourcesLabel}</span>
    {#each m.sources as s (s.n)}
      {#if s.screen}
        <!-- The window in front's text, read on this PC (the Voice
             Assistant to a model that can't read images): nothing to open. -->
        <span class="srcchip screen" data-testid="chat-screen-source" title="Yap read the window's text on this PC and sent it with the question">
          <span class="srcn" aria-hidden="true">{s.n}</span>
          <span class="srctitle">{s.title}</span>
        </span>
      {:else if isPassage(s)}
        <button
          class="srcchip passage"
          class:meeting={!!s.event}
          data-testid={s.event ? 'chat-calendar-source' : undefined}
          title={s.quote || s.title}
          aria-label={`${s.event ? 'Meeting' : 'Passage'} ${s.n}: ${s.title}${s.site ? `, ${s.site}` : ''}. Opens ${s.event ? 'its notes' : 'it'}.`}
          onclick={() => onsource?.(s)}
        >
          <span class="srcn" aria-hidden="true">{s.n}</span>
          <span class="srctitle">{s.title}</span>
          {#if s.site}<span class="srcsite">{s.site}</span>{/if}
        </button>
      {:else}
        <button class="srcchip" title={s.url} onclick={() => openExternalLink(s.url)}>
          <span class="srcn" aria-hidden="true">{s.n}</span>
          <span class="srctitle">{s.title || s.site}</span>
          <!-- (Google Search's links are titled with their site: once is enough.) -->
          {#if s.site && s.site !== s.title}<span class="srcsite">{s.site}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}
{#if m.role === 'assistant' && !m.streaming}
  <div class="msgfoot">
    {#if m.id}
      <button
        class="readbtn"
        class:on={reading}
        aria-label={reading ? 'Stop reading' : 'Read aloud'}
        title={reading ? 'Stop reading (Esc)' : 'Read aloud'}
        onclick={onread}
      >
        {#if reading}
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2.5" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.9v14.2a1.2 1.2 0 0 0 1.8 1l11.6-7.1a1.2 1.2 0 0 0 0-2L8.8 3.9A1.2 1.2 0 0 0 7 4.9z" /></svg>
        {/if}
      </button>
    {/if}
    {#if reading}
      <span class="speaking" role="status">
        <span class="bars" aria-hidden="true"><i></i><i></i><i></i></span>
        Speaking…
      </span>
    {:else if m.model}
      <!-- The model that wrote it (older answers don't say). -->
      <span class="byline" title={whereItRuns(m.model)}>{modelLine(m.model)}</span>
    {/if}
    {#if m.stopped}
      <span class="byline stopped" title="You stopped this answer while it was being written">· Stopped</span>
    {/if}
  </div>
{/if}

<style>
  /* Reading text: Settings → General → Appearance (app.css). */
  .cbubble {
    max-width: 78%;
    border-radius: 12px;
    padding: 8px 12px;
    font-family: var(--yap-font-reading);
    font-size: calc(12.5px * var(--yap-reading-scale));
    line-height: calc(1.6 * var(--yap-reading-leading));
  }
  .cbubble.user {
    align-self: flex-end;
    background: var(--yap-primary-wash);
  }
  .cbubble.assistant {
    align-self: flex-start;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .cbubble:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .cbody {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .cbody.thinking {
    color: var(--yap-muted-55);
    font-style: italic;
  }
  /* An answer being written: a soft caret at the end of its last line. */
  .cbubble.streaming .cbody > :global(:is(p, h2, h3, h4, h5, blockquote):last-child)::after,
  .cbubble.streaming .cbody > :global(:is(ul, ol):last-child > li:last-child)::after {
    content: '';
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-left: 5px;
    border-radius: 50%;
    background: var(--yap-primary);
    vertical-align: middle;
    animation: caret 1s ease-in-out infinite;
  }
  @keyframes caret {
    0%,
    100% {
      opacity: 0.25;
    }
    50% {
      opacity: 0.9;
    }
  }
  .toolchip.doing {
    color: var(--yap-fg);
  }
  .spin {
    width: 9px;
    height: 9px;
    border: 1.5px solid var(--yap-border);
    border-top-color: var(--yap-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .cbubble.streaming .cbody > :global(*)::after,
    .cbubble.streaming .cbody > :global(* > li)::after,
    .spin {
      animation: none;
    }
  }
  :global(:root[data-motion='reduced']) .cbubble.streaming .cbody > :global(*)::after,
  :global(:root[data-motion='reduced']) .cbubble.streaming .cbody > :global(* > li)::after,
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  .stoppednote {
    align-self: flex-end;
    margin: -4px 4px 2px 0;
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .cwait {
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .toolchips {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    max-width: 78%;
  }
  .toolchip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    font-size: 10.5px;
    color: var(--yap-muted);
  }
  .toolchip svg {
    width: 10px;
    height: 10px;
    flex: 0 0 auto;
    color: var(--yap-primary);
  }
  .toolchip.guard svg {
    color: var(--yap-success);
  }
  /* A tool's own chip ("Found "beta" on 2 lines in "Launch plan"") is a
     full-width row, 30 px tall, the text turning ink on hover: before
     ChatView was split (2026-10-24) the New chat row's rule (`.tool`)
     reached these chips too, and that's how they look. Kept as they are. */
  .toolchip.tool {
    flex: 1 1 auto;
    width: 100%;
    height: 30px;
    text-align: left;
    cursor: pointer;
  }
  .toolchip.tool:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  /* A web answer's citations: small numbered pills in the text. */
  .cbubble.assistant :global(.md-cites) {
    display: inline-flex;
    gap: 2px;
    margin-left: 2px;
    vertical-align: 1px;
  }
  .cbubble.assistant :global(a.md-cite) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 15px;
    height: 15px;
    padding: 0 4px;
    box-sizing: border-box;
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
    font-size: 9.5px;
    font-weight: 650;
    line-height: 1;
    text-decoration: none;
  }
  .cbubble.assistant :global(a.md-cite:hover) {
    color: var(--yap-primary);
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
  }
  /* The Sources row under a web answer. */
  .sources {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    max-width: 78%;
    margin: -2px 0 0 2px;
  }
  .srclabel {
    margin-right: 2px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .srcchip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 260px;
    padding: 2px 8px 2px 3px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
  }
  .srcchip:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .srcchip:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .srcn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 15px;
    height: 15px;
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-muted);
    font-size: 9.5px;
    font-weight: 650;
  }
  .srctitle {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .srcsite {
    flex: 0 0 auto;
    color: var(--yap-muted-55);
  }
  .cbubble.assistant :global(p) {
    margin: 4px 0;
  }
  .cbubble.assistant :global(ul),
  .cbubble.assistant :global(ol) {
    margin: 4px 0;
    padding-left: 18px;
  }
  .cbubble.assistant :global(h2),
  .cbubble.assistant :global(h3) {
    margin: 8px 0 4px;
    font-size: calc(13px * var(--yap-reading-scale));
  }
  .cbubble.assistant :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(11.5px * var(--yap-reading-step));
    background: var(--yap-s1);
    border-radius: 4px;
    padding: 1px 5px;
  }

  .cbubble.reading {
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
  }
  .viavoice {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 5px;
    color: var(--yap-primary);
  }
  .viavoice svg {
    width: 11px;
    height: 11px;
  }
  /* Under an answer: ▶ Read aloud / ■ Stop, then the model or "Speaking…". */
  .msgfoot {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 20px;
    margin: -4px 0 2px 2px;
  }
  .readbtn {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-muted);
    cursor: pointer;
    flex: 0 0 auto;
  }
  .readbtn:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .readbtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .readbtn.on {
    color: var(--yap-primary);
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
    background: var(--yap-primary-wash);
  }
  .readbtn svg {
    width: 10px;
    height: 10px;
  }
  .speaking {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-primary);
  }
  .bars {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 10px;
  }
  .bars i {
    width: 2px;
    height: 100%;
    border-radius: 1px;
    background: currentColor;
    animation: talk 0.9s ease-in-out infinite;
  }
  .bars i:nth-child(2) {
    animation-delay: 0.15s;
  }
  .bars i:nth-child(3) {
    animation-delay: 0.3s;
  }
  @keyframes talk {
    0%,
    100% {
      transform: scaleY(0.35);
    }
    50% {
      transform: scaleY(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .bars i {
      animation: none;
      transform: scaleY(0.7);
    }
  }
  .byline {
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }

  /* ---- Snip to Chat: the screenshots in a question ---- */
  .shots {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
    margin-bottom: 6px;
  }
  .shot {
    display: block;
    max-width: min(320px, 100%);
    padding: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    overflow: hidden;
    background: var(--yap-s1);
    cursor: zoom-in;
  }
  .shot img {
    display: block;
    width: 100%;
    max-height: 240px;
    object-fit: contain;
  }
  .shot:focus-visible {
    outline: 2px solid var(--yap-focus-ring, var(--yap-primary-line));
    outline-offset: 2px;
  }
  .shot.gone {
    padding: 10px 14px;
    font-family: var(--yap-font-ui, inherit);
    font-size: 11.5px;
    color: var(--yap-muted);
    background: var(--yap-s2);
    border-style: dashed;
    cursor: default;
  }
  /* Sent as its text: the picture stays for you, quieter. */
  .shot.astext {
    position: relative;
  }
  .shot.astext img {
    opacity: 0.75;
  }
  .astextnote {
    position: absolute;
    right: 6px;
    bottom: 6px;
    padding: 1px 7px;
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-muted);
    font-family: var(--yap-font-ui, inherit);
    font-size: 10.5px;
    font-weight: 600;
  }
  .screentext {
    margin-top: 6px;
    font-family: var(--yap-font-ui, inherit);
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .screentext summary {
    cursor: pointer;
  }
  .screentext pre {
    max-height: 180px;
    overflow: auto;
    margin: 6px 0 0;
    padding: 6px 8px;
    border-radius: var(--yap-r-sm, 6px);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* The window's text, read on this PC: a source with nothing to open. */
  .srcchip.screen {
    cursor: default;
  }
  .srcchip.screen:hover {
    border-color: var(--yap-border-subtle);
    background: var(--yap-s1);
  }
  /* Thought through (Think): the thinking, folded above the answer. */
  .thought,
  .thoughtline {
    align-self: flex-start;
    max-width: 78%;
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .thought summary {
    cursor: pointer;
    width: fit-content;
  }
  .thought summary:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
    border-radius: var(--yap-r-sm, 6px);
  }
  .thought.live {
    max-width: none;
    margin-top: 4px;
  }
  .thoughttext {
    max-height: 220px;
    overflow: auto;
    margin: 6px 0 2px;
    padding: 6px 10px;
    border-left: 2px solid var(--yap-border);
    color: var(--yap-muted);
    font-family: var(--yap-font-reading);
    font-size: calc(11.5px * var(--yap-reading-scale));
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
