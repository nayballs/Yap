// **Chat's status dots, live** (core/chatStatus.js has the rules): the
// state of every conversation in Chat's list — working, needs you, new — for
// the list's rows, its folded workspace groups, the Chat | Code switch and
// the menu rail's Chat icon. Started once per window by ControlPanel
// (`initChatStatus`), so it's kept while another view shows.
//
// What it follows:
// - **the conversations answering** (Rust's list, `chat_answering` then
//   `yap-chat-answering`: every turn, typed, by voice, a phone's or a job's
//   report);
// - **the jobs and their prompts** (stores/jobs.svelte.js);
// - **the ask-first cards waiting** in a turn's stream (`kind: "confirm"`,
//   told by Chat's state: `cardAsked`, `turnOver`);
// - **Chat's list** (`chats_list`: each conversation's last answer and the
//   person's last message), read again as turns end and reports land;
// - **the conversation on screen** (`shown`, ChatView) and **the last answer
//   seen** in each (`markSeen`), kept in this window's storage
//   (`yap.chat.seen`), never chats.json.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { untrack } from 'svelte';
import { jobs } from './jobs.svelte.js';
import {
  SEEN_KEY,
  WORKING_FLOOR_MS,
  workingNow,
  trackWorking,
  nextChange,
  chatStatuses,
  markSeen as seenUpTo,
  seedSeen,
  pruneSeen,
  readSeen,
  writeSeen,
} from '../core/chatStatus.js';

export const chatStatus = $state({
  /** conversation id → `{state, why}` (none: idle and seen). */
  byId: {},
  /** Chat's list as the dots read it (`{id, answerAt, askedAt}`), or null until read. */
  conversations: null,
  /** The conversation on screen (Chat showing it), or null. */
  shown: null,
});

/** conversation → when this window saw it start answering (ms). */
let answering = {};
let answeringSeq = -1;
/** conversation → the ask-first cards waiting there (ids). */
let confirms = {};
/** job id → when this window first saw it at work (ms). */
let jobSince = {};
let jobsRead = false;
let spells = {};
let seen = null;
let started = false;
let timer = null;
let listTimer = null;
/** `chatStatus.byId` as last given, to compare. */
let shownText = '{}';

function loadSeen() {
  try {
    return readSeen(localStorage.getItem(SEEN_KEY));
  } catch {
    return null;
  }
}

function saveSeen() {
  try {
    localStorage.setItem(SEEN_KEY, writeSeen(seen));
  } catch {
    /* private storage refused: seen for this session only */
  }
}

/** Everything again, from the inputs as they are now; the clock's next change scheduled. */
function update() {
  const now = Date.now();
  // The jobs at work: since when this window saw them (already at work
  // when it opened: since before the floor, so they show at once).
  const live = new Set();
  for (const j of jobs.list) {
    if (j.status !== 'queued' && j.status !== 'running') continue;
    live.add(j.id);
    if (!jobSince[j.id]) jobSince[j.id] = jobsRead ? now : now - WORKING_FLOOR_MS;
  }
  for (const id of Object.keys(jobSince)) if (!live.has(id)) delete jobSince[id];
  if (jobs.loaded) jobsRead = true;
  spells = trackWorking(spells, workingNow({ answering, jobs: jobs.list, jobSince, now }), now);
  const counts = Object.fromEntries(Object.entries(confirms).map(([k, v]) => [k, v.length]));
  const next = chatStatuses({
    conversations: chatStatus.conversations,
    jobs: jobs.list,
    approvals: jobs.approvals,
    confirms: counts,
    spells,
    seen: seen || {},
    shown: chatStatus.shown,
    now,
  });
  // (A job's every step comes through here: the dots only hear a real change.)
  const text = JSON.stringify(next);
  if (text !== shownText) {
    shownText = text;
    chatStatus.byId = next;
  }
  clearTimeout(timer);
  const at = nextChange(spells, now);
  if (at != null) timer = setTimeout(update, Math.max(16, at - now));
}

