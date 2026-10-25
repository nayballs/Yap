//! `--search`: meeting search measured on `evals/search/meetings.json` — a
//! made-up team's meetings and notes, and questions with the lines that
//! answer them.
//!
//! - **Retrieval** (no chat model needed): the corpus seeded into the run's
//!   notes, cut into passages as Yap cuts them (`search::index::prepared`),
//!   every passage and question read by the embedding model on Yap's own
//!   llamafile engine (`search::embedder::server_args`, `--embed`,
//!   `--embed-ngl`), then each question searched as Ask across meetings
//!   searches (`search::ask::find_in`, the run's clock at the corpus's
//!   `now`) three ways — keywords (BM25 per passage), meaning, and hybrid —
//!   beside Chat's own note search (`chat::tools::search_notes`, whole notes:
//!   what the Meetings view's Ask used until 25 Oct 2026). Recall@1/3/5 (a
//!   passage holding a gold line, or the gold note for notes), MRR, and the
//!   share whose first passage's cited line *is* a gold line.
//! - **Answers** (each `--model`): the questions asked through Chat's own
//!   turn with the meetings scope (`turn::chat_turn`, the vectors in the
//!   run's index, the embedding server answering the questions): does the
//!   answer cite, does a citation point at a passage with a gold line, and
//!   does a question with no answer say so. `--only` picks questions (their
//!   words or tags); `--max-total-tokens` caps a cloud model's spend.
//!
//! Writes `search.md` and `search.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::search::ask::{self, Found};
use crate::search::rank::{Mode, Tuning, TUNING};
use crate::search::store::Key;

#[derive(Debug, Deserialize)]
pub struct Corpus {
    pub now: String,
    pub notes: Vec<CNote>,
    pub questions: Vec<Question>,
}

