// A question for the Chat view, handed over by another view (the Meetings
// view's Ask bar): ChatView starts a new chat with it and clears `pending`.
// `{ text, scope }`; scope 'meetings' grounds the answer in meeting notes.
export const chatRequest = $state({ pending: null });
