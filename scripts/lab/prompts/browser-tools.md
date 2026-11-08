You are testing your own tools for the people who build this app. This is a test run in a test copy of the app: nothing here belongs to a real person, and your report goes to the developers.

The family to test: **the pages beside the chat and the web** — every tool you have for searching the web, bringing up a page beside the chat, reading it, looking at it, finding words on it, scrolling, waiting, zooming, working its tabs, running several steps at once, and (if you have them) clicking, typing, pressing keys, hovering and filling fields; plus the tools that find, look at and show pictures.

How to test:

1. First list every tool of this family you have, with each of its parameters, exactly as you were given them. If a tool you'd expect isn't there, say so.
2. Test each tool, and each parameter of it at least once, on safe public pages only: https://example.com/, https://en.wikipedia.org/ (for example https://en.wikipedia.org/wiki/Ben_Nevis and https://en.wikipedia.org/wiki/Lighthouse), and https://developer.mozilla.org/ (for example https://developer.mozilla.org/en-US/docs/Web/HTML/Element/table). Use the web search to find a page as well as going to one by its address.
3. Try the edges too: a page that has more than one screen of text (does reading or scrolling reach the rest?), a second and a third tab (switching, listing, closing), a word that isn't on the page, a link followed by a click, a site's own search box (typing a word and pressing Enter), a picture on a page, an address with a typo, and going back.
4. When a tool refuses or fails, copy its message word for word. If a card asks the user first, note what it asked: in this test run the card is answered "Don't" for you, so note that too and carry on.
5. Stay safe: no sign-ins, no purchases, no bookings, no messages sent, and no forms with personal details (a site's own search box is fine). Never type anything that looks like a name, an email address, a password or a card number.

Then write your report, in this order:

## Summary
Two or three sentences: how much of the family works.

## What fails
Each problem, most important first: the tool, what you asked (its arguments), and the exact error or wrong result.

## What's inconsistent
Places where tools disagree with each other or with their own descriptions (a parameter that's ignored, a result that doesn't match what you saw, a refusal that doesn't fit the rule it gives).

## What works
A short list, tool by tool.

## Suggested fixes
Most important first, each one or two sentences.

## Coverage
A table: tool | parameters tried | result (works / fails / refused / not available).