#[derive(Debug, Deserialize)]
pub struct CNote {
    pub id: u64,
    pub title: String,
    /// When it starts (RFC 3339); lines follow 20 s apart.
    pub start: String,
    #[serde(default)]
    pub participants: Vec<String>,
    /// Voice id → name ("them-1": "Priya Shah").
    #[serde(default)]
    pub speakers: HashMap<String, String>,
    /// `[voice, words]`: "you", "them-1"…
    #[serde(default)]
    pub lines: Vec<(String, String)>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gold {
    pub note: u64,
    #[serde(default)]
    pub line: Option<usize>,
    /// "summary" | "content" (else any passage of the note, or the line's).
    #[serde(default)]
    pub field: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Question {
    pub q: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Empty: nothing in the corpus answers it.
    pub gold: Vec<Gold>,
}

pub fn load(path: &Path) -> Result<Corpus, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let c: Corpus = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for q in &c.questions {
        for g in &q.gold {
            let n = c.notes.iter().find(|n| n.id == g.note).ok_or_else(|| format!("\"{}\": no note {}", q.q, g.note))?;
            if g.line.is_some_and(|l| l >= n.lines.len()) {
                return Err(format!("\"{}\": note {} has no line {}", q.q, g.note, g.line.unwrap_or(0)));
            }
        }
    }
    Ok(c)
}

fn secs(rfc: &str) -> Result<i64, String> {
    chrono::DateTime::parse_from_rfc3339(rfc).map(|t| t.timestamp()).map_err(|e| format!("{rfc}: {e}"))
}

/// The corpus in the run's notes store; corpus id → store id.
pub fn seed(c: &Corpus) -> Result<HashMap<u64, u64>, String> {
    crate::notes::eval_reset();
    let mut ids = HashMap::new();
    for n in &c.notes {
        let start = secs(&n.start)? as u64;
        let meeting = !n.lines.is_empty();
        let note = crate::notes::create(&n.title, &n.notes, if meeting { "meeting" } else { "manual" }, if meeting { "Meetings" } else { "Personal" });
        let transcript: Vec<crate::notes::TranscriptSegment> = n
            .lines
            .iter()
            .enumerate()
            .map(|(i, (voice, text))| crate::notes::TranscriptSegment {
                source: if voice == "you" { "you".into() } else { "them".into() },
                speaker: if voice.starts_with("them-") { voice.clone() } else { String::new() },
                text: text.clone(),
                ts: start + 20 * i as u64,
                ..Default::default()
            })
            .collect();
        let speakers: crate::speakers::Speakers = n
            .speakers
            .iter()
            .map(|(id, name)| {
                (id.clone(), crate::speakers::Speaker { name: name.clone(), source: Some(crate::speakers::NameSource::User), locked: true, ..Default::default() })
            })
            .collect();
        crate::notes::modify(note.id, |x| {
            x.transcript = transcript;
            x.speakers = speakers;
            x.participants = n.participants.clone();
            x.enhanced_content = n.summary.clone();
            x.created_ts = start;
            if meeting {
                x.note_type = "meeting".into();
            }
            Ok(((), true))
        })?;
        ids.insert(n.id, note.id);
    }
    Ok(ids)
}

/// Does passage `p` hold an answer from `gold` (store ids)?
fn hits(p: &ask::Passage, gold: &[Gold], ids: &HashMap<u64, u64>) -> bool {
    gold.iter().any(|g| {
        ids.get(&g.note) == Some(&p.note)
            && match (&g.line, &g.field) {
                (Some(l), _) => p.field == crate::search::chunks::Field::Transcript && p.lines.contains(l),
                (None, Some(f)) => p.field.as_str() == f,
                (None, None) => true,
            }
    })
}

fn exact(p: &ask::Passage, gold: &[Gold], ids: &HashMap<u64, u64>) -> bool {
    gold.iter().any(|g| {
        ids.get(&g.note) == Some(&p.note)
            && match g.line {
                Some(l) => p.field == crate::search::chunks::Field::Transcript && p.line == l,
                None => g.field.as_deref().is_none_or(|f| p.field.as_str() == f),
            }
    })
}

fn recall(ranks: &[Option<usize>], k: usize) -> f64 {
    if ranks.is_empty() {
        return 0.0;
    }
    let hit = ranks.iter().filter(|r| r.is_some_and(|r| r < k)).count();
    (hit as f64 / ranks.len() as f64 * 1000.0).round() / 1000.0
}

fn mrr(ranks: &[Option<usize>]) -> f64 {
    if ranks.is_empty() {
        return 0.0;
    }
    let s: f64 = ranks.iter().map(|r| r.map_or(0.0, |r| 1.0 / (r as f64 + 1.0))).sum();
    (s / ranks.len() as f64 * 1000.0).round() / 1000.0
}

/// The run's embedding server: Yap's engine in embedding mode on `gguf`.
pub struct Server {
    child: std::process::Child,
    pub load_ms: u64,
    pub log: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        crate::search::embedder::use_server(None);
    }
}

pub async fn start(runtime: &Path, gguf: &Path, ngl: u32, run_dir: &Path) -> Result<Server, String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| e.to_string())?;
    let mut args = crate::search::embedder::server_args(gguf, port, std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(2).max(2));
    if let Some(i) = args.iter().position(|a| a == "-ngl") {
        args[i + 1] = ngl.to_string();
    }
    let log = run_dir.join("llamafile-embedder.log");
    let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(runtime);
    cmd.args(&args).current_dir(run_dir).stdin(std::process::Stdio::null()).stdout(file.try_clone().map_err(|e| e.to_string())?).stderr(file);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let t0 = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start the embedding server: {e}"))?;
    crate::shell::procs::end_with_yap(&child);
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().map_err(|e| e.to_string())?;
    for _ in 0..600 {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("the embedding server exited ({status}); see {}", log.display()));
        }
        if client.get(format!("http://127.0.0.1:{port}/health")).send().await.is_ok_and(|r| r.status().is_success()) {
            let url = format!("http://127.0.0.1:{port}/v1");
            crate::search::embedder::use_server(Some(url.clone()));
            return Ok(Server { child, load_ms: t0.elapsed().as_millis() as u64, log });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let _ = child.kill();
    Err("the embedding server wasn't ready after 5 minutes".to_string())
}

