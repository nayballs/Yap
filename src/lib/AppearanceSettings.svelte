<script>
  // Settings → General → Appearance (after Claude's own Appearance settings,
  // in free fonts): the font and size of the reading text — Chat's messages,
  // notes and their Enhanced view, the meeting notepad's My thoughts /
  // Transcript / Summary and its Ask answers — and Motion for every Yap
  // window. lib/appearance.js applies them (Settings' copy at once, every
  // other window after the auto-save); app.css has the tokens.
  //
  //   cfg: Settings' config $state (`readingFont`, `readingSize`, `motion`).
  //        Auto-save does the saving.
  import { onMount } from 'svelte';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Select from './ui/Select.svelte';
  import Segmented from './ui/Segmented.svelte';
  import { READING_FONTS, READING_FONT_DESC, READING_SIZES, MOTION_OPTIONS } from './appearance.js';

  let { cfg } = $props();

  // Each font's name in itself, in the list and in the closed box (Claude's
  // picker does the same).
  const FONT_OPTIONS = READING_FONTS.map((f) => ({ value: f.value, label: f.label, style: `font-family: ${f.family}` }));

  // What "System" means right now: Windows' "Animation effects" (Settings →
  // Accessibility → Visual effects), as WebView2 reports it.
  let osReduced = $state(false);
  onMount(() => {
    const mq = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    if (!mq) return;
    osReduced = mq.matches;
    const on = () => (osReduced = mq.matches);
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  });
  const motionDesc = $derived(
    cfg.motion === 'reduced'
      ? 'No sliding, fading or pulsing in any Yap window. Spinners and countdowns stay.'
      : osReduced
        ? 'Follows Windows’ “Animation effects”, which is off: motion is reduced'
        : 'Follows Windows’ “Animation effects”, which is on'
  );
</script>

<Group title="Appearance" id="settings-appearance">
  <Row label="Reading font" desc={READING_FONT_DESC[cfg.readingFont] ?? READING_FONT_DESC.default} setting="reading-font">
    <Select bind:value={cfg.readingFont} options={FONT_OPTIONS} ariaLabel="Reading font" />
  </Row>
  <Row label="Text size" desc="For chats, notes and meeting notes" setting="text-size">
    <Segmented bind:value={cfg.readingSize} options={READING_SIZES} label="Text size" />
  </Row>
  <Row>
    <!-- What chats and notes will look like, in the reading font and size. -->
    <figure class="sample" aria-label="Preview of the reading text">
      <p>
        Priya will send the revised budget to finance by <strong>Friday</strong>, and the offsite moves to
        the 14th. <em>Catering still needs an owner.</em>
      </p>
      <figcaption>Chats, notes and meeting notes read like this. Buttons and menus keep Yap’s font.</figcaption>
    </figure>
  </Row>
  <Row label="Motion" desc={motionDesc} setting="motion">
    <Segmented bind:value={cfg.motion} options={MOTION_OPTIONS} label="Motion" />
  </Row>
</Group>

<style>
  .sample {
    margin: 0;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
  }
  .sample p {
    margin: 0;
    color: var(--yap-fg);
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: calc(1.7 * var(--yap-reading-leading));
  }
  .sample figcaption {
    margin-top: 6px;
    color: var(--yap-muted);
    font-size: 12px;
    line-height: 1.5;
  }
</style>
