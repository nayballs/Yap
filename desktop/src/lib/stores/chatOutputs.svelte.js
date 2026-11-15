// The Outputs button in the title bar (ControlPanel, while a chat is open in
// Chat; beside Summary and the browser) and Chat's panel (ChatView): each
// press counts (`presses`) and ChatView shows the chat's Outputs tab, or
// closes the panel while it shows. Chat says whether a chat is open
// (`shown`: the button is there) and whether Outputs shows (`open`: its
// pressed look).
export const chatOutputs = $state({ presses: 0, open: false, shown: false });