/// The embedding model: `--embed`, else the installed app's (read-only),
/// else `models_dir`'s, else downloaded there (verified).
pub async fn model(explicit: Option<&Path>, models_dir: &Path) -> Result<PathBuf, String> {
    use crate::search::embedder::{MODEL_FILE, MODEL_SHA256, MODEL_URL};
    if let Some(p) = explicit {
        return if p.is_file() { Ok(p.to_path_buf()) } else { Err(format!("no GGUF at {}", p.display())) };
    }
    if let Some(p) = dirs::data_dir().map(|d| d.join("yap").join("models").join("search").join(MODEL_FILE)).filter(|p| p.is_file()) {
        return Ok(p);
    }
    let dest = models_dir.join(MODEL_FILE);
    if !dest.is_file() {
        std::fs::create_dir_all(models_dir).map_err(|e| e.to_string())?;
        println!("Downloading {MODEL_FILE} into {} …", models_dir.display());
        crate::ai::local_llm::models::download_verified(MODEL_URL, &dest, MODEL_SHA256, "model", None).await?;
    }
    Ok(dest)
}

/// The retrieval half: every passage and question embedded, each question
/// searched each way.
pub async fn retrieval(c: &Corpus, ids: &HashMap<u64, u64>) -> Result<Value, String> {
    let notes = crate::notes::all();
    let prepared = crate::search::index::prepared(&notes);
    let passages: Vec<&crate::search::index::Prepared> = prepared.iter().flat_map(|v| v.iter()).collect();
    let words: usize = passages.iter().map(|p| p.embedded.split_whitespace().count()).sum();
    println!("  {} notes → {} passages (~{} words); embedding them …", notes.len(), passages.len(), words);
    let t0 = Instant::now();
    let mut vectors: HashMap<Key, Vec<f32>> = HashMap::new();
    for batch in passages.chunks(16) {
        let texts: Vec<String> = batch.iter().map(|p| p.embedded.clone()).collect();
        let v = crate::search::embedder::embed(&texts).await?;
        for (p, v) in batch.iter().zip(v) {
            vectors.insert(p.key, v);
        }
    }
    let index_ms = t0.elapsed().as_millis() as u64;
    // Into the run's index, as the background pass would put them.
    let items: Vec<(Key, Vec<f32>)> = vectors.iter().map(|(k, v)| (*k, v.clone())).collect();
    crate::search::index::with_store(|s| s.add(&items)).map_err(|e| e.to_string())?;
    let now = chrono::DateTime::parse_from_rfc3339(&c.now).map_err(|e| e.to_string())?.with_timezone(&chrono::Local);
    // Yap's three, then the hybrid's other tunings (docs/architecture/search.md).
    let modes: [(&str, Mode, Tuning); 7] = [
        ("keyword", Mode::Keyword, TUNING),
        ("meaning", Mode::Semantic, TUNING),
        ("hybrid", Mode::Hybrid, TUNING),
        ("hybrid: plain RRF, K 60, title words in full", Mode::Hybrid, Tuning { k: 60.0, weighted: false, title: 1.0 }),
        ("hybrid: plain RRF, K 20", Mode::Hybrid, Tuning { weighted: false, ..TUNING }),
        ("hybrid: weighted, K 60", Mode::Hybrid, Tuning { k: 60.0, ..TUNING }),
        ("hybrid: weighted, K 20", Mode::Hybrid, Tuning { k: 20.0, ..TUNING }),
    ];
    let mut ranks: HashMap<&str, Vec<Option<usize>>> = HashMap::new();
    let mut exacts: HashMap<&str, usize> = HashMap::new();
    let mut notes_ranks: Vec<Option<usize>> = Vec::new();
    let mut query_ms = Vec::new();
    let mut rows = Vec::new();
    let answerable: Vec<&Question> = c.questions.iter().filter(|q| !q.gold.is_empty()).collect();
    let mut abstain = Vec::new();
    for q in &c.questions {
        let tq = Instant::now();
        let qv = crate::search::embedder::embed_query(&q.q).await?;
        query_ms.push(tq.elapsed().as_millis() as u64);
        let sim = |k: &Key| vectors.get(k).map(|v| crate::search::rank::cosine(&qv, v));
        let mut row = json!({ "q": q.q, "tags": q.tags });
        for (name, mode, tuning) in modes {
            let f: Found = ask::find_in(&notes, &prepared, &q.q, &now, &sim, mode, tuning, 10);
            if q.gold.is_empty() {
                row[name] = json!({ "found": f.passages.len(), "dated": f.range.as_ref().map(|r| r.label.clone()) });
                if name == "hybrid" {
                    abstain.push(json!({ "q": q.q, "passages": f.passages.len(), "range": f.range.map(|r| r.label) }));
                }
                continue;
            }
            let r = f.passages.iter().position(|p| hits(p, &q.gold, ids));
            ranks.entry(name).or_default().push(r);
            if f.passages.first().is_some_and(|p| exact(p, &q.gold, ids)) {
                *exacts.entry(name).or_default() += 1;
            }
            let top = f.passages.first().map(|p| format!("{} · {} {}", p.title, p.field.as_str(), p.line)).unwrap_or_default();
            let ex = f.passages.first().is_some_and(|p| exact(p, &q.gold, ids));
            row[name] = json!({ "rank": r.map(|r| r + 1), "exact": ex, "top": top, "top_similarity": f.passages.first().and_then(|p| p.similarity) });
            // What the line would be if each of the first passage's lines
            // were read by the model too (a cost Yap doesn't pay): does it
            // find the gold line more often than the keywords do?
            if name == "hybrid" {
                if let Some(p) = f.passages.first().filter(|p| p.field == crate::search::chunks::Field::Transcript && p.lines.len() > 1) {
                    let texts: Vec<String> = p.text.split('\n').map(|l| crate::search::embedder::document(&p.title, l)).collect();
                    if texts.len() == p.lines.len() {
                        let lv = crate::search::embedder::embed(&texts).await?;
                        let best = lv
                            .iter()
                            .enumerate()
                            .max_by(|a, b| crate::search::rank::cosine(&qv, a.1).total_cmp(&crate::search::rank::cosine(&qv, b.1)))
                            .map(|(i, _)| p.lines[i]);
                        let by_meaning = q.gold.iter().any(|g| ids.get(&g.note) == Some(&p.note) && g.line.is_some() && g.line == best);
                        row["hybrid"]["exact_by_meaning"] = json!(by_meaning);
                        *exacts.entry("line by meaning").or_default() += usize::from(by_meaning);
                    }
                } else if ex {
                    *exacts.entry("line by meaning").or_default() += 1;
                }
            }
        }
        if !q.gold.is_empty() {
            let found = crate::chat::tools::search_notes(&q.q, 5);
            let want: Vec<u64> = q.gold.iter().filter_map(|g| ids.get(&g.note).copied()).collect();
            let r = found.iter().position(|(_, n)| want.contains(&n.id));
            notes_ranks.push(r);
            row["notes_keyword"] = json!({ "rank": r.map(|r| r + 1) });
        }
        rows.push(row);
    }
    let table = |name: &str, r: &[Option<usize>], exact: Option<usize>| {
        json!({
            "search": name,
            "at1": recall(r, 1),
            "at3": recall(r, 3),
            "at5": recall(r, 5),
            "mrr": mrr(r),
            "exact_line_at1": exact.map(|e| (e as f64 / answerable.len().max(1) as f64 * 1000.0).round() / 1000.0),
        })
    };
    let mut summary = vec![table("Chat's note search (whole notes, keywords)", &notes_ranks, None)];
    for (name, _, _) in modes {
        summary.push(table(name, ranks.get(name).map(Vec::as_slice).unwrap_or(&[]), Some(exacts.get(name).copied().unwrap_or(0))));
    }
    let mut sorted = query_ms.clone();
    sorted.sort_unstable();
    let result = json!({
        "notes": notes.len(),
        "passages": passages.len(),
        "passage_words": words,
        "questions": c.questions.len(),
        "answerable": answerable.len(),
        "index_ms": index_ms,
        "passages_per_s": (passages.len() as f64 / (index_ms.max(1) as f64 / 1000.0) * 10.0).round() / 10.0,
        "query_ms_median": sorted.get(sorted.len() / 2),
        "summary": summary,
        "exact_line_by_meaning_at1": (exacts.get("line by meaning").copied().unwrap_or(0) as f64 / answerable.len().max(1) as f64 * 1000.0).round() / 1000.0,
        "unanswerable": abstain,
        "rows": rows,
    });
    for s in result["summary"].as_array().into_iter().flatten() {
        println!(
            "  {:<44} recall@1 {:>5}  @3 {:>5}  @5 {:>5}  MRR {:>5}  exact line@1 {}",
            s["search"].as_str().unwrap_or(""),
            s["at1"],
            s["at3"],
            s["at5"],
            s["mrr"],
            s["exact_line_at1"]
        );
    }
    println!("  hybrid's first passage, its line picked by meaning instead: exact line@1 {}", result["exact_line_by_meaning_at1"]);
    Ok(result)
}

