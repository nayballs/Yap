//! The end of a meeting's one call: its input (the digests and the
//! latest talk, bounded whatever the length) and the checks on the action
//! plan it writes (invented owners moved, lost tasks put back).

use std::collections::HashSet;

use crate::notes::{DigestAction, MeetingDigest, Note};

use super::digest::{due_regex, push_unique, resolve_owner, strip_bullet, supported_due, words};
use super::{
    clock, estimate_tokens, fits_single_pass_within, keep_last_tokens, meeting_t0, people_lines, transcript_lines, truncate_to_tokens,
    FinalBudget, FINAL_NOTES_TOKENS, FINAL_TAIL_TOKENS, UNASSIGNED,
};

// ---- The final call's input ----

fn action_line(a: &DigestAction) -> String {
    if a.due.is_empty() {
        format!("- [ ] {}: {}", a.owner, a.task)
    } else {
        format!("- [ ] {}: {} (due: {})", a.owner, a.task, a.due)
    }
}

/// Every digest action, duplicates (same owner, same task) once.
pub fn digest_actions(digests: &[MeetingDigest]) -> Vec<DigestAction> {
    let mut out: Vec<DigestAction> = Vec::new();
    for a in digests.iter().flat_map(|d| d.actions.iter()) {
        if !out
            .iter()
            .any(|b| b.owner.eq_ignore_ascii_case(&a.owner) && b.task.eq_ignore_ascii_case(&a.task))
        {
            out.push(a.clone());
        }
    }
    out
}

/// The digests as one section, keeping `points_cap` key points of every
/// `stride`-th digest and `items_cap` decisions/tasks/questions of each.
fn render_with(
    digests: &[MeetingDigest],
    t0: u64,
    points_cap: usize,
    stride: usize,
    items_cap: usize,
) -> String {
    let mut points = String::new();
    let (mut decisions, mut questions): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut actions: Vec<DigestAction> = Vec::new();
    for (i, d) in digests.iter().enumerate() {
        let at = clock(d.start_ts.saturating_sub(t0));
        let points_cap = if i % stride.max(1) == 0 {
            points_cap
        } else {
            0
        };
        for p in d.key_points.iter().take(points_cap) {
            points.push_str(&format!("- ({at}) {p}\n"));
        }
        if points_cap > 0 && !d.raw.is_empty() {
            let raw = truncate_to_tokens(&d.raw, 150);
            for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                points.push_str(&format!("- ({at}) {}\n", strip_bullet(line)));
            }
        }
        for x in d.decisions.iter().take(items_cap) {
            push_unique(&mut decisions, x.clone());
        }
        for x in d.questions.iter().take(items_cap) {
            push_unique(&mut questions, x.clone());
        }
        for a in d.actions.iter().take(items_cap) {
            if !actions.contains(a) {
                actions.push(a.clone());
            }
        }
    }
    let mut out = String::new();
    if !points.is_empty() {
        out.push_str("### Key points\n");
        out.push_str(&points);
    }
    let mut list = |title: &str, items: Vec<String>| {
        if !items.is_empty() {
            out.push_str(&format!("### {title}\n"));
            for i in items {
                out.push_str(&i);
                out.push('\n');
            }
        }
    };
    list(
        "Decisions",
        decisions.iter().map(|d| format!("- {d}")).collect(),
    );
    list("Action items", actions.iter().map(action_line).collect());
    list(
        "Open questions",
        questions.iter().map(|q| format!("- {q}")).collect(),
    );
    out
}

/// The digests as one section within `budget` tokens: everything when it
/// fits; else key points give way first, evenly across the meeting (fewer
/// per digest, then the lead point of every 2nd, 3rd… digest), then
/// decisions, tasks and questions per digest. Tasks the model doesn't see
/// still reach the action plan ([`postcheck_action_plan`] puts them back).
pub fn render_digests(digests: &[MeetingDigest], t0: u64, budget: usize) -> String {
    let fits = |r: &String| estimate_tokens(r) <= budget;
    let attempts = (1..=6)
        .rev()
        .map(|cap| (cap, 1, usize::MAX))
        .chain((2..=12).map(|stride| (1, stride, usize::MAX)))
        .chain((1..=6).rev().map(|cap| (0, 1, cap)));
    for (points_cap, stride, items_cap) in
        std::iter::once((usize::MAX, 1, usize::MAX)).chain(attempts)
    {
        let r = render_with(digests, t0, points_cap, stride, items_cap);
        if fits(&r) {
            return r;
        }
    }
    truncate_to_tokens(&render_with(digests, t0, 0, 1, 1), budget)
}

