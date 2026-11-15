<script>
  // **A visual in the answer** (chat/made.rs, 15 Nov 2026; after the Claude app's "Made a visual"): an SVG
  // diagram or a small interactive widget the model made, in a sandboxed frame of its own — the `yapmade`
  // scheme's document with its own strict policy (chat/made/serve.rs), `sandbox="allow-scripts"` here too:
  // an opaque origin with none of Yap's commands. It takes Yap's colours (light and dark), sizes to what it
  // draws, and its buttons may send a message into this chat (`sendPrompt`, visualBridge.js: text only, on
  // the person's click). "Made a visual" over it, ⤢ to see it large, ⌄ to fold it.
  import { registerFrame, visualSrc } from './visualBridge.js';

  /** `v`: the visual (`Made`); `conversation`: its chat; `onprompt(text, v)`: a message its button sends. */
  let { v, conversation, onprompt = null } = $props();

  let open = $state(true);
  let big = $state(false);
  let h = $state(120);
  let bigH = $state(240);
  let frameEl = $state(null);
  let bigEl = $state(null);
  let closeBtn = $state(null);
  const src = $derived(visualSrc(conversation, v.id));

  /** A frame's bridge while it's there; a frame that navigated away (a script's own `location`) put back. */
  function bridge(el, setH) {
    let loads = 0;
    const unregister = registerFrame(el, { onsize: setH, onprompt: (text) => onprompt?.(text, v) });
    const loaded = () => {
      loads += 1;
      if (loads > 1) {
        loads = 0;
        el.src = src;
      }
    };
    el.addEventListener('load', loaded);
    return () => {
      el.removeEventListener('load', loaded);
      unregister();
    };
  }

  $effect(() => {
    if (frameEl) return bridge(frameEl, (n) => (h = n));
  });
  $effect(() => {
    if (bigEl) return bridge(bigEl, (n) => (bigH = n));
  });
  $effect(() => {
    if (big) closeBtn?.focus();
  });

  function keydown(e) {
    if (big && e.key === 'Escape') {
      e.stopPropagation();
      big = false;
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="visual" data-testid="chat-visual" data-visual-id={v.id}>
  <div class="vhead">
    <span class="vlabel" title={v.title}>Made a visual</span>
    <button type="button" class="vbtn" onclick={() => (big = true)} aria-label={`See ${v.title || 'the visual'} large`} title="See it large" data-testid="chat-visual-expand">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-7 7" /><path d="M10 20H4v-6" /><path d="M4 20l7-7" /></svg>
    </button>
    <button type="button" class="vbtn fold" class:shut={!open} onclick={() => (open = !open)} aria-expanded={open} aria-label={open ? 'Fold the visual' : 'Show the visual'} title={open ? 'Fold' : 'Show'} data-testid="chat-visual-fold">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
    </button>
  </div>
  {#if open}
    <div class="vbox">
      <iframe
        bind:this={frameEl}
        {src}
        title={v.title || 'Visual'}
        sandbox="allow-scripts"
        referrerpolicy="no-referrer"
        loading="lazy"
        style:height="{h}px"
        data-testid="chat-visual-frame"
      ></iframe>
    </div>
  {/if}
</div>

{#if big}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="vscrim" onclick={(e) => e.target === e.currentTarget && (big = false)} data-testid="chat-visual-large">
    <div class="vlarge" role="dialog" aria-modal="true" aria-label={v.title || 'Visual'}>
      <div class="vlhead">
        <span class="vltitle">{v.title || 'Visual'}</span>
        <button type="button" class="vbtn" bind:this={closeBtn} onclick={() => (big = false)} aria-label="Close" title="Close (Esc)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
      <div class="vlbody">
        <iframe bind:this={bigEl} {src} title={v.title || 'Visual'} sandbox="allow-scripts" referrerpolicy="no-referrer" style:height="{bigH}px"></iframe>
      </div>
    </div>
  </div>
{/if}

<style>
  .visual {
    margin: 6px 0 10px;
  }
  .vhead {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 6px;
    color: var(--yap-muted);
    font-size: 13px;
  }
  .vlabel {
    margin-right: 2px;
  }
  .vbtn {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--yap-r-sm, 6px);
    background: transparent;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .vbtn:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .vbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring, var(--yap-primary));
    outline-offset: 1px;
  }
  .vbtn svg {
    width: 14px;
    height: 14px;
  }
  .fold svg {
    transition: transform var(--yap-dur, 0.15s) ease;
  }
  .fold.shut svg {
    transform: rotate(-90deg);
  }
  .vbox {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg, 12px);
    padding: 14px 16px;
    background: transparent;
  }
  iframe {
    display: block;
    width: 100%;
    border: 0;
    background: transparent;
    /* The frame's own scheme (its document says light or dark): the same here, or Chromium paints an
       opaque backdrop behind a frame whose scheme differs from its page's. */
    color-scheme: light;
  }
  :global(:root[data-yap-theme='dark']) iframe {
    color-scheme: dark;
  }
  .vscrim {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: var(--yap-scrim);
  }
  .vlarge {
    width: min(1100px, 92vw);
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--yap-r-xl, 14px);
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-modal);
    overflow: hidden;
  }
  .vlhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 12px 10px 18px;
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .vltitle {
    font-weight: 600;
    color: var(--yap-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .vlbody {
    overflow: auto;
    padding: 20px 24px;
  }
</style>
