// The on-device model's context, in words: what LocalContext.svelte (the
// Chat model picker's On this PC pane, Settings → Language Models → Local),
// the model rows ("up to 256K") and the model chip's tooltip say.
// ai/local_llm.rs `context_view` sends the numbers (local_llm_status().context).

/**
 * "8K" (8,192), "128K" (131,072 or 128,000), "40K" (40,960), "200K", "1M":
 * a round thousand as itself, else a multiple of 1,024 as one.
 */
export function fmtTokens(n) {
  const v = Number(n) || 0;
  if (!v) return '';
  if (v >= 1_048_576 && v % 1_048_576 === 0) return `${v / 1_048_576}M`;
  if (v >= 1_000_000) return `${Math.round(v / 100_000) / 10}M`.replace('.0M', 'M');
  if (v % 1000 === 0) return `${v / 1000}K`;
  if (v % 1024 === 0) return `${v / 1024}K`;
  if (v >= 1000) return `${Math.round(v / 1000)}K`;
  return String(v);
}

/** "3.7 GB" from MiB. */
export function fmtGb(mib) {
  const gb = (Number(mib) || 0) / 1024;
  return `${gb < 10 ? gb.toFixed(1) : Math.round(gb)} GB`;
}

/** "up to 256K" for a model trained for `max` tokens ('' unknown). */
export function upTo(max) {
  return max ? `up to ${fmtTokens(max)}` : '';
}

/** "on the graphics card" / "of memory". */
export function where(ctx) {
  return ctx?.onCard ? 'on the graphics card' : 'of memory';
}

/** One choice in the select: "32K tokens · ≈ 4.6 GB". */
export function optionLabel(o) {
  const mem = o.estimateMib ? ` · ≈ ${fmtGb(o.estimateMib)}` : '';
  return `${fmtTokens(o.tokens)} tokens${mem}${o.over ? ' · may not fit' : ''}`;
}

/** The running model's own log says what it takes at `tokens`. */
export function measured(ctx, tokens = ctx?.tokens) {
  return !!(ctx?.usedMib && ctx.serving === tokens && !ctx.pending);
}

/**
 * What the model takes at the context shown: what the running model's log
 * says when it runs at that context ("Uses 3.8 GB on the graphics card"),
 * else the estimate ("≈ 4.6 GB on the graphics card"); '' when unknown.
 */
export function memoryLine(ctx, tokens = ctx?.tokens) {
  if (!ctx) return '';
  if (measured(ctx, tokens)) return `Uses ${fmtGb(ctx.usedMib)} ${where(ctx)}`;
  const o = (ctx.options || []).find((x) => x.tokens === tokens);
  return o?.estimateMib ? `≈ ${fmtGb(o.estimateMib)} ${where(ctx)}` : '';
}

/** The warning for a choice that won't fit on the card, or ''. */
export function overLine(ctx, tokens = ctx?.tokens) {
  const o = (ctx?.options || []).find((x) => x.tokens === tokens);
  if (!o?.over) return '';
  const room = ctx.roomMib ? ` (about ${fmtGb(ctx.roomMib)} to spare)` : '';
  return `More than fits on your graphics card${room}: part of it would run on the processor, slowly.`;
}

/** The control's state line while a change waits or the model restarts. */
export function pendingLine(ctx) {
  if (!ctx?.pending) return '';
  return 'Restarting the model with the new context… (after any dictation that’s going on)';
}

/** "Context: 8K tokens" for the model chip's tooltip ('' without one). */
export function chipContextLine(tokens) {
  return tokens ? `Context: ${fmtTokens(tokens)} tokens` : '';
}

/** A provider's context, as a row's info: "1M context" ('' unknown). */
export function contextInfo(n) {
  return n ? `${fmtTokens(n)} context` : '';
}
