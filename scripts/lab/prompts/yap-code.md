You are testing your own tools for the people who build this app. This is a test run in a test copy of the app: nothing here belongs to a real person, and your report goes to the developers.

The family to test: **agents** — the tools that send an agent to work in a folder on this computer, check on it, tell it more, and stop it. The workspace to use is the test's own scratch folder, **lab-sandbox**: a tiny made-up project (a README, a TODO list, a small JavaScript file of kitchen unit conversions and its tests). Nothing else.

How to test:

1. First list every agent tool you have, with each of its parameters, exactly as you were given them. If a tool you'd expect isn't there, say so.
2. Test each tool, and each parameter at least once, in lab-sandbox only:
   - an agent that only reads: ask it to read README.md and TODO.md and say what the project does;
   - an agent that changes something small: the first unticked item in TODO.md (add tablespoons to src/convert.js, with a test);
   - check on an agent while it works and after it's done;
   - give an agent a follow-up message (for example: "also tick the item in TODO.md");
   - stop an agent that's running;
   - try each mode the tools offer, if they offer more than one.
3. Try the edges too: a workspace name that doesn't exist, checking on an agent id that doesn't exist, a message to an agent that has finished, and stopping one that has already stopped.
4. When a tool refuses or fails, copy its message word for word. If a card asks the user first, note what it asked and how it was answered.
5. Stay safe: lab-sandbox only, no other folder; nothing that installs software, deletes the folder, or goes on the internet.

Then write your report, in this order:

## Summary
Two or three sentences: how much of the family works.

## What fails
Each problem, most important first: the tool, what you asked (its arguments), and the exact error or wrong result.

## What's inconsistent
Places where tools disagree with each other or with their own descriptions (a status that doesn't match what the agent did, a parameter that's ignored, a refusal that doesn't fit the rule it gives).

## What works
A short list, tool by tool.

## Suggested fixes
Most important first, each one or two sentences.

## Coverage
A table: tool | parameters tried | result (works / fails / refused / not available).
