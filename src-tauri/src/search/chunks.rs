//! **Passages**: a note cut into the pieces search works on and Ask across
//! meetings cites. Pure.
//!
//! - A meeting's **transcript** by talk: consecutive lines (each "Speaker:
//!   words", the speaker map's names) up to [`MAX_WORDS`], a passage ending
//!   at a change of speaker once it has [`TARGET_WORDS`], always at a stop
//!   (`Note::breaks`) or a pause of [`GAP_SECS`]; echo, dictation markers and
//!   failed stretches left out; a line longer than a passage split at its
//!   sentences. Each passage keeps its lines (index, speaker, time), so a
//!   citation opens the meeting at the line it came from.
//! - The person's **notes** (`content`, My thoughts) and the **summary**
//!   (`enhanced_content`, the action plan) by paragraphs and list runs up to
//!   [`MAX_WORDS`], a heading starting a new passage and going with the ones
//!   under it ("Decisions: …").
//! - A note with nothing in it but its title: one passage, the title.
//!
//! The same note always gives the same passages, so the index keys each by a
//! hash of what's embedded ([`key`]) and finds the ones it has already read.

use crate::notes::{Note, TranscriptSegment};

/// The most words a passage holds (~150 tokens for EmbeddingGemma: a
/// citation narrows to a few lines, and a query's meaning isn't diluted).
pub const MAX_WORDS: usize = 110;
/// A transcript passage ends at a change of speaker once it has this many.
pub const TARGET_WORDS: usize = 60;
/// A pause this long between lines starts a new passage.
pub const GAP_SECS: u64 = 300;
/// A meeting's last passage under this many words joins the one before (at
/// most [`MAX_WORDS`] + 24 words together).
const MIN_TAIL_WORDS: usize = 25;
/// A chat's long answer starts in its question's passage when at least
/// this many of its words fit there.
const MIN_FIRST_PIECE: usize = 40;

/// Where in a note a passage comes from (or that it's a chat's, or a memory
/// item's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// The meeting's transcript.
    Transcript,
    /// The person's own notes (My thoughts).
    Content,
    /// The summary / action plan.
    Summary,
    /// A chat's messages (each line one, by its index in the conversation).
    Chat,
    /// A memory item: who or what it is, and its facts.
    Memory,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Transcript => "transcript",
            Field::Content => "content",
            Field::Summary => "summary",
            Field::Chat => "chat",
            Field::Memory => "memory",
        }
    }
}

/// One line of a passage: a transcript line (its index in the note), or a
/// line of the notes or the summary (its line number in that field).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub index: usize,
    /// Who said it ("Priya", "You", "Speaker 2"); empty for notes.
    pub speaker: String,
    pub text: String,
    /// When it was said (unix seconds); the note's time for notes.
    pub ts: u64,
}

/// A passage.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Its note's id (a chat's, a memory item's: their own ids, never mixed
    /// with notes' in one search).
    pub note: u64,
    pub field: Field,
    /// The note's title ("Untitled meeting" / "Untitled note" when empty).
    pub title: String,
    /// "meeting" | "note" | "chat" | "memory".
    pub kind: &'static str,
    /// The heading it sits under (notes and summaries), if any.
    pub heading: String,
    pub lines: Vec<Line>,
}

impl Chunk {
    /// When it was said or written: its first line's time.
    pub fn ts(&self) -> u64 {
        self.lines.first().map(|l| l.ts).unwrap_or(0)
    }

    /// The passage as it's searched, shown and given to the model: one line
    /// each, "Speaker: words" for a transcript, the heading first for notes.
    pub fn text(&self) -> String {
        let mut out = String::new();
        if !self.heading.is_empty() {
            out.push_str(&self.heading);
            out.push('\n');
        }
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if !l.speaker.is_empty() {
                out.push_str(&l.speaker);
                out.push_str(": ");
            }
            out.push_str(&l.text);
        }
        out
    }

    /// The speakers in it, in order, once each.
    pub fn speakers(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for l in &self.lines {
            if !l.speaker.is_empty() && !out.contains(&l.speaker.as_str()) {
                out.push(&l.speaker);
            }
        }
        out
    }

    #[cfg(test)]
    pub fn words(&self) -> usize {
        self.lines.iter().map(|l| word_count(&l.text)).sum()
    }
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

