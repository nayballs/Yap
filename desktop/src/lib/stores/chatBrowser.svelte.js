// The browser button in the title bar (ControlPanel, while Chat shows; as
// Claude Code's, top right) and Chat's panel (ChatView): each press counts
// (`presses`), and ChatView opens the panel's page tabs — the last page
// shown, else a new tab — or closes the panel when a page tab shows. Chat
// says whether one does (`open`), for the button's pressed look.
export const chatBrowser = $state({ presses: 0, open: false });