/// The settings an answer runs on: the Chat scope on the model, search by
/// meaning on.
pub fn settings(provider: &str, base_url: &str, key: &str, model: &str) -> crate::config::YapConfig {
    let mut cfg = crate::config::YapConfig {
        pp_provider: provider.to_string(),
        pp_base_url: base_url.to_string(),
        pp_api_key: key.to_string(),
        pp_model: model.to_string(),
        search_by_meaning: true,
        ..Default::default()
    };
    cfg.llm_scopes.insert(
        "chat".to_string(),
        crate::config::LlmScope {
            enabled: true,
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key: key.to_string(),
            prompt: super::runner::DEFAULT_PERSONA.to_string(),
            ..Default::default()
        },
    );
    cfg
}

fn picked<'a>(c: &'a Corpus, only: &[String]) -> Vec<&'a Question> {
    let only: Vec<String> = only.iter().map(|o| o.to_lowercase()).collect();
    c.questions
        .iter()
        .filter(|q| only.is_empty() || only.iter().any(|o| q.q.to_lowercase().contains(o) || q.tags.iter().any(|t| t == o)))
        .collect()
}

/// The answers half on one model (its settings already in place).
pub async fn answers(c: &Corpus, ids: &HashMap<u64, u64>, label: &str, only: &[String], cap: Option<u64>) -> Value {
    let mut rows = Vec::new();
    let mut used: u64 = 0;
    let (mut cited, mut gold_cited, mut passed, mut asked) = (0, 0, 0, 0);
    for q in picked(c, only) {
        if cap.is_some_and(|cap| used >= cap) {
            rows.push(json!({ "q": q.q, "skipped": "token cap reached (--max-total-tokens)" }));
            continue;
        }
        crate::chat::chats::eval_reset();
        super::record::set_context(json!({ "model": label, "scenario": format!("search: {}", q.q) }));
        super::record::begin_turn();
        let t0 = Instant::now();
        let turn = crate::chat::turn::chat_turn(None, None, &q.q, Some("meetings"), false, |_, _| {}, None, None).await;
        let stats = super::record::end_turn();
        used += stats.total_tokens;
        asked += 1;
        let turn = match turn {
            Ok(t) => t,
            Err(e) => {
                rows.push(json!({ "q": q.q, "error": super::record::redact(&e) }));
                continue;
            }
        };
        // The passages it was given (the search is the same again).
        let found = ask::find(&q.q, ask::ASK_PASSAGES).await;
        let nums = ask::cited_numbers(&turn.reply);
        let cites_gold = nums.iter().any(|n| found.passages.iter().any(|p| p.n == *n && hits(p, &q.gold, ids)));
        // No answer there: it cites nothing and says so (Yap's own note
        // under an uncited answer isn't the model saying so).
        let own = turn.reply.replace(ask::UNCITED_NOTE, "");
        let pass = if q.gold.is_empty() { nums.is_empty() && !ask::needs_uncited_note(&own, 1) } else { cites_gold };
        cited += usize::from(!nums.is_empty());
        gold_cited += usize::from(cites_gold);
        passed += usize::from(pass);
        let gold_found = found.passages.iter().any(|p| hits(p, &q.gold, ids));
        rows.push(json!({
            "q": q.q,
            "tags": q.tags,
            "pass": pass,
            "cites": nums,
            "cites_gold": cites_gold,
            "gold_in_passages": gold_found,
            "uncited_note": turn.reply.contains(ask::UNCITED_NOTE),
            "reply": turn.reply,
            "ms": t0.elapsed().as_millis() as u64,
            "tokens": stats.total_tokens,
            "post_fixes": stats.post_fixes,
        }));
        println!(
            "  {} {:<60} cites {:?}{}",
            if pass { "PASS" } else { "FAIL" },
            q.q.chars().take(60).collect::<String>(),
            nums,
            if cites_gold { " (a gold line)" } else { "" }
        );
    }
    json!({
        "asked": asked,
        "passed": passed,
        "cited": cited,
        "gold_cited": gold_cited,
        "tokens": used,
        "rows": rows,
    })
}