/// When a note's meeting was (or the note was made): the calendar event's
/// start, else its first transcript line, else when it was made.
pub fn note_time(n: &Note) -> u64 {
    n.event.as_ref().map(|e| e.start).or_else(|| n.transcript.first().map(|s| s.ts)).unwrap_or(n.created_ts)
}

/// "meeting" for a meeting note (or one with a transcript), else "note".
pub fn note_kind(n: &Note) -> &'static str {
    if n.note_type == "meeting" || !n.transcript.is_empty() || n.event.is_some() {
        "meeting"
    } else {
        "note"
    }
}

/// The title a passage shows.
pub fn note_title(n: &Note) -> String {
    let t = n.title.trim();
    if !t.is_empty() {
        t.to_string()
    } else if note_kind(n) == "meeting" {
        "Untitled meeting".to_string()
    } else {
        "Untitled note".to_string()
    }
}

/// Every passage of `note`: its transcript's, then its notes', then its
/// summary's (a note with none of them: its title alone).
pub fn chunk_note(note: &Note) -> Vec<Chunk> {
    let title = note_title(note);
    let kind = note_kind(note);
    let when = note_time(note);
    let mut out = transcript_chunks(note, &title, kind);
    out.extend(text_chunks(note.id, &title, kind, Field::Content, &note.content, when));
    out.extend(text_chunks(note.id, &title, kind, Field::Summary, &note.enhanced_content, when));
    if out.is_empty() && !note.title.trim().is_empty() {
        out.push(Chunk {
            note: note.id,
            field: Field::Content,
            title: title.clone(),
            kind,
            heading: String::new(),
            lines: vec![Line { index: 0, speaker: String::new(), text: title, ts: when }],
        });
    }
    out
}

/// A transcript line worth searching: talk with words in it.
fn searchable(seg: &TranscriptSegment) -> bool {
    seg.is_talk() && !seg.text.trim().is_empty()
}

fn transcript_chunks(note: &Note, title: &str, kind: &'static str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut cur: Vec<Line> = Vec::new();
    let mut words = 0usize;
    // The passage under way began after a stop (never merged across one).
    let mut after_stop = false;
    let flush = |cur: &mut Vec<Line>, words: &mut usize, out: &mut Vec<Chunk>| {
        if !cur.is_empty() {
            out.push(Chunk {
                note: note.id,
                field: Field::Transcript,
                title: title.to_string(),
                kind,
                heading: String::new(),
                lines: std::mem::take(cur),
            });
        }
        *words = 0;
    };
    for (i, seg) in note.transcript.iter().enumerate() {
        if !searchable(seg) {
            continue;
        }
        let text = seg.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let w = word_count(&text);
        let speaker = crate::speakers::prompt_label(&note.speakers, seg);
        let stop = note.breaks.contains(&i) || cur.last().is_some_and(|l| seg.ts > l.ts.saturating_add(GAP_SECS));
        let new_speaker = cur.last().is_some_and(|l| l.speaker != speaker);
        if !cur.is_empty() && (stop || words + w > MAX_WORDS || (words >= TARGET_WORDS && new_speaker)) {
            flush(&mut cur, &mut words, &mut out);
        }
        if stop {
            after_stop = true;
        } else if cur.is_empty() && !out.is_empty() {
            after_stop = false;
        }
        if w > MAX_WORDS {
            // One long line: a passage per stretch of its sentences.
            flush(&mut cur, &mut words, &mut out);
            for piece in split_long(&text, MAX_WORDS) {
                cur.push(Line { index: i, speaker: speaker.clone(), text: piece, ts: seg.ts });
                flush(&mut cur, &mut words, &mut out);
            }
            continue;
        }
        cur.push(Line { index: i, speaker, text, ts: seg.ts });
        words += w;
    }
    // A few words left at the end (the meeting's last "thanks, bye" or a
    // reply cut off by the size rule) go with the passage before, where
    // they belong, rather than make a passage that matches by accident.
    if !cur.is_empty() && words < MIN_TAIL_WORDS && !after_stop {
        if let Some(prev) = out.last_mut() {
            prev.lines.append(&mut cur);
        }
    }
    flush(&mut cur, &mut words, &mut out);
    out
}