/// What an action on a meeting note reads.
pub struct MeetingInput {
    pub text: String,
    /// Every task the digests found; an action plan must keep them all.
    pub digest_actions: Vec<DigestAction>,
}

/// [`compose_meeting_input_within`] the 8K budgets (the tests' shorthand).
#[cfg(test)]
pub fn compose_meeting_input(note: &Note) -> MeetingInput {
    compose_meeting_input_within(note, FinalBudget::DEFAULT)
}

/// Assemble a meeting note for the final call: the raw transcript when it
/// all fits `budget.single_pass` ([`fits_single_pass_within`]), else the
/// digests plus the raw tail — at most `budget.input` either way (callers
/// run [`prepare_final`] first so the tail is already short; a tail that
/// isn't keeps its end).
pub fn compose_meeting_input_within(note: &Note, budget: FinalBudget) -> MeetingInput {
    let mut text = people_lines(note);
    // The calendar invite's agenda (calendar.rs), when the meeting had one.
    if let Some(invite) = crate::calendar::invite_context(note) {
        text.push_str(&invite);
    }
    let typed = note.content.trim();
    let digest_actions = digest_actions(&note.digests);
    if fits_single_pass_within(note, budget.single_pass) {
        if !typed.is_empty() {
            text.push_str(&format!("\n## Notes typed during the meeting\n{typed}\n"));
        }
        text.push_str(&format!(
            "\n## Meeting Transcript\n{}",
            transcript_lines(&note.speakers, &note.transcript)
        ));
        return MeetingInput {
            text,
            digest_actions,
        };
    }

    let t0 = meeting_t0(note);
    let end = note.transcript.last().map(|s| s.ts).unwrap_or(t0);
    text.push_str(&format!(
        "Meeting length: {}\n",
        clock(end.saturating_sub(t0))
    ));
    if !typed.is_empty() {
        text.push_str(&format!(
            "\n## Notes typed during the meeting\n{}\n",
            truncate_to_tokens(typed, FINAL_NOTES_TOKENS)
        ));
    }
    let upto = crate::notes::digested_upto(note).min(note.transcript.len());
    let tail = &note.transcript[upto..];
    let tail_lines = keep_last_tokens(&transcript_lines(&note.speakers, tail), FINAL_TAIL_TOKENS);
    if let (Some(first), Some(last)) = (note.digests.first(), note.digests.last()) {
        let header = format!(
            "\n## Digest of the meeting so far ({}\u{2013}{})\n",
            clock(first.start_ts.saturating_sub(t0)),
            clock(last.end_ts.saturating_sub(t0))
        );
        let used =
            estimate_tokens(&text) + estimate_tokens(&header) + estimate_tokens(&tail_lines) + 30;
        let room = budget.input.saturating_sub(used);
        text.push_str(&header);
        text.push_str(&render_digests(&note.digests, t0, room));
    }
    if let Some(first) = tail.iter().find(|s| s.is_talk()) {
        text.push_str(&format!(
            "\n## Transcript of the last part ({}\u{2013}{})\n{tail_lines}",
            clock(first.ts.saturating_sub(t0)),
            clock(end.saturating_sub(t0))
        ));
    }
    MeetingInput {
        text,
        digest_actions,
    }
}

/// The meeting part of a note's chat context ("Ask anything…"), within
/// `budget` tokens: the whole transcript when it fits, else the digests and
/// as much of the latest transcript as fits.
pub fn ask_context(note: &Note, budget: usize) -> String {
    let all = transcript_lines(&note.speakers, &note.transcript);
    if estimate_tokens(&all) <= budget || note.digests.is_empty() {
        return format!("Meeting transcript:\n{}", keep_last_tokens(&all, budget));
    }
    let t0 = meeting_t0(note);
    let digests = render_digests(&note.digests, t0, budget / 2);
    let upto = crate::notes::digested_upto(note).min(note.transcript.len());
    let rest = budget.saturating_sub(estimate_tokens(&digests));
    format!(
        "Meeting digest:\n{digests}\nLatest transcript:\n{}",
        keep_last_tokens(&transcript_lines(&note.speakers, &note.transcript[upto..]), rest)
    )
}

// ---- Checking the action plan ----

