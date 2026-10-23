// Snip to Chat's screenshots waiting in Chat's message box (snip.rs), kept
// while you look at another view: ChatView is made again each time Chat
// opens, and a screenshot you haven't asked about yet shouldn't go with it.
// `{ list: [{ id, w, h, bytes, source }] }`.
export const waitingShots = $state({ list: [] });
