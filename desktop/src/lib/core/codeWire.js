// **Remote Control's wire** (Yap Link, 2026-11-08; docs/architecture/link.md,
// "Remote Control"): the Code side's messages between a paired phone and Yap
// on the PC, defined once. The PC's `desktop/src-tauri/src/link/wire.rs` is held
// to this file by its tests (every type both ways, the limits, and each
// example below parsed or written exactly), and the phone's page imports it
// as it is (build.rs `LINK_SHARED` serves it at `core/codeWire.js`), so the
// page and the PC can't drift. Pure: no imports.
//
// Text frames are JSON `{ type, … }` on Yap Link's WebSocket (protocol v1,
// `hello.v` 1). A PC that has these says `"code"`, `"code_act"` and
// `"approve"` in `ready.can`, and whether this phone may use them now in
// `ready.code` (`{on: true}`, or `{on: false, why}`). Every Code message is
// checked by the PC when it arrives: Remote Control on, this phone ticked,
// the workspace allowing it, the action one a phone may take (never Bypass
// permissions, never above Auto). Refused: `error {message}`, nothing done.
//
// Approvals need a fresh tap: `approve` (and `confirm`) only for a card the
// PC sent this phone, still waiting, with `shownMs` (how long the page had
// shown it) at least `MIN_SHOWN_MS`, from a trusted pointer event, never by
// voice. The first answer wins; the PC's own `approve_done` says who.

/** What a PC with Remote Control adds to `ready.can`. */
export const CODE_CAN = ['code', 'code_act', 'approve'];

/** The Code side's messages, phone → PC. */
export const PHONE_TO_PC = [
  'code_list',
  'code_new',
  'agent_open',
  'agent_close',
  'agent_log',
  'agent_diff',
  'agent_stop',
  'agent_continue',
  'agent_message',
  'agent_go_ahead',
  'agent_undo',
  'approve',
  'confirm',
  'set_mode',
];

/** …and PC → phone. */
export const PC_TO_PHONE = [
  'code_list',
  'code_status',
  'chat_message',
  'agent_snapshot',
  'agent_event',
  'agent_log',
  'agent_diff',
  'agent_resync',
  'approve_card',
  'approve_done',
  'confirm_card',
  'confirm_done',
  'mode',
  'report',
  'acted',
];

/** A card's buttons wake this long after it's shown; a tap sooner is refused by the PC. */
export const MIN_SHOWN_MS = 400;
/** The highest Mode a phone may pick, or a phone's turn runs at. */
export const PHONE_MAX_MODE = 'auto';
/** The Modes a phone may pick (`set_mode`), lowest first: never Bypass permissions. */
export const PHONE_MODES = ['plan', 'manual', 'acceptEdits', 'auto'];
/** What the PC keeps to: "Tell the agent…"'s characters, a snapshot's events, a diff page's lines, a live event's diff lines, a Code frame's bytes. */
export const LIMITS = { text: 4000, snapshotEvents: 200, diffLines: 400, liveDiffLines: 40, frame: 65536 };
/** `approve_done` / `confirm_done`'s `by`: this PC, this phone, another phone (with its `name`). */
export const BY = { pc: 'pc', you: 'you', phone: 'phone' };

/** Is Remote Control on for this phone (`ready`)? */
export const codeOn = (ready) => Array.isArray(ready?.can) && ready.can.includes('code') && ready?.code?.on === true;

/** Allow once / Deny on approval card `id`, `shownMs` after the page showed it. */
export const approve = (id, allow, shownMs) => ({ type: 'approve', id, allow: !!allow, shownMs: Math.max(0, Math.round(Number(shownMs) || 0)) });

/** Start / Don't on ask-first card `id`. */
export const confirm = (id, yes, shownMs) => ({ type: 'confirm', id, yes: !!yes, shownMs: Math.max(0, Math.round(Number(shownMs) || 0)) });

/** A Code chat's Mode (one of `PHONE_MODES`; the PC refuses anything higher). */
export const setMode = (chat, mode) => ({ type: 'set_mode', chat, mode: PHONE_MODES.includes(mode) ? mode : PHONE_MAX_MODE });

/**
 * One example of every message, each way: the PC's tests parse the phone's
 * and write the PC's exactly as here. Strict JSON between the markers (the
 * Rust test reads it as JSON); fictional, as everything in this repo.
 */
