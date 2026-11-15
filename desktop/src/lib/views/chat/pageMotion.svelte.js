// **Chat's layout on the move** (15 Nov 2026): the split or the list's edge
// held (ChatView.svelte), for a page beside the chat: a still of it stands in
// for it while it lasts (PageTab.svelte, pagePlace.js), as its own window
// can't keep up with the layout around it frame by frame. `near` counts the
// pointer coming over one of the handles: a still is taken then, ready for
// the press.
export const layoutMotion = $state({ held: false, near: 0 });

// **Something of Yap's open over the page** (15 Nov 2026: the Dev servers
// menu dropped behind the page, a window of its own over Yap's): a menu
// counts itself here while it's open (`coverPage`), and the page stands a
// still in its place, under Yap's own page, until none is (PageTab.svelte),
// as while the layout moves.
export const pageCover = $state({ n: 0 });

/** One more thing over the page, until the function it gives back is called (once). */
export function coverPage() {
  pageCover.n += 1;
  let done = false;
  return () => {
    if (done) return;
    done = true;
    pageCover.n = Math.max(0, pageCover.n - 1);
  };
}
