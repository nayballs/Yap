// A question for the Chat view, handed over by another view (the Meetings
// view's Ask bar): ChatView starts a new chat with it and clears `pending`.
// `{ text, scope }`; scope 'meetings' grounds the answer in meeting notes.
// `{ text, send, dictated: true }`: a question dictated for the chat that
// arrived after you left it (ControlPanel): into the box, sent if `send`.
export const chatRequest = $state({ pending: null });