/// `text` in pieces of at most `max` words, cut after a sentence where one
/// ends in the second half of a piece, else at `max`.
pub fn split_long(text: &str, max: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let end = (start + max).min(words.len());
        let mut cut = end;
        if end < words.len() {
            if let Some(k) = (start + max / 2..end).rev().find(|&k| words[k].ends_with(['.', '!', '?'])) {
                cut = k + 1;
            }
        }
        out.push(words[start..cut].join(" "));
        start = cut;
    }
    out
}

/// A markdown line's heading text ("## Decisions" → "Decisions"), if it's one.
fn heading_of(line: &str) -> Option<String> {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
        let h = t[hashes..].trim().trim_end_matches(':').trim();
        return (!h.is_empty()).then(|| h.to_string());
    }
    // "**Decisions**" alone on a line.
    let bold = t.trim();
    if bold.len() > 4 && bold.starts_with("**") && bold.ends_with("**") && !bold[2..bold.len() - 2].contains("**") {
        let h = bold[2..bold.len() - 2].trim().trim_end_matches(':').trim();
        return (!h.is_empty() && word_count(h) <= 8).then(|| h.to_string());
    }
    None
}

/// A notes field (markdown) in passages: paragraphs and list runs, a heading
/// starting a new one and going with it.
fn text_chunks(note: u64, title: &str, kind: &'static str, field: Field, text: &str, when: u64) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut cur: Vec<Line> = Vec::new();
    let mut words = 0usize;
    let mut flush = |cur: &mut Vec<Line>, words: &mut usize, heading: &str| {
        if !cur.is_empty() {
            out.push(Chunk {
                note,
                field,
                title: title.to_string(),
                kind,
                heading: heading.to_string(),
                lines: std::mem::take(cur),
            });
        }
        *words = 0;
    };
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            // A blank line ends a paragraph once there's enough of it.
            if words >= TARGET_WORDS {
                flush(&mut cur, &mut words, &heading);
            }
            continue;
        }
        if line.starts_with("```") || line.chars().all(|c| matches!(c, '-' | '*' | '_' | '=' | ' ')) {
            continue; // fences and rules
        }
        if let Some(h) = heading_of(line) {
            flush(&mut cur, &mut words, &heading);
            heading = h;
            continue;
        }
        // Task boxes and bullets as plain words.
        let clean = line
            .trim_start_matches(['-', '*', '+', '•'])
            .trim_start()
            .trim_start_matches("[ ]")
            .trim_start_matches("[x]")
            .trim_start_matches("[X]")
            .replace("**", "")
            .trim()
            .to_string();
        if clean.is_empty() {
            continue;
        }
        let w = word_count(&clean);
        if !cur.is_empty() && words + w > MAX_WORDS {
            flush(&mut cur, &mut words, &heading);
        }
        if w > MAX_WORDS {
            flush(&mut cur, &mut words, &heading);
            for piece in split_long(&clean, MAX_WORDS) {
                cur.push(Line { index: i, speaker: String::new(), text: piece, ts: when });
                flush(&mut cur, &mut words, &heading);
            }
            continue;
        }
        cur.push(Line { index: i, speaker: String::new(), text: clean, ts: when });
        words += w;
    }
    flush(&mut cur, &mut words, &heading);
    out
}

/// The title a chat's passages show.
pub fn chat_title(c: &crate::chat::chats::Conversation) -> String {
    let t = c.title.trim();
    if t.is_empty() {
        "Untitled chat".to_string()
    } else {
        t.to_string()
    }
}

