// The Summary button in the title bar (ControlPanel, while a Code chat shows;
// as the Claude app's Code tab has one at the top right) and Chat's panel
// (ChatView): each press counts (`presses`) and ChatView shows the Code
// chat's summary tab, or closes the panel while it shows. Chat says whether a
// Code chat shows (`shown`: the button is there) and whether the summary
// does (`open`: its pressed look).
export const chatSummary = $state({ presses: 0, open: false, shown: false });
