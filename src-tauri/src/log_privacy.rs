//! **No user content in the log at `info` and above** — a lint-style test.
//!
//! Yap's log file (`<data>/logs/yap.log.*`, `lib.rs init_logging`) is written
//! at `info`, and people attach it to bug reports. So `info!`, `warn!` and
//! `error!` never carry what someone dictated, typed or was told: no
//! transcripts or cleaned text, chat messages or answers, memory items,
//! notes, meeting lines, About you, calendar titles, search queries or the
//! clipboard — only lengths, counts, ids and outcomes. The text itself may go
//! to `debug!` (Settings → Privacy & data → Debug logging, which warns), and
//! never an API key or a token at any level.
//!
//! This test reads every Rust source file and fails on an `info!`/`warn!`/
//! `error!` call that formats a variable named like user content (`text =
//! %…`, `{transcript}`, `"…{}", answer`). A false alarm: rename the variable
//! to say what it holds (`chars`, `len`, `id`).

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// Variable and field names that hold what a person dictated, typed or
    /// was told. (Not `title`: card and notice titles are Yap's own words.)
    /// (And a screenshot's pixels or its data URL: Snip to Chat logs sizes.)
    const CONTENT: &str = "text|transcript|raw|raw_text|corrected|cleaned|cleaned_text|final_text|content|contents|answer|reply|question|query|prompt|said|heard|spoken|clipboard|clip_text|about|about_me|note_text|fact|facts|selection|before|after|partial|partials|utterance|dictation|message_text|user_text|body_text|bgra|rgba|pixels|png|data_url|url_data";

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }

    /// Every `info!(…)`/`warn!(…)`/`error!(…)` call in `src`, as (line, text):
    /// balanced parentheses, string literals skipped over.
    fn log_calls(src: &str) -> Vec<(usize, String)> {
        let start = regex::Regex::new(r"(?:^|[^A-Za-z0-9_:])((?:tracing::|log::)?(?:info|warn|error)!\s*\()").unwrap();
        let bytes = src.as_bytes();
        let mut calls = Vec::new();
        for c in start.captures_iter(src) {
            let m = c.get(1).unwrap();
            let mut i = m.end();
            let mut depth = 1;
            let mut in_str = false;
            while i < bytes.len() && depth > 0 {
                let c = bytes[i];
                if in_str {
                    if c == b'\\' {
                        i += 1;
                    } else if c == b'"' {
                        in_str = false;
                    }
                } else if c == b'"' {
                    in_str = true;
                } else if c == b'(' {
                    depth += 1;
                } else if c == b')' {
                    depth -= 1;
                }
                i += 1;
            }
            let line = src[..m.start()].matches('\n').count() + 1;
            calls.push((line, src[m.start()..i.min(src.len())].to_string()));
        }
        calls
    }

    /// Why a log call is formatting user content, if it is.
    fn content_in(call: &str) -> Option<String> {
        let rules = [
            // A field shown with Display/Debug: `text = %out`, `answer = ?a`.
            format!(r"\b({CONTENT})\s*=\s*[%?]"),
            // A field shorthand: `%text`, `?transcript`.
            format!(r"[%?]({CONTENT})\b"),
            // An inline format argument: `{text}`, `{answer:?}`.
            format!(r"\{{({CONTENT})(:[^}}]*)?\}}"),
            // A positional argument that's the variable itself: `"…{}", text)`.
            format!(r",\s*&?({CONTENT})(\.(as_str|trim)\(\))?\s*[,)]"),
        ];
        for rule in rules {
            let re = regex::Regex::new(&rule).unwrap();
            if let Some(c) = re.captures(call) {
                return Some(c[1].to_string());
            }
        }
        None
    }

    #[test]
    fn the_rules_catch_content_and_let_the_rest_through() {
        for bad in [
            r#"tracing::info!(text = %corrected, "Transcript")"#,
            r#"tracing::info!(text = %out, "Rewrite")"#,
            r#"info!("Transcript: {}", transcript)"#,
            r#"tracing::warn!("asked {question}")"#,
            r#"tracing::error!(?answer, "no answer")"#,
            r#"tracing::info!("search: {}", &query)"#,
            r#"tracing::info!(about = %cfg.about_me, "About you")"#,
        ] {
            assert!(content_in(bad).is_some(), "{bad}");
        }
        for ok in [
            r#"tracing::info!(chars = corrected.chars().count(), "Transcript")"#,
            r#"tracing::info!(len = text.len(), "Pasted")"#,
            r#"tracing::warn!("Rewrite failed: {}", e)"#,
            r#"tracing::info!(card = %id, action, "bar: card answered")"#,
            r#"tracing::info!(note_id, title = %warning.title, "meeting guard: length warning")"#,
            r#"tracing::info!("alert: {}", alert.title)"#,
        ] {
            assert_eq!(content_in(ok), None, "{ok}");
        }
        let calls = log_calls("fn f() {\n    tracing::info!(\"a (b) {}\", x);\n    debug!(text = %t);\n    warn!(\"c\")\n}");
        assert_eq!(calls.iter().map(|(l, _)| *l).collect::<Vec<_>>(), [2, 4]);
        assert_eq!(calls[0].1, "tracing::info!(\"a (b) {}\", x)");
    }

    #[test]
    fn no_user_content_is_logged_at_info_or_above() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        assert!(files.len() > 50, "found the sources");
        let mut found = Vec::new();
        for file in &files {
            if file.ends_with("log_privacy.rs") {
                continue; // this file's own examples
            }
            let src = std::fs::read_to_string(file).unwrap();
            for (line, call) in log_calls(&src) {
                if let Some(what) = content_in(&call) {
                    let rel = file.strip_prefix(&root).unwrap_or(file).display();
                    found.push(format!("{rel}:{line}: `{what}` in {}", call.split_whitespace().collect::<Vec<_>>().join(" ")));
                }
            }
        }
        assert!(
            found.is_empty(),
            "User content logged at info/warn/error (log its length or an id instead; the text may go to debug!):\n{}",
            found.join("\n")
        );
    }
}
