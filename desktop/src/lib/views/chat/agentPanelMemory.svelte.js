// **What the Agent panel remembers** while the window stays open
// (AgentPanel.svelte): which rows the person opened (row key, "<agent>:<turn>"
// → true) and which groups they folded ("<agent>:<group>" → true), so closing
// an agent's tab and opening it again — or opening another agent's and coming
// back — finds it as they left it. Rows start folded, the running one too;
// a pill or a report card opens its own. Never saved: a new window starts
// afresh.
export const panelMemory = $state({ open: {}, folded: {} });
