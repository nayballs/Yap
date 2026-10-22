<script>
  // Settings → Speech-to-Text → Runs on (stt_speed.rs): where the speech
  // model runs — the graphics card (Parakeet and the other ONNX models
  // through DirectML, Whisper through Vulkan) or the processor. Automatic:
  // whichever Yap measured fastest for this model on this PC (once, in the
  // background); or chosen. "Runs on: processor (fastest on this PC, 150 ms)".
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

  let choice = $state(cfg.sttDevice || 'auto');
  $effect(() => {
    choice = cfg.sttDevice || 'auto';
  });
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
  const ms = (r) => (r?.warmMs != null ? `${r.warmMs} ms` : null);

  // The line under "Runs on".
  const line = $derived.by(() => {
    if (!status) return '';
    const where = NAME[status.device] || 'graphics card';
    if (measuring) return `Runs on: ${where}. Measuring which is fastest on this PC…`;
    if (!status.hasGpu) return `Runs on: processor (no graphics card found)`;
    const setting = cfg.sttDevice || 'auto';
    if (setting === 'auto') {
      if (!measured) return `Runs on: ${where} (not measured on this PC yet)`;
      const best = ms(run(measured.pick));
      return `Runs on: ${NAME[measured.pick]} (fastest on this PC${best ? `, ${best}` : ''})`;
    }
    const chosen = `Runs on: ${where} (your choice)`;
    if (!measured || measured.pick === setting) return chosen;
    const faster = ms(run(measured.pick));
    return `${chosen}. The ${NAME[measured.pick]} was faster here${faster ? ` (${faster})` : ''}`;
  });

  // What was measured, for the ⓘ.
  const hint = $derived.by(() => {
    if (!measured) return 'Yap times each speech model on this PC once, in the background, and uses the faster of the two.';
    const parts = (measured.runs || [])
      .map((r) => {
        const name = NAME[r.device];
        if (r.skipped) return `${name}: not tried (Whisper is far slower there)`;
        if (r.error) return `${name}: didn't run`;
        if (r.warmMs == null) return `${name}: much slower (${r.firstMs} ms at first)`;
        return `${name}: ${r.warmMs} ms`;
      })
      .join(', ');
    const secs = Math.round((measured.clipMs || 0) / 100) / 10;
    const when = new Date((measured.at || 0) * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
    return `${status?.modelName || 'This model'} on a ${secs}-second clip, ${when}: ${parts}.`;
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
