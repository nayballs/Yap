<script>
  // Settings → Language Models → Dictation cleanup → Usage today: Groq's
  // free tier as bars (Pulse-style), another provider's counts of Yap's own
  // cleanup calls, no limits on this PC or your own server. The day's
  // numbers are settings/languageModels.svelte.js's (`groq-usage`).
  //
  //   cfg: Settings' config $state.
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import { lm } from './languageModels.svelte.js';

  let { cfg } = $props();

  const usage = $derived(lm.usage);

  // Compact token formatter: <1000 → as-is, else "84.2k".
  function fmtK(n) {
    const v = Number(n) || 0;
    return v < 1000 ? String(v) : `${(v / 1000).toFixed(1)}k`;
  }
  // 0–100 percentage of a value against a cap (guards a zero/absent cap).
  function pctOf(value, cap) {
    const c = Number(cap) || 0;
    if (c <= 0) return 0;
    return Math.min(100, Math.round((Number(value) || 0) / c * 100));
  }
  // Pulse-style colour ramp: green < 70%, amber 70–90%, red ≥ 90%.
  function usageColor(pct) {
    if (pct >= 90) return 'var(--yap-danger)';
    if (pct >= 70) return 'var(--yap-warning)';
    return 'var(--yap-success)';
  }
</script>

<Group title="Usage today" id="settings-usage">
  <Row>
    {#snippet children()}
      {@const prov = usage.providers?.[cfg.ppProvider] ?? { tokens: 0, requests: 0 }}
      {#if cfg.ppProvider === 'ondevice'}
        <div class="usage">
          <div class="usage-raw">
            <span class="usage-label">Tokens today</span>
            <span class="usage-stat">{fmtK(prov.tokens)}</span>
          </div>
          <div class="usage-raw">
            <span class="usage-label">Cleanups today</span>
            <span class="usage-stat">{prov.requests}</span>
          </div>
          <p class="usage-caption">
            The on-device model: on this PC, free, no limits.
          </p>
        </div>
      {:else if cfg.ppProvider === 'local'}
        <div class="usage">
          <div class="usage-raw">
            <span class="usage-label">Tokens today</span>
            <span class="usage-stat">{fmtK(prov.tokens)}</span>
          </div>
          <div class="usage-raw">
            <span class="usage-label">Cleanups today</span>
            <span class="usage-stat">{prov.requests}</span>
          </div>
          <p class="usage-caption">
            Your own server: no usage limits.
          </p>
        </div>
      {:else if cfg.ppProvider === 'groq'}
        {@const tPct = pctOf(usage.tokens, usage.tokenCap)}
        {@const rPct = pctOf(usage.requests, usage.requestCap)}
        <div class="usage">
          <div class="usage-bar">
            <div class="usage-row">
              <span class="usage-label">Tokens today</span>
              <span class="usage-stat">
                {tPct}%&nbsp;·&nbsp;{fmtK(usage.tokens)} / {fmtK(usage.tokenCap)}
              </span>
            </div>
            <div class="track">
              <div class="value" style="width:{tPct}%;background:{usageColor(tPct)}"></div>
            </div>
          </div>
          <div class="usage-bar">
            <div class="usage-row">
              <span class="usage-label">Requests today</span>
              <span class="usage-stat">
                {rPct}%&nbsp;·&nbsp;{usage.requests} / {usage.requestCap}
              </span>
            </div>
            <div class="track">
              <div class="value" style="width:{rPct}%;background:{usageColor(rPct)}"></div>
            </div>
          </div>
          <p class="usage-caption">Resets at midnight UTC.</p>
          <p class="usage-fine">Token cap is the free-tier estimate; request count is exact from Groq.</p>
        </div>
      {:else}
        <div class="usage">
          <div class="usage-raw">
            <span class="usage-label">Tokens today</span>
            <span class="usage-stat">{fmtK(prov.tokens)}</span>
          </div>
          <div class="usage-raw">
            <span class="usage-label">Requests today</span>
            <span class="usage-stat">{prov.requests}</span>
          </div>
          <p class="usage-caption">
            This provider only — counts Yap's own cleanup calls. Resets at midnight UTC.
          </p>
        </div>
      {/if}
    {/snippet}
  </Row>
</Group>

<style>
  /* Usage meter (Pulse-style daily bars) */
  .usage {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .usage-bar {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .usage-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .usage-label {
    color: var(--yap-fg);
    font-size: 12.5px;
  }
  .usage-stat {
    color: var(--yap-muted);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .usage .track {
    width: 100%;
    height: 6px;
    background: var(--yap-raised);
    border-radius: 999px;
    overflow: hidden;
  }
  .usage .value {
    height: 100%;
    border-radius: 999px;
    transition: width 0.4s ease, background 0.4s ease;
  }
  .usage-raw {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .usage-caption {
    margin: 2px 0 0;
    color: var(--yap-muted);
    font-size: 12px;
  }
  .usage-fine {
    margin: 0;
    color: var(--yap-muted);
    font-size: 12px;
    line-height: 1.5;
  }
</style>