fn is_task(line: &str) -> bool {
    let t = line.trim_start();
    ["- [ ]", "- [x]", "- [X]", "* [ ]", "* [x]"]
        .iter()
        .any(|p| t.starts_with(p))
}

/// Content words of a task, for "is this the same task" checks.
fn task_words(task: &str) -> HashSet<String> {
    const STOP: [&str; 14] = [
        "the", "a", "an", "to", "and", "for", "of", "on", "by", "with", "due", "will", "our",
        "their",
    ];
    words(task)
        .into_iter()
        .filter(|w| w.len() > 2 && !STOP.contains(&w.as_str()))
        .collect()
}

/// Is `task` one of `present` (most of its words appear in one of them)?
fn covered(task: &str, present: &[HashSet<String>]) -> bool {
    let want = task_words(task);
    if want.is_empty() {
        return true;
    }
    present.iter().any(|p| {
        let hit = want.iter().filter(|w| p.contains(*w)).count();
        hit * 10 >= want.len() * 6
    })
}

fn task_line(task: &str, due: &str) -> String {
    if due.is_empty() {
        format!("- [ ] {task}")
    } else {
        format!("- [ ] {task} (due: {due})")
    }
}

fn is_plan_title(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("action") && !t.contains("unassigned")
}

/// A reply split at its "## " headings (the "### " owner headings stay in
/// the sections' bodies).
struct PlanDoc {
    intro: Vec<String>,
    sections: Vec<(String, Vec<String>)>,
}

impl PlanDoc {
    fn parse(reply: &str) -> Self {
        let mut doc = PlanDoc {
            intro: Vec::new(),
            sections: Vec::new(),
        };
        for line in reply.lines() {
            if let Some(title) = line.trim().strip_prefix("## ") {
                doc.sections.push((title.trim().to_string(), Vec::new()));
            } else if let Some((_, body)) = doc.sections.last_mut() {
                body.push(line.to_string());
            } else {
                doc.intro.push(line.to_string());
            }
        }
        doc
    }

    fn render(&self) -> String {
        let mut out: Vec<String> = self.intro.clone();
        for (title, body) in &self.sections {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("## {title}"));
            let mut body = body.clone();
            while body.last().is_some_and(|l| l.trim().is_empty()) {
                body.pop();
            }
            out.extend(body);
        }
        out.join("\n").trim().to_string()
    }

    fn section(&mut self, pred: impl Fn(&str) -> bool, create: &str, before: &[&str]) -> usize {
        if let Some(i) = self.sections.iter().position(|(t, _)| pred(t)) {
            return i;
        }
        let at = self
            .sections
            .iter()
            .position(|(t, _)| {
                let t = t.to_lowercase();
                before.iter().any(|b| t.contains(b))
            })
            .unwrap_or(self.sections.len());
        self.sections.insert(at, (create.to_string(), Vec::new()));
        at
    }
}