/// `search.md`.
pub fn markdown(retrieval: &Value, answers: &[(String, Value)]) -> String {
    let mut md = String::from("# Meeting search\n\n");
    md.push_str(&format!(
        "{} notes → {} passages (~{} words); embedded in {} ms ({} passages/s); a question {} ms (median).\n\n",
        retrieval["notes"], retrieval["passages"], retrieval["passage_words"], retrieval["index_ms"], retrieval["passages_per_s"], retrieval["query_ms_median"]
    ));
    md.push_str(&format!("{} questions with an answer ({} in all):\n\n| Search | Recall@1 | @3 | @5 | MRR | Cited line exact @1 |\n|---|---|---|---|---|---|\n", retrieval["answerable"], retrieval["questions"]));
    for s in retrieval["summary"].as_array().into_iter().flatten() {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            s["search"].as_str().unwrap_or(""),
            s["at1"],
            s["at3"],
            s["at5"],
            s["mrr"],
            if s["exact_line_at1"].is_null() { "—".to_string() } else { s["exact_line_at1"].to_string() }
        ));
    }
    md.push_str("\n| Question | Tags | Keyword | Meaning | Hybrid | Chat's note search | Hybrid's first (✓ = a gold line) |\n|---|---|---|---|---|---|---|\n");
    let r = |v: &Value| v["rank"].as_u64().map_or("—".to_string(), |x| x.to_string());
    for row in retrieval["rows"].as_array().into_iter().flatten() {
        if row["hybrid"]["rank"].is_null() && row["hybrid"].get("found").is_some() {
            continue;
        }
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            row["q"].as_str().unwrap_or(""),
            row["tags"].as_array().map(|t| t.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            r(&row["keyword"]),
            r(&row["meaning"]),
            r(&row["hybrid"]),
            r(&row["notes_keyword"]),
            row["hybrid"]["top"].as_str().unwrap_or("").to_string() + if row["hybrid"]["exact"] == json!(true) { " ✓" } else { "" },
        ));
    }
    md.push_str("\nNo answer in the corpus (hybrid): ");
    let none: Vec<String> = retrieval["unanswerable"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|u| format!("\"{}\" → {} passages{}", u["q"].as_str().unwrap_or(""), u["passages"], u["range"].as_str().map(|r| format!(" ({r})")).unwrap_or_default()))
        .collect();
    md.push_str(&none.join("; "));
    md.push('\n');
    for (label, a) in answers {
        md.push_str(&format!(
            "\n## Answers: {label}\n\n{}/{} pass; {} cite a passage, {} cite one with a gold line; {} tokens.\n\n| Question | Pass | Cites | Gold cited | Gold in passages | Note added | Answer |\n|---|---|---|---|---|---|---|\n",
            a["passed"], a["asked"], a["cited"], a["gold_cited"], a["tokens"]
        ));
        for row in a["rows"].as_array().into_iter().flatten() {
            let reply: String = row["reply"].as_str().unwrap_or(row["error"].as_str().unwrap_or("")).replace('\n', " ").chars().take(220).collect();
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                row["q"].as_str().unwrap_or(""),
                if row["pass"] == json!(true) { "yes" } else { "no" },
                row["cites"],
                row["cites_gold"],
                row["gold_in_passages"],
                row["uncited_note"],
                reply.replace('|', "\\|"),
            ));
        }
    }
    md
}
