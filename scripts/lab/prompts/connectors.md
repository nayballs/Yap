You are testing your own tools for the people who build this app. This is a test run in a test copy of the app: nothing here belongs to a real person, and your report goes to the developers.

The family to test: **connectors** — tools from other services the user has added to the app (their names usually have two underscores in the middle, like service__action), and any tool for finding or searching such tools.

How to test:

1. First list every connector tool and every tool-finding tool you have, with each of its parameters, exactly as you were given them, grouped by connector. If there are none at all, say so plainly, and test only what's there (searching for tools, for example).
2. For each connector, test its tools that only read or look things up, each parameter at least once, with neutral, made-up inputs (search for "lighthouse", "unit conversion", "example"). A tool that would create, change, send or delete something: call it at most once, with obviously made-up content, so you can see whether the app asks the user first — then note what the card asked.
3. Try the edges too: an empty search, a search with no results, a tool given a wrong parameter, and a tool name that doesn't exist.
4. When a tool refuses or fails, copy its message word for word. In this test run a card that asks the user is answered "Don't" for you: note that and carry on.
5. Stay safe: never connect, sign in or authorise anything; no messages sent, no purchases, nothing with personal details.

Then write your report, in this order:

## Summary
Two or three sentences: how much of the family works.

## What fails
Each problem, most important first: the tool, what you asked (its arguments), and the exact error or wrong result.

## What's inconsistent
Places where tools disagree with each other or with their own descriptions (a parameter that's ignored, a result that doesn't match its description, a card that asked when it shouldn't have, or didn't when it should).

## What works
A short list, tool by tool.

## Suggested fixes
Most important first, each one or two sentences.

## Coverage
A table: tool | parameters tried | result (works / fails / refused / not available).
