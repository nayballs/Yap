// **A chat's chapters** (`mark_chapter`, chat/cards/chapters.rs, 15 Nov
// 2026): each kept on the answer whose turn started it; its divider goes
// above that turn's question (the person's message just before the answer).

/**
 * The chapters of `messages`, in order: `{ at, title, summary }`, `at` the
 * index of the question it starts at (the person's message before its answer,
 * past an agent's report marker — kept as a "user" message of kind
 * `agentReport` in a Code chat —, or the answer itself when nothing of the
 * person's comes before). Pure.
 */
export function chaptersOf(messages) {
  const out = [];
  (messages || []).forEach((m, i) => {
    const c = m?.role === 'assistant' ? m.chapter : null;
    if (!c?.title) return;
    let at = i;
    for (let j = i - 1; j >= 0; j--) {
      const prev = messages[j];
      if (prev?.kind === 'agentReport') continue;
      if (prev?.role === 'user') {
        at = j;
        break;
      }
      if (prev?.role === 'assistant') break;
    }
    // (One divider a place: the later call wins.)
    const same = out.findIndex((x) => x.at === at);
    const chapter = { at, title: String(c.title), summary: String(c.summary || '') };
    if (same >= 0) out[same] = chapter;
    else out.push(chapter);
  });
  return out;
}

/**
 * The chapter the reader is in: the last one whose divider's top (`tops`, the
 * dividers' offsets from the thread's top, in order) is at or above `scrollTop`
 * plus a little (`slack`) — or, scrolled to the end (`atEnd`), the last one in
 * view (above `scrollTop + view`), as a contents does at a page's foot; -1
 * before the first (the chat's start). Pure.
 */
export function currentChapter(tops, scrollTop, slack = 48, view = 0, atEnd = false) {
  const line = atEnd && view > 0 ? scrollTop + view - slack : scrollTop + slack;
  let at = -1;
  (tops || []).forEach((t, i) => {
    if (t <= line) at = i;
  });
  return at;
}
