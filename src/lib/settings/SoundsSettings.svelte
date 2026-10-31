<script>
  // Settings → Dictation → Sounds (dictation/sound.rs): Yap's sounds on or
  // off and how loud, the pack (▶ plays its whole set), where they play, and
  // each moment's switch (▶ plays its sound in the pack chosen). The packs
  // and moments are core/sounds.js; the sounds are made in Rust.
  //
  //   cfg:     Settings' config $state (auto-save does the saving).
  //   outputs: output device names (list_output_devices).
  import { invoke } from '@tauri-apps/api/core';
  import { onDestroy } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import Slider from '../ui/Slider.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { SOUND_PACKS, SOUND_MOMENTS } from '../core/sounds.js';

  let { cfg, outputs = [] } = $props();

  const outputOptions = $derived([{ value: null, label: 'System default' }, ...outputs.map((d) => ({ value: d, label: d }))]);
  const pack = $derived(SOUND_PACKS.some((p) => p.id === cfg.soundPack) ? cfg.soundPack : 'classic');

  // What ▶ is playing ("pack:soft", "moment:soundError") until it ends.
  let playing = $state('');
  let timer = 0;

  async function hear(key, packId, cues) {
    if (playing === key) {
      stop();
      return;
    }
    clearTimeout(timer);
    try {
      const ms = await invoke('sound_preview', { pack: packId, cues, volume: Number(cfg.audioFeedbackVolume) });
      playing = key;
      timer = setTimeout(() => {
        if (playing === key) playing = '';
      }, Number(ms) + 150);
    } catch (e) {
      playing = '';
      toast({ title: "Couldn't play the sounds", description: String(e), variant: 'destructive' });
    }
  }

  function stop() {
    clearTimeout(timer);
    playing = '';
    invoke('sound_preview_stop').catch(() => {});
  }

  onDestroy(() => {
    clearTimeout(timer);
    if (playing) invoke('sound_preview_stop').catch(() => {});
  });
</script>

{#snippet play(key, label)}
  {#if playing === key}
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2.5" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.9v14.2a1.2 1.2 0 0 0 1.8 1l11.6-7.1a1.2 1.2 0 0 0 0-2L8.8 3.9A1.2 1.2 0 0 0 7 4.9z" /></svg>
  {/if}
  <span class="sr">{playing === key ? `Stop ${label}` : `Hear ${label}`}</span>
{/snippet}

<Group title="Sounds" id="settings-sounds">
  <Row
    label="Sounds"
    desc="Short sounds for what Yap is doing, and how loud they are"
    hint="While a meeting records, only a dictation's start and stop sounds play, so nothing else ends up in its notes."
    setting="chime"
  >
    <Slider
      inline
      bind:value={cfg.audioFeedbackVolume}
      min={0}
      max={1}
      step={0.05}
      label="Sound volume"
      disabled={!cfg.soundEnabled}
      format={(v) => `${Math.round(Number(v) * 100)}%`}
    />
    <Toggle bind:checked={cfg.soundEnabled} ariaLabel="Sounds" />
  </Row>
  <Row label="Sound pack" desc="Press ▶ to hear a pack's sounds, one after another" setting="sound-pack" stack>
    <div class="packs" role="radiogroup" aria-label="Sound pack">
      {#each SOUND_PACKS as p (p.id)}
        <div class="pack" class:sel={pack === p.id} class:off={!cfg.soundEnabled}>
          <button
            type="button"
            class="pick"
            role="radio"
            aria-checked={pack === p.id}
            disabled={!cfg.soundEnabled}
            onclick={() => (cfg.soundPack = p.id)}
          >
            <span class="dot" aria-hidden="true"></span>
            <span class="name">{p.name}</span>
            <span class="pdesc">{p.desc}</span>
          </button>
          <button
            type="button"
            class="play"
            class:on={playing === `pack:${p.id}`}
            title={playing === `pack:${p.id}` ? 'Stop' : `Hear ${p.name}`}
            onclick={() => hear(`pack:${p.id}`, p.id, null)}
          >
            {@render play(`pack:${p.id}`, p.name)}
          </button>
        </div>
      {/each}
    </div>
  </Row>
  <Row label="Sounds play on" desc="The speakers or headset for Yap's sounds" setting="chime-output">
    <Select bind:value={cfg.outputDevice} options={outputOptions} ariaLabel="Sounds play on" wide disabled={!cfg.soundEnabled} />
  </Row>
  {#each SOUND_MOMENTS as m (m.field)}
    <Row label={m.label} desc={m.desc} setting={m.setting}>
      <span class="moment">
        <button
          type="button"
          class="play small"
          class:on={playing === `moment:${m.field}`}
          title={playing === `moment:${m.field}` ? 'Stop' : 'Hear it'}
          onclick={() => hear(`moment:${m.field}`, pack, m.cues)}
        >
          {@render play(`moment:${m.field}`, m.label)}
        </button>
        <Toggle bind:checked={cfg[m.field]} ariaLabel={m.label} disabled={!cfg.soundEnabled} />
      </span>
    </Row>
  {/each}
</Group>

<style>
  .packs {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .pack {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .pack.sel {
    border-color: var(--yap-fg-62);
  }
  /* Sounds off: the choice greyed out, its ▶ still there to listen first. */
  .pack.off .pick {
    opacity: 0.55;
  }
  .pick {
    display: flex;
    flex: 1;
    min-width: 0;
    align-items: baseline;
    gap: 8px;
    padding: 9px 4px 9px 12px;
    border: 0;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .pick:disabled {
    cursor: default;
  }
  .pick:focus-visible,
  .play:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
    border-radius: var(--yap-r);
  }
  .dot {
    flex: none;
    align-self: center;
    width: 12px;
    height: 12px;
    border: 1.5px solid var(--yap-fg-62);
    border-radius: 50%;
    box-sizing: border-box;
  }
  .sel .dot {
    border: 4px solid var(--yap-fg);
  }
  .name {
    flex: none;
    font-size: 13px;
    font-weight: 600;
  }
  .pdesc {
    min-width: 0;
    overflow: hidden;
    font-size: 12px;
    color: var(--yap-muted);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .play {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    cursor: pointer;
  }
  .play:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .play.on {
    border-color: var(--yap-fg-62);
  }
  .play:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .play svg {
    width: 10px;
    height: 10px;
  }
  .play.small {
    width: 26px;
    height: 26px;
  }
  .moment {
    display: inline-flex;
    align-items: center;
    gap: 12px;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
