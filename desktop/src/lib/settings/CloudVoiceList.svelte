<script>
  // A cloud voice's list of voices (Settings → Voice Assistant → Voice): one
  // row each, picked as a radio, with ▶ to hear it. ElevenLabsSettings
  // (the account's own voices, then ElevenLabs') and HostedVoiceSettings
  // (OpenAI's, Gemini's, Groq's) share it.
  //
  //   label:        the list's accessible name ("ElevenLabs voice").
  //   groups:       [{ label, voices: [{ id, name, desc }] }]; a group with
  //                 no label shows no heading.
  //   selected:     the picked voice's id.
  //   playing:      the voice whose sample is playing ('' for none).
  //   onpick(v), onpreview(v): picked, ▶ / ■ clicked.
  //   previewTitle(v): ▶'s tooltip.
  let { label, groups = [], selected = '', playing = '', onpick, onpreview, previewTitle = () => 'Preview' } = $props();
</script>

<div class="vlist" role="radiogroup" aria-label={label}>
  {#each groups as g, gi (g.label || gi)}
    {#if g.label}<div class="vgroup">{g.label}</div>{/if}
    {#each g.voices as v (v.id)}
      <div class="vrow" class:sel={v.id === selected}>
        <button type="button" class="vpick" role="radio" aria-checked={v.id === selected} aria-label={v.name} onclick={() => onpick?.(v)}>
          <span class="dot" aria-hidden="true"></span>
          <span class="vname">{v.name}</span>
          {#if v.desc}<span class="vdesc">{v.desc}</span>{/if}
        </button>
        <button
          type="button"
          class="vplay"
          class:on={playing === v.id}
          aria-label={playing === v.id ? `Stop ${v.name}` : `Preview ${v.name}`}
          title={playing === v.id ? 'Stop' : previewTitle(v)}
          onclick={() => onpreview?.(v)}
        >
          {#if playing === v.id}
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2.5" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.9v14.2a1.2 1.2 0 0 0 1.8 1l11.6-7.1a1.2 1.2 0 0 0 0-2L8.8 3.9A1.2 1.2 0 0 0 7 4.9z" /></svg>
          {/if}
        </button>
      </div>
    {/each}
  {/each}
</div>

<style>
  .vlist {
    display: flex;
    flex-direction: column;
    max-height: 264px;
    overflow-y: auto;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .vgroup {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 6px 12px 4px;
    background: var(--yap-s1);
    font-size: 10.5px;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--yap-muted-70, var(--yap-muted));
  }
  .vrow {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: 6px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .vrow:first-child,
  .vgroup + .vrow {
    border-top: none;
  }
  .vrow.sel {
    background: var(--yap-raised-soft);
  }
  .vpick {
    display: flex;
    flex: 1;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
    padding: 8px 12px;
    border: none;
    background: none;
    font: inherit;
    text-align: left;
    color: var(--yap-fg);
    cursor: pointer;
  }
  .vpick:focus-visible,
  .vplay:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--yap-focus-ring);
  }
  .dot {
    flex: 0 0 auto;
    width: 6px;
    height: 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    transform: translateY(-1px);
  }
  .vrow.sel .dot {
    background: var(--yap-ink);
  }
  .vname {
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
  }
  .vdesc {
    overflow: hidden;
    font-size: 11.5px;
    color: var(--yap-muted);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .vplay {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid transparent;
    border-radius: var(--yap-r-full);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .vplay:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .vplay.on {
    color: var(--yap-link);
    background: var(--yap-primary-wash);
  }
  .vplay svg {
    width: 11px;
    height: 11px;
  }
</style>
