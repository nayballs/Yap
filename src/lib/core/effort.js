// Effort (src-tauri/src/ai/effort.rs): how hard a cloud model reasons before
// it answers, chosen per model in Chat's model picker and Settings →
// Language Models → Chat. Rust says which levels a model takes (`info`:
// `{ levels, default, chosen, fast }`, from ai_model_status / effort_info);
// this names them as the Claude Code app does (xhigh is "Extra") and says
// what each one does.

/** The level's name in the picker and on the chip. */
export const EFFORT_LABELS = { low: 'Low', medium: 'Medium', high: 'High', xhigh: 'Extra', max: 'Max' };

const WHAT = {
  low: 'quickest, for simple questions',
  medium: 'balanced, for everyday questions',
  high: 'thinks more, for detailed work',
  xhigh: 'thinks longer, for hard problems',
  max: 'thinks the longest: slow, and it can use a lot of your plan',
};

/** The picker's and Settings' one line on what effort is. */
export const EFFORT_NOTE = 'Higher effort thinks longer: more thorough answers, but slower, and it uses more of your plan.';

/** Fast mode's line (Claude Opus 5.5): it costs more, so it's off unless switched on. */
export const FAST_NOTE = 'Faster answers from Claude Opus 5.5, at twice the price per token. Off unless you switch it on.';

/** "Extra" for "xhigh"; '' for none. */
export function effortLabel(id) {
  return id ? EFFORT_LABELS[id] || id : '';
}

/** The level a model answers at: the one chosen, else its default ('' when its default isn't a level). */
export function effortNow(info) {
  return info?.chosen || info?.default || '';
}

/**
 * The choices, lowest first; "Default" first when the model's own default
 * isn't one of them (GPT-5.4 Mini doesn't reason unless asked): it sends
 * nothing.
 */
export function effortOptions(info) {
  const levels = (info?.levels || []).map((id) => ({ value: id, label: effortLabel(id) }));
  return info && !info.default ? [{ value: '', label: 'Default' }, ...levels] : levels;
}

/** The line under the choice: what the level in effect does ("Extra: thinks longer, for hard problems."). */
export function effortHint(info) {
  const now = effortNow(info);
  if (!now) return `Default: the model's own setting. ${EFFORT_NOTE}`;
  const said = `${effortLabel(now)}: ${WHAT[now] || ''}.`;
  return now === info.default ? `${said} The model's default.` : said;
}
