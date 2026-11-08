// Money as Yap's lines say it: what an answer (core/aiModel.js `paidLine`)
// or an agent (core/agents.js `costLine`) cost, or would have cost on the API.
// Pure, with no imports, so a page's pure helpers and the e2e specs can use it.

/** Millionths of a dollar as an answer's line says them: "$0.004", "$0.0004", "under $0.0001". */
export function dollars(micros) {
  const usd = (Number(micros) || 0) / 1_000_000;
  if (usd < 0.0001) return 'under $0.0001';
  if (usd < 0.01) return `$${usd.toFixed(4).replace(/0+$/, '')}`;
  return `$${usd.toFixed(2)}`;
}