// prettier-ignore
export const EXAMPLES = /* wire:begin */ {
  "in": {
    "code_list": { "type": "code_list" },
    "code_new": { "type": "code_new", "workspace": "garden-planner" },
    "agent_open": { "type": "agent_open", "id": "1a2b3c4d" },
    "agent_close": { "type": "agent_close" },
    "agent_log": { "type": "agent_log", "id": "1a2b3c4d", "before": 41 },
    "agent_diff": { "type": "agent_diff", "id": "1a2b3c4d", "file": "notes.md", "turn": 1 },
    "agent_stop": { "type": "agent_stop", "id": "1a2b3c4d" },
    "agent_continue": { "type": "agent_continue", "id": "1a2b3c4d" },
    "agent_message": { "type": "agent_message", "id": "1a2b3c4d", "text": "Add a watering schedule too" },
    "agent_go_ahead": { "type": "agent_go_ahead", "id": "1a2b3c4d" },
    "agent_undo": { "type": "agent_undo", "id": "1a2b3c4d", "turn": 2, "sure": true },
    "approve": { "type": "approve", "id": 7, "allow": true, "shownMs": 2300 },
    "confirm": { "type": "confirm", "id": 3, "yes": true, "shownMs": 1800 },
    "set_mode": { "type": "set_mode", "chat": 12, "mode": "plan" }
  },
  "out": {
    "code_list": {
      "type": "code_list",
      "workspaces": [{ "name": "garden-planner", "path": "D:\\Projects\\garden-planner", "allowed": true, "usedTs": 1730000000 }],
      "chats": [{ "id": 12, "title": "Watering schedule", "ts": 1730000100, "workspace": "garden-planner" }],
      "agents": [
        {
          "id": "1a2b3c4d",
          "conversation": 12,
          "folder": "garden-planner",
          "status": "running",
          "now": "Adding a watering schedule",
          "turnTitle": "Add a watering schedule",
          "startedTs": 1730000110,
          "endedTs": 0,
          "asks": false,
          "plan": false,
          "planAllowed": false,
          "messageId": "chat:12:0",
          "turnList": [{ "n": 1, "title": "Add a watering schedule", "status": "running", "messageId": "chat:12:0" }]
        }
      ]
    },
    "code_status": {
      "type": "code_status",
      "seq": 4,
      "answering": [12],
      "agents": [],
      "approvals": [{ "id": 7, "conversation": 12, "agent": "1a2b3c4d" }],
      "confirms": [{ "id": 3, "conversation": 12 }]
    },
    "chat_message": { "type": "chat_message", "conversation": 12, "message": { "role": "user", "text": "The agent in garden-planner: it's done.", "kind": "agentReport" } },
    "agent_snapshot": {
      "type": "agent_snapshot",
      "agent": { "id": "1a2b3c4d", "folder": "garden-planner", "status": "running" },
      "events": [{ "line": 1, "event": { "id": "1a2b3c4d", "status": "running", "folder": "garden-planner", "step": "Reading notes.md", "ts": 1730000111 } }],
      "more": false,
      "seq": 0
    },
    "agent_event": { "type": "agent_event", "id": "1a2b3c4d", "seq": 1, "line": 2, "event": { "id": "1a2b3c4d", "status": "running", "folder": "garden-planner", "now": "Adding a watering schedule", "ts": 1730000112 } },
    "agent_log": { "type": "agent_log", "id": "1a2b3c4d", "events": [], "more": false },
    "agent_diff": { "type": "agent_diff", "id": "1a2b3c4d", "file": "notes.md", "lines": ["-Draft", "+Final"], "more": false, "added": 1, "removed": 1 },
    "agent_resync": { "type": "agent_resync", "id": "1a2b3c4d" },
    "approve_card": {
      "type": "approve_card",
      "card": { "id": 7, "run": "1a2b3c4d", "conversation": 12, "origin": "typed", "tool": "Bash", "summary": "Run a command: npm test", "detail": "npm test", "label": "garden-planner", "askedTs": 1730000120 },
      "expiresTs": 1730000720
    },
    "approve_done": { "type": "approve_done", "id": 7, "outcome": "allowed", "by": "phone", "name": "Kitchen tablet" },
    "confirm_card": {
      "type": "confirm_card",
      "id": 3,
      "conversation": 12,
      "agent": { "folder": "garden-planner", "path": "D:\\Projects\\garden-planner", "mode": "auto", "modeLabel": "Auto", "brief": "Add a watering schedule", "newFolder": false, "web": true, "continues": false },
      "expiresTs": 1730000240
    },
    "confirm_done": { "type": "confirm_done", "id": 3, "outcome": "allowed", "by": "you" },
    "mode": { "type": "mode", "chat": 12, "mode": "plan", "label": "Plan", "cap": "manual" },
    "report": { "type": "report", "conversation": 12, "agent": "1a2b3c4d", "turn": 1, "status": "done" },
    "acted": { "type": "acted", "action": "stop", "id": "1a2b3c4d", "message": "Stopping the agent in garden-planner." }
  }
} /* wire:end */;
