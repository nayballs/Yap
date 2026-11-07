//! **What a job changed in a file, line by line**, for the job's tab:
//! "Files changed" opens each file to a compact diff. Pure.
//!
//! - **Edit / MultiEdit**: their `old_string` and `new_string` (MultiEdit's
//!   `edits[]`), lines removed and added with a few unchanged ones around
//!   them; MultiEdit's edits one after another.
//! - **Write**: "New file · N lines", "Rewritten · N lines" when the file
//!   was there before (the backup manifest knows), or "Written" when Yap
//!   can't tell (no backups were made): its first lines.
//! - **Caps**: [`EDIT_MAX`] lines an edit, [`JOB_MAX`] a job; the rest is
//!   counted ("…N more lines"), and every line at most [`LINE_MAX`]
//!   characters.
//!
//! A diff goes in the job's `yap-job` event and its log, never in Yap's own
//! log (it's the person's files).

use serde::Serialize;
use serde_json::Value;

/// The most lines one edit shows.
pub const EDIT_MAX: usize = 200;
/// The most lines a whole job's diffs show.
pub const JOB_MAX: usize = 2_000;
/// The most characters of a line.
pub const LINE_MAX: usize = 300;
/// Unchanged lines kept around a change.
const CONTEXT: usize = 3;
/// The largest table the line matcher fills (old lines × new lines); past
/// it, the old lines are all removed and the new ones all added.
const MATCH_CELLS: usize = 1_000_000;

/// **One change to a file**, as the tab shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    /// The file, as the job's lists say it (relative to its folder).
    pub file: String,
    /// "edit", "new" (a file it made), "rewrite" (a file there before,
    /// written whole) or "write" (written whole; whether it was there
    /// before isn't known).
    pub kind: String,
    /// The lines, each with its mark first: `-` removed, `+` added, ` `
    /// unchanged; a lone `@` is a gap between two parts.
    pub lines: Vec<String>,
    /// Lines added and removed in all (shown or not).
    pub added: usize,
    pub removed: usize,
    /// Lines not shown (past the caps).
    #[serde(skip_serializing_if = "is_zero")]
    pub more: usize,
    /// Every match replaced (Edit's `replace_all`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// One line's place in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same,
    Removed,
    Added,
}

/// `s` as lines (no line ends; none for "").
fn lines_of(s: &str) -> Vec<&str> {
    s.lines().collect()
}

/// Old lines and new lines matched up: each line once, in order.
fn matched<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Op, &'a str)> {
    // The same lines at both ends, matched at once.
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..].iter().rev().zip(new[head..].iter().rev()).take_while(|(a, b)| a == b).count();
    let (a, b) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let mut out: Vec<(Op, &str)> = old[..head].iter().map(|l| (Op::Same, *l)).collect();
    if a.len().saturating_mul(b.len()) > MATCH_CELLS {
        out.extend(a.iter().map(|l| (Op::Removed, *l)));
        out.extend(b.iter().map(|l| (Op::Added, *l)));
    } else {
        // The longest common run of lines (a table from the end).
        let w = b.len() + 1;
        let mut t = vec![0u32; (a.len() + 1) * w];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                t[i * w + j] = if a[i] == b[j] { t[(i + 1) * w + j + 1] + 1 } else { t[(i + 1) * w + j].max(t[i * w + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                out.push((Op::Same, a[i]));
                i += 1;
                j += 1;
            } else if j < b.len() && (i == a.len() || t[i * w + j + 1] >= t[(i + 1) * w + j]) {
                // Removed lines first when both would do.
                if i < a.len() && t[(i + 1) * w + j] == t[i * w + j + 1] {
                    out.push((Op::Removed, a[i]));
                    i += 1;
                } else {
                    out.push((Op::Added, b[j]));
                    j += 1;
                }
            } else {
                out.push((Op::Removed, a[i]));
                i += 1;
            }
        }
    }
    out.extend(old[old.len() - tail..].iter().map(|l| (Op::Same, *l)));
    out
}

/// A line with its mark, cut to [`LINE_MAX`].
fn marked(op: Op, line: &str) -> String {
    let mark = match op {
        Op::Same => ' ',
        Op::Removed => '-',
        Op::Added => '+',
    };
    format!("{mark}{}", super::steps::clip(line.trim_end_matches('\r'), LINE_MAX))
}

