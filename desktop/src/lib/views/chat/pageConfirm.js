// The words of Yap's question before acting on a page (chat/pages/act/
// confirm.rs `Card`), shared by its card in the thread (PageConfirm.svelte)
// and its page tab's strip (PageTab.svelte), so the two always say the same.
// `act`: press (a button or a link; an older card has none), enter (Enter
// in a field), open (go to a link's address), type (typed words, `text`),
// drag (what's dragged, onto `text`),
// agent (an agent about to start, chat/tools/agents.rs: `agent` {folder, path, mode,
// modeLabel, brief, newFolder, web}; Yap Code gives it a card of its own).

const WHY = {
  buy: 'It may buy something.',
  pay: 'It may take a payment.',
  book: 'It may make a booking.',
  'place an order': 'It may place an order.',
  cancel: 'It may cancel something for you.',
  confirm: 'It may confirm something for you.',
  subscribe: 'It may sign you up for something.',
  send: 'It may send something.',
  delete: 'It may delete something.',
  agree: 'It agrees to terms for you.',
  'sign in': 'It signs in to the site.',
};

/**
 * The question in parts: what Yap would do (`lead`), the words in quotes
 * (`quoted`), what follows them (`then`, may be empty) — "Yap wants to type
 * ‘…’ into ‘Email’", then " on site".
 */
export function questionParts(c) {
  switch (c?.act) {
    case 'type':
      return { lead: 'Yap wants to type', quoted: c.text || '', then: ` into ‘${c.label}’` };
    case 'open':
      return { lead: 'Yap wants to open', quoted: c.label, then: '' };
    case 'enter':
      return { lead: 'Yap wants to press Enter in', quoted: c.label, then: '' };
    case 'drag':
      // What's dragged, onto what (`text`).
      return { lead: 'Yap wants to drag', quoted: c.label, then: c.text ? ` onto ‘${c.text}’` : '' };
    case 'agent':
      // A follow-up for the chat's agent (its next turn), or a new agent.
      return { lead: c.agent?.continues ? 'Yap wants to tell the agent in' : 'Yap wants to send an agent to', quoted: c.label, then: c.agent?.modeLabel ? ` (${c.agent.modeLabel})` : '' };
    default:
      return { lead: 'Yap wants to press', quoted: c?.label || '', then: '' };
  }
}

/** Why it asks, in a line. */
export function whyOf(c) {
  if (c?.act === 'type') return 'Those words aren’t in what you said, nor on the page.';
  if (c?.act === 'agent') {
    if (c.agent?.newFolder) return 'No agent has worked in this folder before.';
    if (c.agent?.web) return 'This chat read the web, which could have put words in the agent’s brief.';
    return 'Claude Code would work on your files.';
  }
  return WHY[c?.does] || 'It may do something on the site for you.';
}

/** What a card says once answered (`state`: allowed, refused, timedOut). */
export function doneWords(c, state) {
  const verb = c?.act === 'type' ? 'type' : c?.act === 'open' ? 'open' : c?.act === 'agent' ? 'send' : c?.act === 'drag' ? 'drag' : 'press';
  if (state === 'allowed') return 'Allowed once.';
  if (state === 'timedOut') return `No answer in time, so Yap didn’t ${verb} it.`;
  return `Yap didn’t ${verb} it.`;
}