/// **A chat in passages** (search by meaning over past chats, the Chat
/// sidebar's search, `search_chats`): its messages as lines ("You: …",
/// "Yap: …", each by its index in the conversation), a passage holding a
/// question and its answer where they fit — at most [`MAX_WORDS`], ending
/// at the person's next question once it has [`TARGET_WORDS`]; a message
/// longer than a passage cut at its sentences (its question with its first
/// piece); a few words of an answer left at the end joining the passage
/// before. Tool chips, sources and screenshots aren't searched: what was
/// said is. Pure.
pub fn chunk_chat(conv: &crate::chat::chats::Conversation) -> Vec<Chunk> {
    let title = chat_title(conv);
    let mut out: Vec<Chunk> = Vec::new();
    let mut cur: Vec<Line> = Vec::new();
    let mut words = 0usize;
    let flush = |cur: &mut Vec<Line>, words: &mut usize, out: &mut Vec<Chunk>| {
        if !cur.is_empty() {
            out.push(Chunk { note: conv.id, field: Field::Chat, title: title.clone(), kind: "chat", heading: String::new(), lines: std::mem::take(cur) });
        }
        *words = 0;
    };
    for (i, m) in conv.messages.iter().enumerate() {
        let text = m.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        let speaker = if m.role == "user" { "You" } else { "Yap" }.to_string();
        let w = word_count(&text);
        let asks = m.role == "user";
        if w > MAX_WORDS {
            // A long answer: its question goes with its first piece (when
            // there's room for a good part of one), every other piece is a
            // passage of its own.
            if MAX_WORDS.saturating_sub(words) < MIN_FIRST_PIECE || (asks && words >= TARGET_WORDS) {
                flush(&mut cur, &mut words, &mut out);
            }
            let first = split_long(&text, MAX_WORDS - words).into_iter().next().unwrap_or_default();
            let used = word_count(&first);
            cur.push(Line { index: i, speaker: speaker.clone(), text: first, ts: m.ts });
            flush(&mut cur, &mut words, &mut out);
            let rest = text.split_whitespace().skip(used).collect::<Vec<_>>().join(" ");
            for piece in split_long(&rest, MAX_WORDS) {
                cur.push(Line { index: i, speaker: speaker.clone(), text: piece, ts: m.ts });
                flush(&mut cur, &mut words, &mut out);
            }
            continue;
        }
        if !cur.is_empty() && (words + w > MAX_WORDS || (asks && words >= TARGET_WORDS)) {
            flush(&mut cur, &mut words, &mut out);
        }
        cur.push(Line { index: i, speaker, text, ts: m.ts });
        words += w;
    }
    // A few words of an answer left at the end go with their question's
    // passage (a new question is a passage of its own, however short).
    if !cur.is_empty() && words < MIN_TAIL_WORDS && cur[0].speaker != "You" {
        if let Some(prev) = out.last_mut() {
            if prev.lines.iter().map(|l| word_count(&l.text)).sum::<usize>() + words <= MAX_WORDS + 24 {
                prev.lines.append(&mut cur);
            }
        }
    }
    flush(&mut cur, &mut words, &mut out);
    out
}

/// A memory item as one passage (search by meaning over what Yap remembers:
/// the people, projects and topics a question is about): its name as the
/// title, then who or what it is and its facts, a line each. Profile and
/// Preferences aren't searched: they go with every question. Pure.
pub fn chunk_memory(item: &crate::memory::ops::Item) -> Option<Chunk> {
    if item.section.is_you() || item.title.trim().is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    let mut add = |index: usize, text: &str| {
        let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() {
            lines.push(Line { index, speaker: String::new(), text: t, ts: item.updated_ts });
        }
    };
    add(0, &item.summary);
    for (i, f) in item.details.iter().enumerate() {
        add(i + 1, f);
    }
    if lines.is_empty() {
        lines.push(Line { index: 0, speaker: String::new(), text: item.title.trim().to_string(), ts: item.updated_ts });
    }
    Some(Chunk { note: item.id, field: Field::Memory, title: item.title.trim().to_string(), kind: "memory", heading: String::new(), lines })
}

/// What the embedding model reads for a passage (its document prompt:
/// EmbeddingGemma's "title: … | text: …").
pub fn embed_text(c: &Chunk) -> String {
    super::embedder::document(&c.title, &c.text())
}