/// One replacement's lines: the changes with [`CONTEXT`] unchanged lines
/// around them, a gap (`@`) where more were left out; and the lines added
/// and removed.
fn hunk(old: &str, new: &str) -> (Vec<String>, usize, usize) {
    let ops = matched(&lines_of(old), &lines_of(new));
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, (op, _))| *op != Op::Same).map(|(i, _)| i).collect();
    let added = ops.iter().filter(|(op, _)| *op == Op::Added).count();
    let removed = ops.iter().filter(|(op, _)| *op == Op::Removed).count();
    let mut out = Vec::new();
    let mut last: Option<usize> = None;
    for (i, (op, line)) in ops.iter().enumerate() {
        let near = changed.iter().any(|&c| c.abs_diff(i) <= CONTEXT);
        if !near {
            continue;
        }
        if last.is_some_and(|l| i > l + 1) {
            out.push("@".to_string());
        }
        out.push(marked(*op, line));
        last = Some(i);
    }
    (out, added, removed)
}

/// **An Edit or MultiEdit's diff** of `file` from its input; `None` for
/// another tool or no change.
pub fn of_edit(tool: &str, input: &Value, file: &str) -> Option<FileDiff> {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let edits: Vec<(String, String, bool)> = match tool {
        "Edit" => vec![(s(input, "old_string"), s(input, "new_string"), input.get("replace_all").and_then(Value::as_bool).unwrap_or(false))],
        "MultiEdit" => input
            .get("edits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|e| (s(e, "old_string"), s(e, "new_string"), e.get("replace_all").and_then(Value::as_bool).unwrap_or(false)))
            .collect(),
        _ => return None,
    };
    let mut d = FileDiff { file: file.to_string(), kind: "edit".into(), ..Default::default() };
    for (old, new, all) in &edits {
        let (lines, added, removed) = hunk(old, new);
        if lines.is_empty() {
            continue;
        }
        if !d.lines.is_empty() {
            d.lines.push("@".into());
        }
        d.lines.extend(lines);
        d.added += added;
        d.removed += removed;
        d.all |= *all;
    }
    (!d.lines.is_empty()).then_some(d)
}

/// **A Write's diff** of `file`: its `content`'s lines, all added; `kind`
/// "new", "rewrite" or "write".
pub fn of_write(file: &str, content: &str, kind: &str) -> FileDiff {
    let lines: Vec<String> = lines_of(content).into_iter().map(|l| marked(Op::Added, l)).collect();
    FileDiff { file: file.to_string(), kind: kind.to_string(), added: lines.len(), lines, ..Default::default() }
}

/// `d` cut to [`EDIT_MAX`] lines and the `left` the job has (gaps not
/// counted); the lines it keeps.
pub fn capped(mut d: FileDiff, left: usize) -> (FileDiff, usize) {
    let max = EDIT_MAX.min(left);
    let total = d.lines.iter().filter(|l| *l != "@").count();
    if total <= max {
        return (d, total);
    }
    let mut kept = 0;
    let mut end = 0;
    for (i, l) in d.lines.iter().enumerate() {
        if l != "@" {
            if kept == max {
                break;
            }
            kept += 1;
        }
        end = i + 1;
    }
    d.lines.truncate(end);
    while d.lines.last().is_some_and(|l| l == "@") {
        d.lines.pop();
    }
    d.more += total - kept;
    (d, kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_edit_is_its_lines_removed_and_added_with_a_few_around_them() {
        let old = "fn water(bed) {\n  bed.water += 1;\n  return bed;\n}";
        let new = "fn water(bed, litres) {\n  bed.water += litres;\n  return bed;\n}";
        let d = of_edit("Edit", &json!({ "old_string": old, "new_string": new }), "src/planner.js").unwrap();
        assert_eq!(
            d.lines,
            ["-fn water(bed) {", "-  bed.water += 1;", "+fn water(bed, litres) {", "+  bed.water += litres;", "   return bed;", " }"]
        );
        assert_eq!((d.added, d.removed, d.more, d.kind.as_str(), d.all), (2, 2, 0, "edit", false));
        // A line added in the middle of many: three either side, gaps where more were left out.
        let old: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        let mut new = old.clone();
        new.insert(10, "a new line".into());
        new[1] = "line two".into();
        let d = of_edit("Edit", &json!({ "old_string": old.join("\n"), "new_string": new.join("\n"), "replace_all": true }), "notes.md").unwrap();
        assert_eq!(d.lines[..6], [" line 1", "-line 2", "+line two", " line 3", " line 4", " line 5"]);
        assert_eq!(d.lines[6], "@");
        assert_eq!(d.lines[7..], [" line 8", " line 9", " line 10", "+a new line", " line 11", " line 12", " line 13"]);
        assert_eq!((d.added, d.removed, d.all), (2, 1, true));
        // Only added, only removed, nothing changed.
        assert_eq!(of_edit("Edit", &json!({ "old_string": "", "new_string": "a\nb" }), "x").unwrap().lines, ["+a", "+b"]);
        assert_eq!(of_edit("Edit", &json!({ "old_string": "a\r\nb\r\n", "new_string": "" }), "x").unwrap().lines, ["-a", "-b"]);
        assert_eq!(of_edit("Edit", &json!({ "old_string": "same", "new_string": "same" }), "x"), None);
        assert_eq!(of_edit("Read", &json!({}), "x"), None);
    }

    #[test]
    fn a_multiedit_is_its_edits_in_turn_and_a_write_its_lines() {
        let d = of_edit(
            "MultiEdit",
            &json!({ "edits": [
                { "old_string": "Draft", "new_string": "Final" },
                { "old_string": "same", "new_string": "same" },
                { "old_string": "tomato", "new_string": "tomatoes\npeppers" }
            ] }),
            "notes.md",
        )
        .unwrap();
        assert_eq!(d.lines, ["-Draft", "+Final", "@", "-tomato", "+tomatoes", "+peppers"]);
        assert_eq!((d.added, d.removed), (3, 2));
        let w = of_write("guide.md", "# Guide\n\nPlant in spring.\n", "new");
        assert_eq!(w.lines, ["+# Guide", "+", "+Plant in spring."]);
        assert_eq!((w.kind.as_str(), w.added, w.removed), ("new", 3, 0));
        // Long lines cut.
        let long = "x".repeat(LINE_MAX + 50);
        assert_eq!(of_write("a", &long, "write").lines[0].chars().count(), LINE_MAX + 1);
        // The event says only what it has.
        let v = serde_json::to_value(&w).unwrap();
        assert!(v.get("more").is_none() && v.get("all").is_none());
        assert_eq!(v["kind"], "new");
    }

    #[test]
    fn the_caps_count_what_they_leave_out() {
        let content: String = (0..500).map(|i| format!("row {i}\n")).collect();
        let (d, kept) = capped(of_write("big.csv", &content, "new"), JOB_MAX);
        assert_eq!((d.lines.len(), kept, d.more, d.added), (EDIT_MAX, EDIT_MAX, 300, 500));
        // The job's own cap: what's left of it.
        let (d, kept) = capped(of_write("big.csv", &content, "new"), 50);
        assert_eq!((d.lines.len(), kept, d.more), (50, 50, 450));
        let (d, kept) = capped(of_write("big.csv", &content, "new"), 0);
        assert_eq!((d.lines.len(), kept, d.more, d.added), (0, 0, 500, 500));
        // Gaps aren't lines, and never end one.
        let d = FileDiff { lines: vec!["-a".into(), "+b".into(), "@".into(), "-c".into()], ..Default::default() };
        let (d, kept) = capped(d, 2);
        assert_eq!((d.lines, kept, d.more), (vec!["-a".to_string(), "+b".to_string()], 2, 1));
        // Under the caps: as it was.
        let small = of_write("a", "one\ntwo", "new");
        assert_eq!(capped(small.clone(), JOB_MAX), (small, 2));
    }

    #[test]
    fn a_big_change_with_nothing_in_common_is_removed_then_added() {
        let old: String = (0..1_200).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..1_200).map(|i| format!("new {i}\n")).collect();
        let d = of_edit("Edit", &json!({ "old_string": old, "new_string": new }), "x").unwrap();
        assert_eq!((d.added, d.removed), (1_200, 1_200));
        assert_eq!((d.lines[0].as_str(), d.lines[1_200].as_str()), ("-old 0", "+new 0"));
    }
}
