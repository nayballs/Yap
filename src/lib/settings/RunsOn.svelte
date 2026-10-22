<script>
  // Settings → Speech-to-Text → Runs on (stt_speed.rs): where the speech
  // model runs — the graphics card (Parakeet and the other ONNX models
  // through DirectML, Whisper through Vulkan) or the processor. Automatic:
  // the graphics card, unless Yap measured the processor clearly faster for
  // this model on this PC at every dictation length (once, in the
  // background); or chosen. "Runs on: graphics card (180 ms for 11 s of
  // speech on this PC)".
  //
  //   cfg: Settings' config $state (auto-save does the saving; the old GPU
  //        switch follows: useGpu = not the processor).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Row from '../ui/Row.svelte';
  import Select from '../ui/Select.svelte';
  import Button from '../ui/Button.svelte';

  let { cfg } = $props();

  let status = $state(null);
  let asking = $state(false);

  const OPTIONS = [
    { value: 'auto', label: 'Automatic' },
    { value: 'gpu', label: 'Graphics card' },
    { value: 'cpu', label: 'Processor' },
  ];
  const NAME = { gpu: 'graphics card', cpu: 'processor' };

  function read() {
    invoke('stt_speed_status')
      .then((s) => (status = s))
      .catch(() => {});
  }

  onMount(() => {
    read();
    const offs = [
      listen('yap-stt-speed', (e) => {
        status = e.payload;
        asking = false;
      }),
      // A model switched or Runs on changed: what's measured and where it
      // runs follow.
      listen('yap-config-saved', () => setTimeout(read, 300)),
    ];
    return () => offs.forEach((p) => p.then((f) => f()));
  });

  // Follows the config (a reload, another window), and the Select writes it.
  let choice = $derived(cfg.sttDevice || 'auto');
  function onchange(v) {
    cfg.sttDevice = v;
    cfg.useGpu = v !== 'cpu';
  }

  async function measure() {
    asking = true;
    try {
      await invoke('stt_speed_measure');
    } catch {
      asking = false;
    }
  }

  const measuring = $derived(asking || !!status?.measuring);
  const measured = $derived(status?.measured || null);
  const run = (device) => measured?.runs?.find((r) => r.device === device) || null;
  const secs = (ms) => `${Math.round((ms || 0) / 1000)} s`;
  // The typical dictation length's warm time: "180 ms for 11 s of speech".
  const typical = (r) => {
    const t = r?.lengths?.[1];
    return t?.warmMs != null ? `${t.warmMs} ms for ${secs(t.clipMs)} of speech` : null;
  };

  // The line under "Runs on".
  const line = $derived.by(() => {
    if (!status) return '';
    const where = NAME[status.device] || 'graphics card';
    if (measuring) return `Runs on: ${where}. Measuring how fast each is on this PC…`;
    if (!status.hasGpu) return `Runs on: processor (no graphics card found)`;
    const setting = cfg.sttDevice || 'auto';
    if (setting === 'auto') {
      if (!measured) return `Runs on: ${where} (not measured on this PC yet)`;
      const time = typical(run(measured.pick));
      if (measured.pick === 'cpu') return `Runs on: processor (clearly faster on this PC${time ? `: ${time}` : ''})`;
      return `Runs on: ${NAME[measured.pick]}${time ? ` (${time} on this PC)` : ''}`;
    }
    const chosen = `Runs on: ${where} (your choice)`;
    if (!measured || measured.pick === setting) return chosen;
    const time = typical(run(measured.pick));
    return `${chosen}. Automatic picks the ${NAME[measured.pick]} here${time ? ` (${time})` : ''}`;
  });

  // What was measured, for the ⓘ.
  const RULE =
    'Automatic keeps the graphics card unless the processor takes at least 30% less time at every length. On the processor, dictation slows down while a meeting records or another app keeps it busy.';
  const hint = $derived.by(() => {
    if (!measured)
      return `Yap times each speech model on this PC once, in the background, at three dictation lengths. ${RULE}`;
    const parts = (measured.runs || [])
      .map((r) => {
        const name = NAME[r.device].replace(/^./, (c) => c.toUpperCase());
        if (r.skipped) return `${name}: not tried (Whisper is far slower there).`;
        if (r.error) return `${name}: didn't run.`;
        if (!r.lengths?.length) return `${name}: much slower (${r.firstMs} ms at first).`;
        const warm = r.lengths.map((t) => `${secs(t.clipMs)} of speech ${t.warmMs ?? '–'} ms`).join(', ');
        return `${name}: ${warm} (the first after loading ${r.firstMs} ms).`;
      })
      .join(' ');
    const when = new Date((measured.at || 0) * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
    return `${status?.modelName || 'This model'}, measured ${when}. ${parts} ${RULE}`;
  });
</script>

<Row label="Speed" desc={line} {hint} setting="runs-on">
  <div class="runs-on">
    <Select bind:value={choice} options={OPTIONS} ariaLabel="Runs on" {onchange} />
    <Button variant="ghost" size="sm" disabled={measuring || !status} onclick={measure}>
      {measuring ? 'Measuring…' : measured ? 'Measure again' : 'Measure now'}
    </Button>
  </div>
</Row>

<style>
  .runs-on {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
</style>