/// The index's key for a passage read by `model`: the first 16 bytes of the
/// SHA-256 of the model's tag and what it reads. Pure.
pub fn key(model: &str, embedded: &str) -> [u8; 16] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0u8]);
    h.update(embedded.as_bytes());
    let full = h.finalize();
    let mut k = [0u8; 16];
    k.copy_from_slice(&full[..16]);
    k
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A note from JSON (the fields a test cares about).
    pub(crate) fn note(v: serde_json::Value) -> Note {
        let mut base = json!({ "id": 1, "createdTs": 1_000, "updatedTs": 1_000 });
        if let (Some(b), Some(o)) = (base.as_object_mut(), v.as_object()) {
            for (k, val) in o {
                b.insert(k.clone(), val.clone());
            }
        }
        serde_json::from_value(base).expect("a note")
    }

    pub(crate) fn line(source: &str, text: &str, ts: u64) -> serde_json::Value {
        json!({ "source": source, "text": text, "ts": ts })
    }

    #[test]
    fn a_transcript_is_cut_by_talk() {
        let long = "word ".repeat(40);
        let n = note(json!({
            "noteType": "meeting",
            "title": "Budget review",
            "transcript": [
                line("them", &format!("Priya here. {long}"), 100),
                line("them", &long, 110),
                line("you", "Agreed, the venue costs too much and we should look at the catering numbers again before Friday.", 120),
                { "source": "you", "text": "", "ts": 125, "dictated": true },
                { "source": "you", "text": "echoed words", "ts": 126, "echo": true },
                line("you", "Let's cut the catering instead and keep the second night for the team dinner.", 130),
            ],
        }));
        let chunks = chunk_note(&n);
        assert_eq!(chunks.len(), 2, "{chunks:#?}");
        // Them's two lines (82 words) end at the change of speaker.
        assert_eq!(chunks[0].lines.iter().map(|l| l.index).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(chunks[0].speakers(), ["Them"]);
        // Markers and echo are left out; the line indexes are the note's.
        assert_eq!(chunks[1].lines.iter().map(|l| l.index).collect::<Vec<_>>(), [2, 5]);
        assert_eq!(chunks[1].text(), "You: Agreed, the venue costs too much and we should look at the catering numbers again before Friday.\nYou: Let's cut the catering instead and keep the second night for the team dinner.");
        assert_eq!((chunks[1].field, chunks[1].kind, chunks[1].ts()), (Field::Transcript, "meeting", 120));
        assert_eq!(chunks[1].title, "Budget review");
    }

    #[test]
    fn a_passage_never_grows_past_its_size_and_stops_at_a_break() {
        let thirty = "talk ".repeat(30);
        let lines: Vec<_> = (0..8).map(|i| line("them", &thirty, 100 + i * 5)).collect();
        let mut n = note(json!({ "noteType": "meeting", "transcript": lines }));
        let chunks = chunk_note(&n);
        assert!(chunks.iter().all(|c| c.words() <= MAX_WORDS), "{chunks:#?}");
        assert_eq!(chunks.len(), 3, "three lines of 30 words fit, a fourth doesn't");
        // A stop after line 2 (a pause, then Resume): line 2 starts afresh.
        n.breaks = vec![2];
        let chunks = chunk_note(&n);
        assert_eq!(chunks[0].lines.len(), 2);
        assert_eq!(chunks[1].lines[0].index, 2);
        // So does a long pause.
        let n = note(json!({ "noteType": "meeting", "transcript": [line("them", "Before lunch.", 100), line("them", "After lunch.", 100 + GAP_SECS + 1)] }));
        assert_eq!(chunk_note(&n).len(), 2);
    }

    #[test]
    fn a_few_words_at_the_end_join_the_passage_before() {
        let sixty = "word ".repeat(64);
        let n = note(json!({ "noteType": "meeting", "transcript": [
            line("them", &sixty, 100),
            line("you", "Thanks everyone, bye.", 110),
        ]}));
        let chunks = chunk_note(&n);
        assert_eq!(chunks.len(), 1, "not a passage of three words");
        assert_eq!(chunks[0].lines.iter().map(|l| l.index).collect::<Vec<_>>(), [0, 1]);
        // Never across a stop.
        let mut stopped = n.clone();
        stopped.breaks = vec![1];
        assert_eq!(chunk_note(&stopped).len(), 2);
        // Nor when it's the only passage.
        let alone = note(json!({ "noteType": "meeting", "transcript": [line("you", "Thanks everyone, bye.", 110)] }));
        assert_eq!(chunk_note(&alone).len(), 1);
    }

    #[test]
    fn a_very_long_line_is_split_at_its_sentences() {
        let sentence = "This sentence has exactly eight words in it. ";
        let text = sentence.repeat(30); // 240 words
        let n = note(json!({ "noteType": "meeting", "transcript": [line("them", &text, 100)] }));
        let chunks = chunk_note(&n);
        assert!(chunks.len() >= 3, "{}", chunks.len());
        assert!(chunks.iter().all(|c| c.words() <= MAX_WORDS && c.lines[0].index == 0));
        assert!(chunks[0].lines[0].text.ends_with('.'), "cut after a sentence");
        assert_eq!(chunks.iter().map(|c| c.words()).sum::<usize>(), 240, "nothing lost");
        assert_eq!(split_long("one two three", 2), ["one two", "three"]);
        assert!(split_long("", 5).is_empty());
    }

    #[test]
    fn notes_and_summaries_by_paragraph_with_their_headings() {
        let plan = "Q3 offsite planning.\n\n## Action plan\n\n### Alice\n- [ ] Send the revised budget (due: Friday)\n\n## Decisions\n- Hold the offsite in Lisbon\n\n---\n## Open questions\n- Who handles catering?";
        let n = note(json!({
            "noteType": "meeting",
            "title": "Offsite",
            "content": "Ask **Bob** about the venue.\nCheck flights.",
            "enhancedContent": plan,
            "createdTs": 500,
        }));
        let chunks = chunk_note(&n);
        let content: Vec<_> = chunks.iter().filter(|c| c.field == Field::Content).collect();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0].text(), "Ask Bob about the venue.\nCheck flights.");
        assert_eq!(content[0].ts(), 500, "a note's passage is dated by the note");
        let summary: Vec<_> = chunks.iter().filter(|c| c.field == Field::Summary).collect();
        let texts: Vec<String> = summary.iter().map(|c| c.text()).collect();
        assert_eq!(
            texts,
            [
                "Q3 offsite planning.",
                "Alice\nSend the revised budget (due: Friday)",
                "Decisions\nHold the offsite in Lisbon",
                "Open questions\nWho handles catering?",
            ]
        );
        // Each line keeps its line number in the field.
        assert_eq!(summary[2].lines[0].index, 8);
    }

    #[test]
    fn a_title_alone_is_a_passage_and_an_empty_note_none() {
        let n = note(json!({ "title": "Call the plumber" }));
        let chunks = chunk_note(&n);
        assert_eq!(chunks.len(), 1);
        assert_eq!((chunks[0].text().as_str(), chunks[0].kind), ("Call the plumber", "note"));
        assert!(chunk_note(&note(json!({}))).is_empty());
        assert_eq!(note_title(&note(json!({ "noteType": "meeting" }))), "Untitled meeting");
    }

    #[test]
    fn named_voices_carry_their_names() {
        let n = note(json!({
            "noteType": "meeting",
            "transcript": [{ "source": "them", "speaker": "them-1", "text": "The budget is tight.", "ts": 100 }],
            "speakers": { "them-1": { "name": "Priya Shah", "source": "user" } },
        }));
        let c = &chunk_note(&n)[0];
        assert_eq!(c.text(), "Priya Shah: The budget is tight.");
    }

    #[test]
    fn the_same_words_have_the_same_key_and_another_model_another() {
        let a = key("m1", "title: x | text: y");
        assert_eq!(a, key("m1", "title: x | text: y"));
        assert_ne!(a, key("m2", "title: x | text: y"));
        assert_ne!(a, key("m1", "title: x | text: z"));
    }

    fn chat(title: &str, messages: &[(&str, &str)]) -> crate::chat::chats::Conversation {
        serde_json::from_value(json!({
            "id": 9, "title": title, "createdTs": 100, "updatedTs": 100,
            "messages": messages.iter().enumerate().map(|(i, (r, t))| json!({ "role": r, "text": t, "ts": 100 + i as u64 })).collect::<Vec<_>>(),
        }))
        .unwrap()
    }

    #[test]
    fn a_chat_is_cut_into_questions_with_their_answers() {
        let forty = "word ".repeat(40);
        let c = chat(
            "Trip ideas",
            &[
                ("user", "Where should we eat in Lisbon?"),
                ("assistant", &format!("Try Alfama. {forty}")),
                ("user", "And breakfast?"),
                ("assistant", "A pastelaria near the hotel."),
                ("user", ""),
            ],
        );
        let chunks = chunk_chat(&c);
        // The first question and answer (48 words) take the next question
        // in too: under 60 words, a passage goes on.
        assert_eq!(chunks.len(), 1, "{chunks:#?}");
        assert_eq!((chunks[0].kind, chunks[0].field, chunks[0].title.as_str(), chunks[0].note), ("chat", Field::Chat, "Trip ideas", 9));
        assert_eq!(chunks[0].lines.iter().map(|l| l.index).collect::<Vec<_>>(), [0, 1, 2, 3], "the empty message left out");
        assert_eq!(chunks[0].speakers(), ["You", "Yap"]);
        assert!(chunks[0].text().starts_with("You: Where should we eat in Lisbon?\nYap: Try Alfama."));
        // Past 60 words, the person's next question starts a passage.
        let sixty = "word ".repeat(62);
        let c = chat("Long", &[("user", "First?"), ("assistant", &sixty), ("user", "Second question here please"), ("assistant", "A short answer to it, in a few more words than that.")]);
        let chunks = chunk_chat(&c);
        assert_eq!(chunks.iter().map(|c| c.lines.iter().map(|l| l.index).collect::<Vec<_>>()).collect::<Vec<_>>(), [vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn a_long_answer_starts_with_its_question_and_goes_on_in_passages() {
        let sentence = "This sentence has exactly ten words in it, all told. ";
        let long = sentence.repeat(30); // 300 words
        let c = chat("Essay", &[("user", "Write me three hundred words please"), ("assistant", &long)]);
        let chunks = chunk_chat(&c);
        assert!(chunks.len() >= 3, "{}", chunks.len());
        // The question and the answer's first piece together.
        assert_eq!(chunks[0].lines.len(), 2);
        assert_eq!((chunks[0].lines[0].index, chunks[0].lines[1].index), (0, 1));
        assert!(chunks.iter().all(|c| c.words() <= MAX_WORDS + 24), "{:?}", chunks.iter().map(|c| c.words()).collect::<Vec<_>>());
        // Every word of the answer once.
        let answer: usize = chunks.iter().flat_map(|c| c.lines.iter().filter(|l| l.index == 1)).map(|l| word_count(&l.text)).sum();
        assert_eq!(answer, 300);
        assert!(chunks[1..].iter().all(|c| c.lines.iter().all(|l| l.index == 1 && l.speaker == "Yap")));
    }

    #[test]
    fn a_memory_item_is_one_passage_and_you_none() {
        let item: crate::memory::ops::Item = serde_json::from_value(json!({
            "id": 4, "section": "people", "title": "Leah", "summary": "Sister", "details": ["Lives in Leeds", "Birthday on 12 May"], "updatedTs": 50,
        }))
        .unwrap();
        let c = chunk_memory(&item).unwrap();
        assert_eq!((c.kind, c.field, c.note, c.title.as_str()), ("memory", Field::Memory, 4, "Leah"));
        assert_eq!(c.text(), "Sister\nLives in Leeds\nBirthday on 12 May");
        assert_eq!(embed_text(&c), "title: Leah | text: Sister\nLives in Leeds\nBirthday on 12 May");
        let you: crate::memory::ops::Item = serde_json::from_value(json!({ "id": 1, "section": "profile", "title": "Profile", "details": ["Lives in Leeds"] })).unwrap();
        assert!(chunk_memory(&you).is_none(), "Profile goes with every question already");
    }
}
