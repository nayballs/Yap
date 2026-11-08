//! What a turn tells its model besides the history: the system prompt
//! and the question's context — the instructions for what this model may
//! do, the notes the eager RAG finds, the meetings' passages, memory,
//! About you, the calendar, the chat's documents, and how to answer.
//!
//! **Two parts, for the model's cache** (2026-10-22): the system prompt
//! holds what stays the same from turn to turn — the persona, the
//! instructions, About you without its clock — and everything that
//! changes with the question (the time, the notes the eager RAG found,
//! the meetings, earlier web answers, how to answer out loud) goes in
//! the question's own message (`chats::with_context`). A hybrid local
//! model (Qwen3.5) re-uses its cache only up to a checkpoint, so a
//! system prompt that changed every turn made every Chat request read
//! the whole conversation again (docs/model-evals.md); cloud providers'
//! prefix caches like a stable start too. That's for a model that calls
//! tools (Qwen3.5-4B, the cloud models); one that can't (Qwen2.5-1.5B,
//! dense: it re-reads only from the first change anyway) keeps it all in
//! the system prompt — with the notes next to the question it gave half
//! answers (4 of 10 turns, `notes-follow-ups`, 2026-10-22). OpenWhispr
//! runs BOTH eager RAG and the notes tools: the model gets relevant notes
//! up front AND can search for more.

use super::gates::Gates;
use super::history::Sizing;
use super::{Asked, Opened};
use crate::chat::tools::FirstStep;

/// What the notes the eager RAG found ([`rag_context`]) come under, in the
/// question's context (`chats::with_context`): OpenWhispr's framing line.
/// (Telling the model the notes "may not be what it's about" didn't keep
/// Qwen3.5-4B from answering a follow-up about a note that only shared
/// "time" with it — 0 of 6, `notes-long-chat`, 2026-10-22; the stricter
/// match, `tools::rag_notes`, did.)
const RAG_LEAD: &str = "Below are notes from the user's library that may be relevant. Reference them naturally if they help answer the question.";