/// Deterministic checks on an action-plan reply (the layout of
/// `llm::ACTION_PLAN_DEFAULT_FRAGMENT`): a "### Owner" heading must be "You",
/// "Everyone", an attendee or a name said in `source`, else its tasks move to
/// "## Unassigned"; a "(due: …)" must use words from `source`, else it goes;
/// and any digest task the reply lost is put back under its owner.
pub fn postcheck_action_plan(
    reply: &str,
    attendees: &[String],
    source: &str,
    digest_actions: &[DigestAction],
) -> String {
    let source_words: HashSet<String> = words(source).into_iter().collect();
    let mut doc = PlanDoc::parse(reply);
    let mut moved: Vec<String> = Vec::new();

    // Owner headings in the action-plan section.
    if let Some(i) = doc.sections.iter().position(|(t, _)| is_plan_title(t)) {
        let body = std::mem::take(&mut doc.sections[i].1);
        let mut kept = Vec::new();
        let mut dropping = false;
        for line in body {
            if let Some(name) = line.trim().strip_prefix("### ") {
                let owner = resolve_owner(name, attendees, source);
                dropping = owner == UNASSIGNED;
                if !dropping {
                    kept.push(format!("### {owner}"));
                }
            } else if dropping {
                if is_task(&line) {
                    moved.push(line.trim().to_string());
                }
            } else {
                kept.push(line);
            }
        }
        doc.sections[i].1 = kept;
    }

    // Deadlines that weren't said.
    for line in doc
        .sections
        .iter_mut()
        .flat_map(|(_, body)| body.iter_mut())
        .chain(moved.iter_mut())
    {
        if !is_task(line) {
            continue;
        }
        if let Some(c) = due_regex().captures(line) {
            if supported_due(&c[1], &source_words).is_empty() {
                let start = c.get(0).map(|m| m.start()).unwrap_or(line.len());
                line.truncate(start);
            }
        }
    }

    // Lost digest tasks go back, under their owner.
    let present: Vec<HashSet<String>> = doc
        .sections
        .iter()
        .flat_map(|(_, body)| body.iter())
        .chain(moved.iter())
        .filter(|l| is_task(l))
        .map(|l| task_words(&strip_bullet(l)))
        .collect();
    let mut lost: Vec<&DigestAction> = Vec::new();
    for a in digest_actions {
        if !covered(&a.task, &present) && !lost.iter().any(|b| b.task == a.task) {
            lost.push(a);
        }
    }
    for a in lost.iter().filter(|a| a.owner != UNASSIGNED) {
        let plan = doc.section(
            is_plan_title,
            "Action plan",
            &["decision", "question", "unassigned"],
        );
        let body = &mut doc.sections[plan].1;
        let heading = format!("### {}", a.owner);
        let at = match body
            .iter()
            .position(|l| l.trim().eq_ignore_ascii_case(&heading))
        {
            Some(h) => {
                let next = body[h + 1..]
                    .iter()
                    .position(|l| l.trim().starts_with("### "))
                    .map(|p| h + 1 + p)
                    .unwrap_or(body.len());
                let mut at = next;
                while at > h + 1 && body[at - 1].trim().is_empty() {
                    at -= 1;
                }
                at
            }
            None => {
                while body.last().is_some_and(|l| l.trim().is_empty()) {
                    body.pop();
                }
                if !body.is_empty() {
                    body.push(String::new());
                }
                body.push(heading);
                body.len()
            }
        };
        body.insert(at, task_line(&a.task, &a.due));
    }
    moved.extend(
        lost.iter()
            .filter(|a| a.owner == UNASSIGNED)
            .map(|a| task_line(&a.task, &a.due)),
    );
    if !moved.is_empty() {
        let un = doc.section(
            |t: &str| t.to_lowercase().contains("unassigned"),
            UNASSIGNED,
            &[],
        );
        let body = &mut doc.sections[un].1;
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        body.extend(moved);
    }
    doc.render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::meeting_summary::digest::{next_window, Plan};
    use crate::meetings::meeting_summary::{fits_single_pass, FINAL_INPUT_TOKENS, SINGLE_PASS_TOKENS};
    use crate::meetings::meeting_summary::tests::{digest, note_with, talk};

    #[test]
    fn final_input_stays_in_budget_for_any_length() {
        // A 2-hour and an 8-hour meeting, digested as they went (live) and
        // then as `prepare_final` would, with a long tail of typed notes.
        for (hours, words) in [(2u64, 40), (8, 40), (2, 47)] {
            let n = (hours * 3600 / 15) as usize;
            let mut note = note_with(talk(n, words, 15));
            let mut from = 0;
            let plans = [
                Plan::Live,
                Plan::Final {
                    keep: FINAL_TAIL_TOKENS,
                },
            ];
            for plan in plans {
                while let Some((f, to)) = next_window(&note.transcript, from, plan) {
                    let start = note.transcript[f].ts;
                    let points: Vec<String> = (0..6)
                        .map(|i| {
                            format!("a fairly detailed key point number {i} about the discussion")
                        })
                        .collect();
                    let points: Vec<&str> = points.iter().map(|s| s.as_str()).collect();
                    let task = format!("task from {f}");
                    note.digests
                        .push(digest(f, to, start, &points, &[("Alice", &task, "")]));
                    from = to;
                }
            }
            note.content = "typed notes ".repeat(400);
            let input = compose_meeting_input(&note);
            let tokens = estimate_tokens(&input.text);
            assert!(
                tokens <= FINAL_INPUT_TOKENS + 50,
                "{hours}h: {tokens} tokens"
            );
            assert!(input.text.contains("## Digest of the meeting so far"));
            assert_eq!(
                input.text.contains("## Transcript of the last part"),
                from < note.transcript.len()
            );
            // The raw opening of the meeting is not in it.
            assert!(!input.text.contains(&note.transcript[0].text));
            assert_eq!(input.digest_actions.len(), note.digests.len());
        }
    }

    #[test]
    fn a_bigger_on_device_context_reads_more_of_the_meeting_raw() {
        // ~9,000 tokens of talk: digested at 8K, one raw pass at 32K.
        let note = note_with(talk(150, 30, 10));
        let tokens = estimate_tokens(&transcript_lines(&note.speakers, &note.transcript));
        assert!(tokens > SINGLE_PASS_TOKENS * 2 && tokens < SINGLE_PASS_TOKENS * 4, "{tokens}");
        assert!(!fits_single_pass(&note));
        assert!(!fits_single_pass_within(&note, FinalBudget::scaled(2).single_pass));
        let big = FinalBudget::scaled(4);
        assert!(fits_single_pass_within(&note, big.single_pass));
        let input = compose_meeting_input_within(&note, big);
        assert!(input.text.contains("## Meeting Transcript"));
        assert!(input.text.contains(&note.transcript[0].text), "the opening, raw");
        // Within the 32K window with the prompt and the reply's room.
        assert!(estimate_tokens(&input.text) + 3_600 <= 32_768);
        // Never more than 4×, never less than the 8K budgets.
        assert_eq!(FinalBudget::scaled(9), big);
        assert_eq!(FinalBudget::scaled(0), FinalBudget::DEFAULT);
        assert_eq!((big.single_pass, big.input), (14_000, 18_000));
    }

    #[test]
    fn short_meeting_keeps_the_single_pass() {
        let note = note_with(talk(20, 20, 15));
        assert!(fits_single_pass(&note));
        let input = compose_meeting_input(&note);
        assert!(input.text.contains("## Meeting Transcript"));
        assert!(input.text.contains(&note.transcript[0].text));
    }

    #[test]
    fn postcheck_moves_invented_owners_and_restores_lost_tasks() {
        let attendees = vec!["Alice".to_string(), "Bob Stone".to_string()];
        let source = "Them: Alice will send the revised budget by Friday. You: Bob, the venue?";
        let reply = "The team planned the offsite.\n\n## Action plan\n\n### Alice\n- [ ] Send the revised budget (due: Friday)\n\n### Mallory\n- [ ] Order the pizza (due: Tuesday)\n\n## Decisions\n- Offsite in May\n";
        let digest_actions = vec![
            DigestAction {
                owner: "Alice".into(),
                task: "send the revised budget".into(),
                due: "Friday".into(),
            },
            DigestAction {
                owner: "Bob Stone".into(),
                task: "book the venue".into(),
                due: String::new(),
            },
            DigestAction {
                owner: UNASSIGNED.into(),
                task: "update the wiki".into(),
                due: String::new(),
            },
        ];
        let out = postcheck_action_plan(reply, &attendees, source, &digest_actions);
        assert!(!out.contains("Mallory"), "{out}");
        assert!(
            out.contains("### Alice\n- [ ] Send the revised budget (due: Friday)"),
            "{out}"
        );
        assert!(out.contains("### Bob Stone\n- [ ] book the venue"), "{out}");
        // Mallory's task lost its made-up deadline and moved to Unassigned,
        // with the lost unassigned task.
        let un = out.split("## Unassigned").nth(1).expect(&out);
        assert!(
            un.contains("- [ ] Order the pizza\n") || un.contains("- [ ] Order the pizza"),
            "{out}"
        );
        assert!(!un.contains("Tuesday"), "{out}");
        assert!(un.contains("- [ ] update the wiki"), "{out}");
        // The action plan comes before Decisions, Unassigned last.
        let (plan, dec, una) = (
            out.find("## Action plan").unwrap(),
            out.find("## Decisions").unwrap(),
            out.find("## Unassigned").unwrap(),
        );
        assert!(plan < dec && dec < una, "{out}");
        // Nothing to fix: unchanged.
        let clean = "Summary.\n\n## Action plan\n\n### You\n- [ ] Send the deck";
        let you = [DigestAction {
            owner: "You".into(),
            task: "send the deck".into(),
            due: String::new(),
        }];
        assert_eq!(
            postcheck_action_plan(clean, &attendees, "You: I'll send the deck", &you),
            clean
        );
    }

    #[test]
    fn ask_context_is_bounded() {
        let mut note = note_with(talk(600, 40, 15));
        assert!(estimate_tokens(&ask_context(&note, 3_000)) <= 3_050);
        note.digests.push(digest(
            0,
            300,
            1000,
            &["early point"],
            &[("Alice", "do a thing", "")],
        ));
        let c = ask_context(&note, 3_000);
        assert!(c.contains("early point"));
        assert!(estimate_tokens(&c) <= 3_100);
    }
}