/** Chat's list as Chat read it (chatState's `refreshList`), or read here. */
export function tookList(list) {
  if (!Array.isArray(list)) return;
  const conversations = list.map((c) => ({ id: c.id, answerAt: Number(c.answerAt ?? -1), askedAt: Number(c.askedAt ?? -1) }));
  // The first list ever read here: what's in it counts as seen.
  if (seen == null) {
    seen = seedSeen(conversations);
    saveSeen();
  } else {
    const pruned = pruneSeen(seen, conversations);
    if (pruned !== seen) {
      seen = pruned;
      saveSeen();
    }
  }
  // (Never back: a read that left before an answer was saved, landing after
  // one the page already has, keeps it — so a seen answer never reads as new.)
  const before = new Map((chatStatus.conversations || []).map((c) => [String(c.id), c]));
  chatStatus.conversations = conversations.map((c) => {
    const was = before.get(String(c.id));
    return was ? { ...c, answerAt: Math.max(c.answerAt, was.answerAt), askedAt: Math.max(c.askedAt, was.askedAt) } : c;
  });
  if (chatStatus.shown != null) markSeen(chatStatus.shown);
  update();
}

async function readList() {
  try {
    tookList(await invoke('chats_list'));
  } catch {
    /* keep the last */
  }
}

/** Read Chat's list again in a moment (several reasons at once: one read). */
export function refreshListSoon(ms = 120) {
  clearTimeout(listTimer);
  listTimer = setTimeout(readList, ms);
}

/**
 * **Seen up to `at`** in conversation `id` (its last answer on screen;
 * none given: the list's). Its "new" goes.
 */
export function markSeen(id, at = null) {
  if (id == null) return;
  const conv = chatStatus.conversations?.find((c) => String(c.id) === String(id));
  const upTo = Number.isInteger(at) ? Math.max(at, conv?.answerAt ?? -1) : (conv?.answerAt ?? -1);
  const next = seenUpTo(seen || {}, id, upTo);
  if (next === seen) return;
  seen = next;
  saveSeen();
  // The list catches up with an answer the page has (so it never reads as unseen).
  if (conv && upTo > conv.answerAt) chatStatus.conversations = chatStatus.conversations.map((c) => (c === conv ? { ...c, answerAt: upTo } : c));
  update();
}

/** **The conversation on screen** (ChatView; null: Chat isn't showing one). */
export function setShown(id) {
  const next = id ?? null;
  if (chatStatus.shown === next) return;
  const was = chatStatus.shown;
  chatStatus.shown = next;
  // Leaving one: what it had is seen.
  if (was != null) markSeen(was);
  if (next != null) markSeen(next);
  update();
}

/** An ask-first card in a turn's stream (`kind: "confirm"`): waiting while `state` is "asking". */
export function cardAsked(conversation, card) {
  if (conversation == null || card?.id == null) return;
  const k = String(conversation);
  const have = confirms[k] || [];
  const waiting = card.state === 'asking';
  if (waiting === have.includes(card.id)) return;
  confirms = { ...confirms, [k]: waiting ? [...have, card.id] : have.filter((c) => c !== card.id) };
  if (!confirms[k].length) delete confirms[k];
  update();
}

/** A turn there is over: no card of its waits any more. */
export function turnOver(conversation) {
  if (conversation == null || !confirms[String(conversation)]) return;
  const { [String(conversation)]: _gone, ...rest } = confirms;
  confirms = rest;
  update();
}

/** Rust's list of the conversations answering (`{conversations, seq}`). */
function onAnswering(p, first = false) {
  const seq = Number(p?.seq);
  if (Number.isFinite(seq) && seq < answeringSeq) return;
  if (Number.isFinite(seq)) answeringSeq = seq;
  const now = Date.now();
  const ids = (p?.conversations || []).map(String);
  const next = {};
  // (Answering when this window opened: since before the floor, shown at once.)
  for (const id of ids) next[id] = answering[id] ?? (first ? now - WORKING_FLOOR_MS : now);
  const ended = Object.keys(answering).some((id) => !next[id]);
  const begun = ids.some((id) => !answering[id]);
  answering = next;
  // A turn ended (its answer saved) or began (the person's question saved): the list again.
  if (ended || begun) refreshListSoon();
  update();
}

/** Load once per window and follow the app's events. */
export function initChatStatus() {
  if (started) return;
  started = true;
  seen = loadSeen();
  invoke('chat_answering')
    .then((p) => onAnswering(p, true))
    .catch(() => {});
  readList();
  listen('yap-chat-answering', (e) => onAnswering(e.payload)).catch(() => {});
  listen('yap-job-report', () => refreshListSoon()).catch(() => {});
  listen('yap-assistant-turn', (e) => {
    if (['answered', 'failed', 'superseded'].includes(e.payload?.phase)) refreshListSoon();
  }).catch(() => {});
  // The jobs and their prompts as they change.
  $effect.root(() => {
    $effect(() => {
      void jobs.list;
      void jobs.approvals;
      void jobs.loaded;
      untrack(update);
    });
  });
}