/// Eager keyword-RAG (OpenWhispr `buildRAGContext`, keyword edition per the
/// ROADMAP's escalating plan): top-5 notes by the shared keyword scorer —
/// `strict`, only those that really match the question (`tools::rag_notes`:
/// for a question's context, since 22 Oct 2026, where a loose match pulled
/// follow-ups off the conversation); else the loose search, as before, in
/// the system prompt of a model without tools (Qwen2.5-1.5B answered less
/// well from one note than from five) — formatted exactly like theirs —
/// `<note id="N" title="T">\n{first 500 chars}\n</note>` joined by blank
/// lines.
fn rag_context(query: &str, strict: bool) -> String {
    const RAG_NOTE_LIMIT: usize = 5;
    const RAG_NOTE_SNIPPET_LENGTH: usize = 500;

    let notes = if strict {
        crate::chat::tools::rag_notes(query, RAG_NOTE_LIMIT)
    } else {
        crate::chat::tools::search_notes(query, RAG_NOTE_LIMIT)
    };
    notes
        .into_iter()
        .map(|(_, n)| {
            // Snippet from raw content (their choice); fall back to enhanced /
            // transcript for notes that are transcript-only.
            let source = if !n.content.trim().is_empty() {
                n.content.clone()
            } else if !n.enhanced_content.trim().is_empty() {
                n.enhanced_content.clone()
            } else {
                // Who said each line (a name the speaker map has, else
                // You / Them): "who fixed the form?" answers from it
                // (Qwen3.5-4B named the wrong person from the bare lines,
                // `evals/search/library.json`, 2026-10-25).
                n.transcript
                    .iter()
                    .filter(|s| s.is_talk() && !s.text.trim().is_empty())
                    .map(|s| format!("{}: {}", crate::speakers::prompt_label(&n.speakers, s), s.text.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let snippet: String = source.chars().take(RAG_NOTE_SNIPPET_LENGTH).collect();
            let title = if n.title.trim().is_empty() {
                "Untitled".to_string()
            } else {
                n.title.clone()
            };
            format!("<note id=\"{}\" title=\"{}\">\n{}\n</note>", n.id, title, snippet)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// **What a turn tells its model** besides the history ([`build`]): the
/// system prompt, the same from turn to turn where it can be (the model's
/// cache), and the question's context, what changes with the question
/// (`chats::with_context`) — or, for a model that can't call tools, all
/// of it in the system prompt.
pub(super) struct Prompt {
    pub(super) system: String,
    pub(super) context: Vec<String>,
    /// Where the line on earlier web answers goes for a model without tools
    /// (it depends on the history sent, `history.rs`).
    pub(super) note_at: usize,
    /// Where an older stretch's summary goes (`compact.rs`), when the history
    /// needs one: before the prompt's end, which small models heed most.
    pub(super) summary_at: usize,
    /// A model that calls tools: what changes with the question goes in its
    /// own message, not the system prompt.
    in_message: bool,
}

impl Prompt {
    /// The persona, the instructions for what the model may do (and the
    /// screen's line), and — for a model that calls tools — the clock first
    /// in the question's context. `memory`: memory's tools are offered (not
    /// in a Code chat).
    fn start(t: &Asked<'_>, memory: bool) -> Prompt {
        let g = t.gates;
        let system = head(&t.ep.persona, &g, &t.profile, memory);
        // Where the line on earlier web answers goes for a model without tools
        // (it depends on the history sent, chosen below).
        let note_at = system.len();
        let mut context: Vec<String> = Vec::new();
        if g.tools {
            context.push(crate::ai::about_you::clock_line(&t.profile, t.now));
        }
        Prompt { system, context, note_at, summary_at: note_at, in_message: g.tools }
    }

    /// A block that changes with the question: in the question's context
    /// for a model that calls tools, else at the system prompt's end, after
    /// a blank line.
    pub(super) fn add(&mut self, block: String) {
        if self.in_message {
            self.context.push(block);
        } else {
            self.system.push_str("\n\n");
            self.system.push_str(&block);
        }
    }

    /// A note about the history (earlier web answers, another model's):
    /// after the clock in the question's context, or where
    /// [`Prompt::note_at`] says in the system prompt. Two such notes: the
    /// later one first.
    pub(super) fn add_early(&mut self, note: String) {
        if self.in_message {
            // After the clock, as before.
            self.context.insert(1.min(self.context.len()), note);
        } else {
            self.system.insert_str(self.note_at, &format!("\n\n{note}"));
        }
    }

    /// The pages open beside the chat (chat/pages.rs): "this page" is the one
    /// in front, and browser_read reads it. Whether any is open.
    async fn tabs(&mut self, t: &Asked<'_>) -> bool {
        let mut tabs_open = false;
        if t.gates.pictures != crate::chat::pictures::Gate::Off {
            let tabs = crate::chat::pages::backend::tabs().await;
            if !tabs.is_empty() {
                tabs_open = true;
                self.context.push(crate::chat::pages::tabs_line(&tabs));
            }
        }
        tabs_open
    }

    /// **Asked across the meetings** (the Meetings view's Ask bar, a
    /// conversation started there): the passages that match, found on this
    /// PC — by meaning and keywords, within the dates the question names —
    /// numbered for the model to cite, each a source that opens its meeting
    /// at the line (search/ask.rs). Nothing for any other question.
    async fn across_meetings(&mut self, t: &Asked<'_>, doc_turn: &crate::chat::documents::Turn) -> Across {
        if !t.meetings {
            return Across::default();
        }
        // The page's line: "Searching your meetings" (or "Running /todos").
        if let Some(w) = t.watcher {
            crate::chat::activity::tell(w, crate::chat::activity::searching_meetings(t.skill.as_ref().map(|s| s.skill.command.as_str())));
        }
        // A Skill reads its own passages (the open tasks, a week of
        // meetings…) and adds its instructions (chat/skills.rs).
        let found = match &t.skill {
            Some(s) => Box::pin(crate::chat::skills::find(s)).await,
            None => Box::pin(crate::search::ask::find(&t.text, crate::search::ask::ASK_PASSAGES)).await,
        };
        let passages = crate::search::ask::sources(&found);
        let chip = match &t.skill {
            Some(s) => crate::chat::skills::chip(s, &found),
            None => crate::search::ask::chip(&found),
        };
        if let Some(w) = t.watcher {
            w.send(crate::chat::stream::Event::Chips { chips: vec![chip.clone()], sources: passages.clone(), web: false });
        }
        self.system.push_str("\n\n");
        self.system.push_str(&crate::search::ask::instructions(self.in_message));
        if let Some(s) = &t.skill {
            self.system.push_str("\n\n");
            self.system.push_str(&crate::chat::skills::system_block(s));
        }
        let block = crate::search::ask::context(&found);
        // A document written from them names its passages' meetings in
        // place of "[2]" (documents.rs `name_citations`).
        doc_turn.cite_passages(crate::search::ask::labels(&found));
        self.add(block);
        // A Skill that makes something to keep (/tickets, /prd): a document,
        // for a model that can write one — its line pointing at the panel,
        // or at Chat for the Voice Assistant asked from another app.
        let in_view = t.scope != Some("assistant") || crate::chat::assistant::asked_in_chat();
        if let Some(note) = t.skill.as_ref().filter(|_| t.gates.tools).and_then(|s| crate::chat::skills::document_note(s, in_view)) {
            self.context.push(note);
        }
        Across { passages, chip: Some(chip), found: Some(found) }
    }

    /// **From the person's library**: the notes the eager RAG finds (not
    /// across the meetings: those have their passages), the calendar Yap
    /// looks up for a model without tools — or a line saying none is
    /// connected —, this chat's documents for a model that changes them,
    /// memory (`memory_here`) and past chats, then how the person wants
    /// answers. The calendar's meetings it looked up, and its chip. `code`:
    /// a Code chat — no notes found by themselves, nor how the person wants
    /// answers (memory's); its model reads the notes with its tools when
    /// asked.
    #[allow(clippy::too_many_arguments)]
    fn library(
        &mut self,
        t: &Asked<'_>,
        chat_docs: &[crate::chat::documents::DocRef],
        memory_here: Option<String>,
        past_chats: Option<String>,
        calendar_zone: crate::chat::tools::calendar::Zone,
        code: bool,
    ) -> (Vec<crate::chat::tools::calendar::Cited>, Option<String>) {
        let g = t.gates;
        let rag = if t.meetings || code { String::new() } else { rag_context(&t.text, self.in_message) };
        if !rag.is_empty() {
            self.add(format!("{RAG_LEAD}\n\n{rag}"));
        }
        // A model without tools asked about the calendar: Yap looks it up for it
        // (not beside the fixed web pipeline, whose prompt is its own).
        let mut small_calendar: Vec<crate::chat::tools::calendar::Cited> = Vec::new();
        let (calendar_block, calendar_chip) = (g.calendar && !g.tools && !(g.web_on && crate::web::needs_web(&t.text)))
            .then(|| crate::chat::tools::calendar::block(&t.text, calendar_zone, t.now.timestamp(), &mut small_calendar))
            .flatten()
            .unzip();
        // Asked about the calendar with none connected: say so, not guess.
        let calendar_missing = (!g.calendar && !t.meetings && crate::chat::tools::calendar::asks_about_calendar(&t.text))
            .then(|| crate::chat::tools::calendar::NOT_CONNECTED_NOTE.to_string());
        // This chat's documents, for a model that changes them (documents.rs).
        if g.tools {
            if let Some(block) = crate::chat::documents::context(chat_docs, g.doc_tools) {
                self.context.push(block);
            }
        }
        for block in memory_here.into_iter().chain(past_chats).chain(calendar_block).chain(calendar_missing) {
            self.add(block);
        }
        // How the person wants answers, again last in the context, where a
        // small tool model heeds it (a model without tools has it at the end
        // of its system prompt, in memory's "You" block). Not in a Code chat.
        if self.in_message && !code {
            if let Some(note) = crate::memory::answer_note() {
                self.context.push(note);
            }
        }
        (small_calendar, calendar_chip)
    }

    /// The system prompt's end ([`ending`]: About you, memory's "You"), with
    /// [`Prompt::summary_at`] just before it, then what came with the
    /// question from the screen (the Voice Assistant's look at the window in
    /// front, `assistant/screen.rs`): what the picture or its text is, or
    /// why there's none.
    fn end(&mut self, about: &str, memory_you: &str, profile: &crate::ai::about_you::Profile) {
        // Where an older stretch's summary goes (`compact.rs`), when the history
        // needs one: before the prompt's end, which small models heed most.
        self.summary_at = self.system.len();
        // Last in the system prompt, where small models heed it: what the
        // context block is and About you without its clock — or, the context
        // staying in the system prompt, the whole block with it.
        self.system.push_str(&ending(self.in_message, profile, about, memory_you));
        if let Some(note) = crate::snip::turn::note() {
            self.add(note);
        }
    }

    /// **How to answer this one**: out loud (`spoken`), the Voice Assistant's
    /// line on documents, a passage selected in the panel or text selected
    /// in another app (`selection`, the turn's), and what the question means
    /// for this chat's documents (`chat_docs`, `documents::steer`).
    fn how_to_answer(&mut self, t: &Asked<'_>, spoken: bool, chat_docs: &[crate::chat::documents::DocRef], selection: Option<&crate::chat::documents::Selection>, context_size: u64) {
        let g = t.gates;
        // A tool model with the web starts with the answer; one read aloud
        // whole answers short (a web answer is said short anyway,
        // `assistant::spoken_answer`). Last in the context, just before the
        // question, where small models heed it.
        if spoken && g.tools && g.web_on {
            self.context.push("This question was asked out loud: start your answer with the answer itself, in one or two short sentences, then any details.".to_string());
        }
        // The Voice Assistant: a document only when one is asked for, then a
        // sentence saying where it is — "it's in Chat", or, asked with Chat in
        // front, "it's open on the right" (`assistant::asked_in_chat`). The
        // rule itself stays in the system prompt, the same for every turn (the
        // model's cache). (A Skill said aloud answers as its instructions say,
        // its document's line its own: `skills::document_note`.)
        if t.scope == Some("assistant") && g.tools && t.skill.is_none() {
            self.context.push(crate::chat::documents::spoken_note(crate::chat::assistant::asked_in_chat()).to_string());
        }
        // The model is told what this question means for documents
        // (documents.rs `steer`): an option picked → a new document; a change
        // to the open one → that document reworked; something to keep → a
        // document now. The system prompt's rule alone, the 4B answered a guide
        // and a trip plan in the chat, a changed build in the chat with an
        // offer, and "Let's do option A" by copying the options; Claude Haiku
        // 5.5 explained a build in the chat and offered to write it. A cloud
        // model's line is one short sentence (`brief`); in the context, never
        // the system prompt, so its prompt cache stays whole.
        // **A passage selected in the panel** ("Change this…", "Ask about
        // this"): which document and blocks, and that a change goes there only
        // — in place of `steer`'s line (the selection says where). A model
        // without tools hears what was selected.
        let selected = selection.and_then(|s| crate::chat::documents::selection_note(s, g.tools.then_some(g.doc_tools)));
        // **Text selected in another app** (`assistant/selected.rs`): what was
        // selected and where, as much as the model's context has room for.
        let selected = selected.or_else(|| {
            t.about_selection
                .as_ref()
                .map(|s| crate::chat::assistant::selected::context_note(s, crate::chat::assistant::selected::room(context_size)))
        });
        if let Some(note) = &selected {
            self.add(note.clone());
        }
        if g.tools && !t.meetings && selected.is_none() {
            let brief = !crate::ai::ai_model::runs_here(&t.ep.provider, &t.ep.base_url);
            if let Some(note) = crate::chat::documents::steer(&t.text, chat_docs, brief, g.doc_tools) {
                self.context.push(note);
            }
        }
    }

    /// **A strong model writing a document** (`writes_doc`): how far it may
    /// look, and to read the pages its facts come from (and a new one's
    /// outline first, `outline_first`). A strong Claude's own web fetch puts
    /// up to `FETCH_TOKENS` of a page in the request (Yap can't trim it):
    /// its reads are sized by that. How hard it thinks is its effort, as on
    /// every question (`ai/effort.rs`). How far the turn may look, written
    /// or not.
    fn research(&mut self, t: &Asked<'_>, sizing: &Sizing, writes_doc: bool, outline_first: bool) -> crate::web::Reach {
        let g = t.gates;
        let request = crate::chat::budget::budget(sizing.context_size, sizing.context_known, 0, sizing.kind, true, crate::chat::budget::request_cap(&t.ep.provider, &t.ep.model)).request;
        let deep_reach = if g.claude_in_chat {
            crate::web::Reach::deep_paged(Some(request), crate::web::native::FETCH_TOKENS as usize)
        } else {
            crate::web::Reach::deep(Some(request))
        };
        if writes_doc && g.web_on {
            let reader = if g.claude_in_chat {
                crate::chat::documents::Reader::WebFetch
            } else if g.own_search {
                crate::chat::documents::Reader::Search
            } else {
                crate::chat::documents::Reader::ReadPage
            };
            self.context.push(crate::chat::documents::research_note(deep_reach, reader, outline_first));
        }
        deep_reach
    }
}

/// Asked across the meetings ([`Prompt::across_meetings`]): the passages
/// found (the answer's sources, numbered), their chip, and what the search
/// found (its passages, for citations the answer left out). Empty for any
/// other question.
#[derive(Default)]
pub(super) struct Across {
    pub(super) passages: Vec<crate::chat::chats::Source>,
    pub(super) chip: Option<String>,
    pub(super) found: Option<crate::search::ask::Found>,
}

/// **What the prompt step settled** ([`build`]), for the steps after it:
/// the prompt, how the tool loop starts, what was found across the
/// meetings or in the calendar, this turn's documents and research, and
/// how earlier web answers and the small model's pipeline are told.
pub(super) struct Built {
    pub(super) prompt: Prompt,
    /// How the tool loop's first step goes ([`first_step`]).
    pub(super) first: FirstStep,
    pub(super) across: Across,
    /// The calendar Yap looked up for a model without tools: the meetings
    /// it numbered, and its chip.
    pub(super) small_calendar: Vec<crate::chat::tools::calendar::Cited>,
    pub(super) calendar_chip: Option<String>,
    /// The documents of this chat (documents.rs) and the ones this turn
    /// makes or changes, for its pills.
    pub(super) doc_turn: crate::chat::documents::Turn,
    /// **A strong model writing a document** (2026-10-31, as the Claude app
    /// writes one): it may research deeper — how far is sized to the
    /// model's request (`web::Reach::deep`, `deep_reach`), said in the
    /// context with the web on; how hard it thinks is its effort, as on
    /// every question (`ai/effort.rs`). Claude Opus 5.5, held to 3 searches,
    /// wrote a level-by-level guide of "Level 1", "Endgame" and "Reference
    /// only" rows. **A new one is outlined first** (`outline_first`,
    /// `web::Turn::outline_first`): nothing looked up before
    /// `create_document`, so the panel opens at once.
    pub(super) writes_doc: bool,
    pub(super) outline_first: bool,
    pub(super) deep_reach: crate::web::Reach,
    /// **Answered out loud**: the Voice Assistant's question, or one
    /// dictated into Chat with spoken replies on. A question only dictated
    /// into Chat is a Chat question, answered as a typed one (30 Oct 2026):
    /// its answer is read, not heard.
    pub(super) spoken: bool,
    /// How earlier web answers reach the model ([`web_history_mode`]): their
    /// searches replayed as tool calls (`replay`), else a line about them
    /// (`history_note`).
    pub(super) replay: bool,
    pub(super) history_note: bool,
    /// About you's block, and memory's "You": the end of a small model's
    /// web pipeline's prompt too (`small.rs`). (A Code chat: About you
    /// without its note, no "You".)
    pub(super) about: String,
    pub(super) memory_you: String,
    /// About you as this turn tells it ([`code_profile`]): a plan chat's
    /// system prompt is written from it (`plan.rs`).
    pub(super) profile: crate::ai::about_you::Profile,
    /// A Code chat's workspace (`Opened::code`): no memory in this turn, and
    /// on the Claude plan its session runs there (`plan.rs`).
    pub(super) code: Option<crate::agents::workspaces::Folder>,
    /// The person's time zone, for the calendar tool.
    pub(super) calendar_zone: crate::chat::tools::calendar::Zone,
}

/// **About you as a turn tells it**: whole in a plain chat; in a Code chat
/// (`code`) the name and the region, not the note (`Profile::without_note`).
/// Pure.
pub(super) fn code_profile(profile: &crate::ai::about_you::Profile, code: bool) -> crate::ai::about_you::Profile {
    if code {
        profile.without_note()
    } else {
        profile.clone()
    }
}

/// **The prompt for this turn** ([`Prompt`]) and what goes with it
/// ([`Built`]), its parts in the order they're written: the instructions
/// and the clock, the pages open beside the chat, the meetings' passages,
/// the library (notes, calendar, documents, memory), the prompt's end, how
/// to answer, a document's research, and the spoken answer's line.
pub(super) async fn build(t: &Asked<'_>, sizing: &Sizing, opened: &Opened) -> Built {
    let g = t.gates;
    let conv = &opened.conv;
    let selection = opened.selection.as_ref();
    // The documents of this chat (documents.rs): which there are, and what
    // the person changed since the model last saw them, for a model that
    // can change them; the ones this turn makes or changes, for its pills.
    let chat_docs = crate::chat::documents::of_conversation(&conv.messages);
    let doc_turn = crate::chat::documents::Turn::new(&chat_docs);
    // A strong model writing a document, and a new one outlined first
    // ([`Built::writes_doc`]).
    let writes_doc = g.strong && !t.meetings && crate::chat::documents::writes_document(&t.text, &chat_docs, selection.is_some());
    let outline_first = writes_doc && g.web_on && crate::chat::documents::makes_new_document(&t.text, &chat_docs, selection.is_some());
    // **A Code chat** (agents/code_chat.rs, 7 Nov 2026): project knowledge
    // lives with the project (its CLAUDE.md, Claude Code's own memory), and
    // Yap's memory is personal — so none of it here (its passages, its
    // "You", its tools, past chats), About you without its note, and
    // nothing found by itself: the model reads meetings, notes and tasks
    // with its tools when asked.
    let code = opened.code.is_some();
    let profile = code_profile(&t.profile, code);
    // Memory (memory.rs): the "You" part in the system prompt with About
    // you (stable from turn to turn), what this question is about with it.
    let memory_you = if code { String::new() } else { crate::memory::you_block() };
    // By its words, and by meaning when that's on (search/library.rs).
    let memory_here = if code { None } else { crate::search::library::memory_block(&t.text).await };
    // "Search and reference past chats": a tool for a model with tools; for
    // one without, the chats a question points back to ("what did we say
    // about Lisbon?"), a couple of lines each.
    let past_chats = if !code && t.cfg.memory_search_chats && !g.tools && crate::chat::chats::refers_to_past(&t.text) {
        crate::search::library::past_chats_block(&t.text, conv.id).await
    } else {
        None
    };
    // Which earlier answers came from a search: a tool-calling model with
    // the web gets each one's search replayed as the call it was
    // (`chats::history_messages`); any other model, a line in the
    // question's context (`chats::web_history_note`). Never a note in an
    // answer's text: a model copies it into answers that searched nothing.
    // (Not for a Claude searching in Chat's request: a replayed call to Yap's
    // `web_search` would share its name with Anthropic's.)
    let (replay, history_note) = web_history_mode(g.tools && g.web_on && !g.claude_in_chat);
    let about = crate::ai::about_you::prompt_block(&profile, t.now);
    let calendar_zone = crate::chat::tools::calendar::Zone::of(t.profile.region.time_zone.as_deref());
    let mut prompt = Prompt::start(t, !code);
    // A Code chat (agents/code_chat.rs): its folder, and that the person's own
    // request is enough there — in the question's context, so the tool loop
    // and a plan chat both have it.
    if let (true, Some(folder)) = (g.agents, opened.code.as_ref()) {
        prompt.add(crate::agents::code_chat::prompt_line_for(folder));
        // Its agent there, which a follow-up carries on (one agent per Code chat).
        if let Some(line) = crate::agents::code_chat::agent_line_in(Some(conv.id), folder) {
            prompt.add(line);
        }
    }
    let tabs_open = prompt.tabs(t).await;
    let here = crate::ai::ai_model::runs_here(&t.ep.provider, &t.ep.base_url);
    let first = first_step(&t.text, &g, here, outline_first, t.about_selection.is_some(), tabs_open);
    let across = prompt.across_meetings(t, &doc_turn).await;
    let (small_calendar, calendar_chip) = prompt.library(t, &chat_docs, memory_here, past_chats, calendar_zone, code);
    prompt.end(&about, &memory_you, &profile);
    let spoken = t.by_voice && (t.scope == Some("assistant") || t.cfg.speak_replies);
    prompt.how_to_answer(t, spoken, &chat_docs, selection, sizing.context_size);
    let deep_reach = prompt.research(t, sizing, writes_doc, outline_first);
    // Not a Skill's: its answer is Chat's run, whole in the chat, and the
    // voice says as much of it as "How much to read aloud" says
    // (`assistant::spoken_answer`, "The rest is in Chat.").
    if spoken && !(g.tools && g.web_on) && t.skill.is_none() {
        prompt.add(crate::chat::assistant::SPOKEN_ANSWER_NOTE.to_string());
    }
    Built {
        prompt,
        first,
        across,
        small_calendar,
        calendar_chip,
        doc_turn,
        writes_doc,
        outline_first,
        deep_reach,
        spoken,
        replay,
        history_note,
        about,
        memory_you,
        profile,
        code: opened.code.clone(),
        calendar_zone,
    }
}

/// **How the tool loop's first step goes** (`tools::FirstStep`): a model on
/// this PC (`here`) asked something that needs the web must search first
/// (left to decide, Qwen3.5-4B answered "the best RPG games coming out in
/// 2026" from memory, a tester's chat, 2026-10-21). Cloud models decide for
/// themselves. Never before a new document's outline (`outline_first`:
/// that comes first), nor for a question about text selected in another
/// app (`about_selection`). `tabs_open`: a page is open beside the chat.
fn first_step(text: &str, g: &Gates, here: bool, outline_first: bool, about_selection: bool, tabs_open: bool) -> FirstStep {
    if !outline_first
        && !about_selection
        && g.tools
        && g.web_on
        && here
        // (A question about the page open beside the chat reads that page.)
        && !(tabs_open && crate::chat::pages::about_open_page(text))
        // (Nor, for a model that may act on pages, one with a page open or a link
        // given: what it asks is likely that page's; it can still search.)
        && !(g.act && (tabs_open || !crate::web::links_in(text).is_empty()))
        && crate::web::needs_web(text)
    {
        first_search(text)
    } else {
        FirstStep::Model
    }
}

/// What follows the persona in Chat's system prompt: the tools' instructions
/// for a model that calls them (the web's, the apps', memory's, past
/// chats', the calendar's as the turn offers them), else the plain notes
/// for one that can't act. The same from turn to turn. A strong model gets
/// the web's lines that let it keep writing after the web, and a Claude
/// searching in Chat's own request its own tools' (`web_lines`,
/// `tools::web_tool_instructions`). Each family's lines only while the
/// gates offer its tools (`g`: a turn fitted to a small context leaves
/// some out, `fit.rs`). `memory`: memory's tools and past chats' are
/// offered (never in a Code chat). Connectors: Yap's own line about them,
/// never a server's instructions.
fn instructions(g: &Gates, profile: &crate::ai::about_you::Profile, memory: bool) -> String {
    let mut s = String::new();
    if g.tools {
        s.push_str("\n\nYou have access to tools. ");
        // (The documents' line is the set's: documents.rs `ToolSet`.)
        s.push_str(&crate::chat::tools::tool_instructions(g.doc_tools));
        if g.apps {
            s.push(' ');
            s.push_str(&crate::chat::tools::APP_TOOL_INSTRUCTIONS.join(" "));
        }
        if g.web_on {
            s.push(' ');
            s.push_str(&crate::chat::tools::web_tool_instructions(g.web_lines));
            s.push(' ');
            s.push_str(&crate::web::time_note(profile.region.time_zone_label.as_deref()));
            // Outside the US: prices from where they are, or one plain line
            // saying there are none.
            if let Some(local) = crate::ai::about_you::LocalPrices::for_region(&profile.region) {
                s.push(' ');
                s.push_str(&crate::web::price_note(&local));
            }
            // Pictures from the web (chat/pictures.rs): finding the right one,
            // looking before showing when the model can.
            if g.picture_tools {
                let lines = crate::chat::pictures::instructions(g.pictures, g.web_lines == crate::chat::tools::WebLines::Claude);
                if !lines.is_empty() {
                    s.push(' ');
                    s.push_str(&lines);
                }
            }
            // Pages beside the chat (chat/pages.rs): opening a page or a
            // search there, reading the one open, shots for a model that sees.
            let pages = crate::chat::pages::instructions(g.pictures, g.act);
            if !pages.is_empty() {
                s.push(' ');
                s.push_str(&pages);
            }
        }
        if memory {
            s.push(' ');
            s.push_str(&crate::chat::tools::MEMORY_TOOL_INSTRUCTIONS.join(" "));
            if g.past_chats {
                s.push(' ');
                s.push_str(&crate::chat::tools::CHATS_TOOL_INSTRUCTIONS.join(" "));
            }
        }
        if g.calendar {
            s.push(' ');
            s.push_str(&crate::chat::tools::calendar::CALENDAR_TOOL_INSTRUCTIONS.join(" "));
        }
        // Yap Code (chat/tools/agents.rs): agents on and Claude Code on this PC,
        // as the turn offers them (never across the meetings).
        if g.agents {
            s.push(' ');
            s.push_str(&crate::chat::tools::agents::AGENT_TOOL_INSTRUCTIONS.join(" "));
        }
        // Connectors (chat/tools/connectors.rs): Yap's own line, never a
        // server's instructions.
        if g.connectors {
            s.push(' ');
            s.push_str(crate::chat::tools::connectors::INSTRUCTIONS);
        }
    } else {
        // No tools: it can't do anything on the PC, so it mustn't say so.
        s.push_str("\n\n");
        s.push_str(crate::apps::NO_TOOLS_NOTE);
        if g.apps {
            s.push(' ');
            s.push_str(crate::apps::NO_TOOLS_OPEN);
        }
    }
    s
}

/// **The system prompt's start**, as every turn writes it and as it's
/// measured ([`stable_prompt`]): the persona, the instructions for what the
/// model may do ([`instructions`]) and the screen's line.
fn head(persona: &str, g: &Gates, profile: &crate::ai::about_you::Profile, memory: bool) -> String {
    let mut s = persona.trim().to_string();
    s.push_str(&instructions(g, profile, memory));
    let screen_line = crate::chat::see_screen::instructions(g.screen);
    if !screen_line.is_empty() {
        s.push(' ');
        s.push_str(screen_line);
    }
    s
}

/// The end of Chat's system prompt, where small models heed it: what the
/// context block is and About you without its clock (a model that calls
/// tools) — or, its context staying in the system prompt, the whole About
/// you block (`about`) — then memory's "You".
fn ending(in_message: bool, profile: &crate::ai::about_you::Profile, about: &str, memory_you: &str) -> String {
    let mut s = String::new();
    if in_message {
        s.push_str("\n\n");
        s.push_str(crate::chat::chats::CONTEXT_NOTE);
        let profile_block = crate::ai::about_you::profile_block(profile);
        if !profile_block.is_empty() {
            s.push_str("\n\n");
            s.push_str(&profile_block);
        }
    } else {
        s.push_str("\n\n");
        s.push_str(about);
    }
    if !memory_you.is_empty() {
        s.push_str("\n\n");
        s.push_str(memory_you);
    }
    s
}

/// The part of a Chat request that's there whatever the question, for the
/// turn's gates `g`: the system prompt's start as [`build`] writes it
/// ([`head`]: the persona, the instructions, the screen's line), the
/// prompt's end, and (a model that calls tools) the clock in the question's
/// context. What a question must fit beside (`chat_turn`'s check before
/// it's sent, Chat's limits and ring, the tools fitted to a small context:
/// `fit.rs`). `code`: a Code chat's, as [`build`] writes it — none of
/// memory's lines or its "You", About you without its note — so a question
/// there is measured against what's sent (8 Nov 2026: counted as a plain
/// chat's, an 8K model with the web and Yap Code on had no room left for
/// any question in a Code chat, though its requests fit).
pub(super) fn stable_prompt(persona: &str, g: &Gates, profile: &crate::ai::about_you::Profile, now: chrono::DateTime<chrono::Utc>, code: bool) -> String {
    let profile = &code_profile(profile, code);
    let about = crate::ai::about_you::prompt_block(profile, now);
    let memory_you = if code { String::new() } else { crate::memory::you_block() };
    let mut s = head(persona, g, profile, !code);
    s.push_str(&ending(g.tools, profile, &about, &memory_you));
    if g.tools {
        s.push_str("\n\n");
        s.push_str(&crate::ai::about_you::clock_line(profile, now));
    }
    s
}

/// How earlier web answers reach the model: `(replay, note)` — their
/// searches replayed as tool calls (a tool-calling model with the web on),
/// else the system prompt's line. A model evaluation may pick another way
/// (`YAP_EVAL_WEB_HISTORY` = replay | note | none) to compare them.
fn web_history_mode(tool_model_with_web: bool) -> (bool, bool) {
    if crate::ai::eval::active() {
        match std::env::var("YAP_EVAL_WEB_HISTORY").as_deref() {
            Ok("note") => return (false, true),
            Ok("none") => return (false, false),
            Ok("replay") => return (tool_model_with_web, true),
            _ => {}
        }
    }
    (tool_model_with_web, true)
}

/// The first step for a model on this PC when the question needs the web:
/// it must search (`tools::FirstStep::MustSearch`: its own query, written
/// with the conversation in mind — "Arsenal match tonight 21 October 2026"
/// where the question as a query found nothing useful; Yap searches for it
/// when the server won't make it). A model evaluation may compare
/// (`YAP_EVAL_FIRST` = must | yap | model).
fn first_search(question: &str) -> crate::chat::tools::FirstStep {
    if crate::ai::eval::active() {
        match std::env::var("YAP_EVAL_FIRST").as_deref() {
            Ok("yap") => return crate::chat::tools::FirstStep::YapSearches(crate::web::search_query(question)),
            Ok("model") => return crate::chat::tools::FirstStep::Model,
            _ => {}
        }
    }
    crate::chat::tools::FirstStep::MustSearch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::pictures::Gate;
    use crate::config::YapConfig;

    fn prompt(in_message: bool) -> Prompt {
        Prompt { system: "Persona.".into(), context: vec!["The clock.".into()], note_at: "Persona.".len(), summary_at: "Persona.".len(), in_message }
    }

    #[test]
    fn a_block_goes_with_the_question_for_a_tool_model_and_in_the_system_prompt_for_one_without() {
        let mut tools = prompt(true);
        tools.add("Notes.".into());
        tools.add_early("Earlier web answers.".into());
        tools.add_early("Another model wrote some.".into());
        assert_eq!(tools.system, "Persona.");
        assert_eq!(tools.context, ["The clock.", "Another model wrote some.", "Earlier web answers.", "Notes."]);
        let mut plain = prompt(false);
        plain.system.push_str(" Instructions.");
        plain.add("Notes.".into());
        plain.add_early("Earlier web answers.".into());
        plain.add_early("Another model wrote some.".into());
        assert_eq!(plain.system, "Persona.\n\nAnother model wrote some.\n\nEarlier web answers. Instructions.\n\nNotes.");
        assert_eq!(plain.context, ["The clock."]);
        // An empty context: the note goes first.
        let mut empty = Prompt { context: Vec::new(), ..prompt(true) };
        empty.add_early("Earlier web answers.".into());
        assert_eq!(empty.context, ["Earlier web answers."]);
    }

    /// Imogen in the UK, with a note of her own (fictional).
    fn imogen() -> crate::ai::about_you::Profile {
        let uk = crate::ai::about_you::Detected {
            locale: Some("en-GB".into()),
            country: Some("GB".into()),
            currency: Some("GBP".into()),
            symbol: Some("£".into()),
            metric: Some(true),
            time_zone: Some("Europe/London".into()),
        };
        crate::ai::about_you::Profile { region: crate::ai::about_you::resolve(&uk, ""), name: "Imogen".into(), about: "I keep bees on an allotment".into() }
    }

    fn tool_gates() -> Gates {
        Gates {
            tools: true,
            doc_tools: Default::default(),
            strong: false,
            claude_in_chat: false,
            own_search: false,
            web_lines: crate::chat::tools::WebLines::of(false, false, false),
            web_on: false,
            pictures: Gate::Off,
            screen: Gate::Off,
            act: false,
            calendar: false,
            agents: false,
            connectors: false,
            apps: true,
            past_chats: true,
            picture_tools: true,
        }
    }

    #[test]
    fn a_code_chats_prompt_keeps_yaps_memory_out_and_about_you_without_its_note() {
        use crate::memory::ops::Section;
        use crate::memory::{save_item, ItemInput};
        // Memory holds a person and how the user wants answers (this
        // thread's own store), and she's told About you something of hers.
        save_item(ItemInput { id: None, section: Section::People, title: "Aoife".into(), summary: "Cousin".into(), details: vec!["Lives in Galway".into()], dictation: true }).unwrap();
        save_item(ItemInput { id: None, section: Section::Preferences, title: String::new(), summary: String::new(), details: vec!["Likes answers as bullet points".into()], dictation: false }).unwrap();
        let you = crate::memory::you_block();
        assert!(you.contains("bullet points"), "{you}");
        let cfg = YapConfig { memory_search_chats: true, ..Default::default() };
        let ep = super::super::Endpoint {
            base_url: "https://api.example.com/v1".into(),
            api_key: String::new(),
            model: "fake-chat".into(),
            provider: "custom".into(),
            persona: "You are a helpful assistant.".into(),
            disable_thinking: false,
        };
        let stop = crate::chat::stream::Stoppable::new(None);
        let t = Asked {
            app: None,
            cfg,
            text: "Where does Aoife's watering schedule live in this project?".into(),
            scope: None,
            by_voice: false,
            skill: None,
            meetings: false,
            about_selection: None,
            ep,
            answered_by: serde_json::from_value(serde_json::json!({ "provider": "custom", "id": "fake-chat" })).unwrap(),
            gates: tool_gates(),
            profile: imogen(),
            now: chrono::Utc::now(),
            watcher: None,
            stop: &stop,
        };
        let conv: crate::chat::chats::Conversation = serde_json::from_value(serde_json::json!({ "id": 4242, "title": "Watering", "createdTs": 1, "updatedTs": 1 })).unwrap();
        let garden = crate::agents::workspaces::Folder { name: "garden-planner".into(), path: r"D:\Projects\garden-planner".into(), cap: None, added_ts: 1 };
        let opened = Opened { conv, user_index: 0, selection: None, code: Some(garden.clone()) };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let built = rt.block_on(build(&t, &Sizing::for_tests(128_000), &opened));
        let all = format!("{}\n{}", built.prompt.system, built.prompt.context.join("\n"));
        // None of memory: its passages, its "You", how they want answers.
        for never in ["Lives in Galway", "Cousin", "bullet points", "What Yap remembers"] {
            assert!(!all.contains(never), "{never}: {all}");
        }
        // About you: the name and the region, never the note.
        assert!(all.contains("Their name is Imogen.") && all.contains("they're in the United Kingdom (British English)"), "{all}");
        assert!(!all.contains("allotment") && !all.contains("They told Yap"), "{all}");
        // No memory tools, nor past chats', in its instructions.
        assert!(built.prompt.system.contains("You have access to tools."));
        assert!(!built.prompt.system.contains("Use remember") && !built.prompt.system.contains("search_chats"), "{}", built.prompt.system);
        assert!(built.memory_you.is_empty() && built.profile.about.is_empty() && !built.about.contains("allotment"));
        assert_eq!(built.code, Some(garden));

        // A plain chat's prompt as before: memory's "You", About you whole,
        // memory's tools (here the parts `build` writes them from, as a
        // plain turn's search for memory's passages would read this PC's
        // own settings).
        let plain = code_profile(&t.profile, false);
        let ending = ending(true, &plain, "", &you);
        assert!(ending.contains("bullet points") && ending.contains("They told Yap: \"I keep bees on an allotment\"") && ending.contains("Their name is Imogen."), "{ending}");
        let with = instructions(&t.gates, &plain, true);
        assert!(with.contains("Use remember") && with.contains("search_chats"));
        let without = instructions(&t.gates, &plain, false);
        assert!(!without.contains("Use remember") && !without.contains("search_chats"));
        // The note's the only part a Code chat drops.
        assert_eq!(code_profile(&t.profile, true), crate::ai::about_you::Profile { about: String::new(), ..imogen() });
    }

    #[test]
    fn a_code_chats_question_is_measured_against_what_its_turn_sends() {
        use crate::memory::ops::Section;
        use crate::memory::{save_item, ItemInput};
        // (8 Nov 2026: measured as a plain chat's, with memory's tools and
        // lines, an 8K model with the web and Yap Code on had no room left
        // for any question in a Code chat, though what it sends fits.)
        save_item(ItemInput { id: None, section: Section::Preferences, title: String::new(), summary: String::new(), details: vec!["Likes answers as bullet points".into()], dictation: false }).unwrap();
        let cfg = YapConfig { memory_search_chats: true, ..Default::default() };
        let g = Gates { web_on: true, pictures: Gate::Show, ..tool_gates() };
        let (persona, now) = ("You are a helpful assistant.", chrono::Utc::now());
        // The stable prompt: a Code chat's as `build` writes it — no memory
        // lines, no "You", About you without its note.
        let plain = g.stable_prompt(persona, &imogen(), now, false);
        let code = g.stable_prompt(persona, &imogen(), now, true);
        for part in ["Use remember", "search_chats", "bullet points", "allotment"] {
            assert!(plain.contains(part), "{part}: {plain}");
            assert!(!code.contains(part), "{part}: {code}");
        }
        assert!(code.contains("Their name is Imogen.") && code.contains("browser_navigate"), "{code}");
        // The tools' schemas: exactly the tools each turn offers (run.rs: a
        // Code chat none of memory's).
        let offered = |memory: bool| {
            let extra = crate::chat::tools::Extra { memory, search_chats: memory, conversation: Some(7), doc_tools: g.doc_tools, pictures: g.pictures, ..Default::default() };
            crate::chat::budget::estimate(&crate::chat::tools::offered_tools(cfg.open_apps, true, extra).to_string())
        };
        let schemas = |code: bool| crate::chat::budget::estimate(&g.offered(None, Some(7), code).to_string());
        let (plain_tools, code_tools) = (schemas(false), schemas(true));
        assert_eq!((plain_tools, code_tools), (offered(true), offered(false)));
        assert!(code_tools < plain_tools);
    }

    #[test]
    fn only_a_model_on_this_pc_must_search_first_and_never_before_an_outline() {
        let g = Gates {
            tools: true,
            doc_tools: Default::default(),
            strong: false,
            claude_in_chat: false,
            own_search: false,
            web_lines: crate::chat::tools::WebLines::of(false, false, false),
            web_on: true,
            pictures: Gate::Show,
            screen: Gate::Off,
            act: false,
            calendar: false,
            agents: false,
            connectors: false,
            apps: true,
            past_chats: true,
            picture_tools: true,
        };
        let asks = "What's the latest version of Python?";
        assert!(crate::web::needs_web(asks), "the test's question needs the web");
        assert_eq!(first_step(asks, &g, true, false, false, false), FirstStep::MustSearch);
        // A cloud model decides; so does one asked about something else.
        assert_eq!(first_step(asks, &g, false, false, false, false), FirstStep::Model);
        assert_eq!(first_step("Thanks, that's all.", &g, true, false, false, false), FirstStep::Model);
        // Never before a new document's outline, nor about selected text.
        assert_eq!(first_step(asks, &g, true, true, false, false), FirstStep::Model);
        assert_eq!(first_step(asks, &g, true, false, true, false), FirstStep::Model);
        // Without tools or the web: nothing to call.
        assert_eq!(first_step(asks, &Gates { web_on: false, ..g }, true, false, false, false), FirstStep::Model);
        assert_eq!(first_step(asks, &Gates { tools: false, ..g }, true, false, false, false), FirstStep::Model);
        // A model that may act on pages, with a page open: likely that page's.
        assert_eq!(first_step(asks, &Gates { act: true, ..g }, true, false, false, true), FirstStep::Model);
        assert_eq!(first_step(asks, &Gates { act: true, ..g }, true, false, false, false), FirstStep::MustSearch);
    }
}
