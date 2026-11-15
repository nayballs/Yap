// **Chat's layout on the move** (15 Nov 2026): the split or the list's edge
// held (ChatView.svelte), for a page beside the chat: a still of it stands in
// for it while it lasts (PageTab.svelte, pagePlace.js), as its own window
// can't keep up with the layout around it frame by frame. `near` counts the
// pointer coming over one of the handles: a still is taken then, ready for
// the press.
export const layoutMotion = $state({ held: false, near: 0 });
