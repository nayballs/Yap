//! `--search`'s second corpus, `evals/search/library.json`: search beyond
//! the meetings (2026-10-25) — one made-up person's notes, two meetings,
//! their past chats and memory, and questions of five kinds.
//!
//! - **Retrieval** (no chat model needed): every note, chat and memory item
//!   seeded and cut as Yap cuts them (`search::index`), read by the run's
//!   embedding server, then each question searched **before** (Yap's
//!   keyword searches until 25 Oct 2026: the Notes view's title-and-preview
//!   filter, `search_notes`' whole-note keyword count, `chats::search`,
//!   memory's keyword rule) and **after** (`search::library`, words only and
//!   with meaning). Recall@1/3/5 and MRR by item; for memory, recall of the
//!   gold items in the three that go with a question, and how many
//!   questions about nothing in memory got something anyway (and the
//!   closest similarity each had: what the model's memory floor is set by).
//! - **Answers** (each `--model`): the `chat` questions asked in Chat (not
//!   across meetings: `turn::chat_turn` with no scope, the tools and memory
//!   as a person has them), an answer passing when it holds one of the
//!   expected words. `--search-keyword` asks them with the searches as they
//!   were (the "before"); `--only` picks questions, `--max-total-tokens`
//!   caps a cloud model's spend.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::search::library::{self, Doc};
use crate::search::store::Key;

#[derive(Debug, Deserialize)]
pub struct Library {
    pub now: String,
    pub notes: Vec<super::search::CNote>,
    #[serde(default)]
    pub chats: Vec<LChat>,
    #[serde(default)]
    pub memory: Vec<LMemory>,
    pub questions: Vec<LQuestion>,
}

#[derive(Debug, Deserialize)]
pub struct LChat {
    pub id: u64,
    pub title: String,
    /// `[role, text]`.
    pub messages: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
pub struct LMemory {
    pub id: u64,
    pub section: String,
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub facts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LQuestion {
    /// "box" | "tool" | "chats" | "memory" | "chat".
    pub kind: String,
    pub q: String,
    /// The corpus ids of the notes, chats or memory items that answer it.
    #[serde(default)]
    pub gold: Vec<u64>,
    /// A `chat` answer holds one of these.
    #[serde(default)]
    pub expect: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub fn load(path: &Path) -> Result<Library, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let l: Library = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for q in &l.questions {
        let known: Vec<u64> = match q.kind.as_str() {
            "box" | "tool" => l.notes.iter().map(|n| n.id).collect(),
            "chats" => l.chats.iter().map(|c| c.id).collect(),
            "memory" => l.memory.iter().map(|m| m.id).collect(),
            "chat" => Vec::new(),
            k => return Err(format!("\"{}\": no kind \"{k}\"", q.q)),
        };
        if let Some(g) = q.gold.iter().find(|g| !known.contains(g)) {
            return Err(format!("\"{}\": no {} {g}", q.q, q.kind));
        }
        if q.kind == "chat" && q.expect.is_empty() {
            return Err(format!("\"{}\": a chat question needs `expect`", q.q));
        }
    }
    Ok(l)
}

/// Corpus id → store id, for each kind.
#[derive(Debug, Default, Clone)]
pub struct Ids {
    pub notes: HashMap<u64, u64>,
    pub chats: HashMap<u64, u64>,
    pub memory: HashMap<u64, u64>,
}

/// The chats and memory in the run's (fresh) stores.
fn seed_chats_and_memory(l: &Library, ids: &mut Ids) -> Result<(), String> {
    crate::chat::chats::eval_reset();
    crate::memory::eval_reset();
    ids.chats.clear();
    ids.memory.clear();
    for c in &l.chats {
        let conv = crate::chat::chats::create(&c.title);
        for (role, text) in &c.messages {
            crate::chat::chats::append(conv.id, role, text, &[], None, false)?;
        }
        ids.chats.insert(c.id, conv.id);
    }
    for m in &l.memory {
        use crate::memory::ops::{Action, Change, Origin, Rules, Section};
        let section = Section::parse(&m.section).ok_or_else(|| format!("memory section \"{}\"", m.section))?;
        let change = Change {
            action: Action::Add,
            id: None,
            section: Some(section),
            name: m.name.clone(),
            about: (!m.about.is_empty()).then(|| m.about.clone()),
            facts: m.facts.clone(),
            remove_facts: vec![],
            say: String::new(),
        };
        let rules = Rules { sensitive_ok: true, origin: Origin::new("chat", None, "Seed"), now: crate::memory::now_secs(), max_changes: 1, grounding: None };
        if crate::memory::change(&[change], &rules).done.is_empty() {
            return Err(format!("memory seed \"{}\" wasn't kept", m.name));
        }
        let id = crate::memory::items().iter().find(|i| i.title == m.name).map(|i| i.id).ok_or("a seeded memory item went missing")?;
        ids.memory.insert(m.id, id);
    }
    Ok(())
}

/// Everything in the run's stores: the notes (as the meetings corpus seeds
/// them), the chats, memory.
pub fn seed(l: &Library) -> Result<Ids, String> {
    let corpus = super::search::Corpus { now: l.now.clone(), notes: l.notes.clone(), questions: Vec::new() };
    let mut ids = Ids { notes: super::search::seed(&corpus)?, ..Default::default() };
    seed_chats_and_memory(l, &mut ids)?;
    Ok(ids)
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

/// The first place in `found` (store ids) holding a gold item.
fn rank(found: &[u64], gold: &[u64], map: &HashMap<u64, u64>) -> Option<usize> {
    let want: Vec<u64> = gold.iter().filter_map(|g| map.get(g).copied()).collect();
    found.iter().position(|id| want.contains(id))
}

/// The Notes view's search until 25 Oct 2026: the open folder's notes whose
/// title or first 120 characters held the text typed (here: any folder).
fn old_box(query: &str) -> Vec<u64> {
    let q = query.trim().to_lowercase();
    let mut notes = crate::notes::all();
    notes.sort_by_key(|n| std::cmp::Reverse(n.updated_ts));
    notes
        .into_iter()
        .filter(|n| n.title.to_lowercase().contains(&q) || n.content.chars().take(120).collect::<String>().to_lowercase().contains(&q))
        .map(|n| n.id)
        .collect()
}

/// The retrieval half: every passage of every kind read, every question
/// searched before and after.
pub async fn retrieval(l: &Library, ids: &Ids) -> Result<Value, String> {
    let lib = crate::search::index::Library::now();
    let passages: Vec<&crate::search::index::Prepared> = lib.passages().collect();
    println!("  {} notes, {} chats, {} memory items → {} passages; embedding them …", l.notes.len(), l.chats.len(), l.memory.len(), passages.len());
    let t0 = Instant::now();
    let mut vectors: HashMap<Key, Vec<f32>> = HashMap::new();
    for batch in passages.chunks(16) {
        let texts: Vec<String> = batch.iter().map(|p| p.embedded.clone()).collect();
        for (p, v) in batch.iter().zip(crate::search::embedder::embed(&texts).await?) {
            vectors.insert(p.key, v);
        }
    }
    let index_ms = t0.elapsed().as_millis() as u64;
    let items: Vec<(Key, Vec<f32>)> = vectors.iter().map(|(k, v)| (*k, v.clone())).collect();
    crate::search::index::with_store(|s| s.add(&items)).map_err(|e| e.to_string())?;

    let notes = crate::notes::all();
    let notes_prepared = crate::search::index::prepared(&notes);
    let note_docs: Vec<Doc> = notes.iter().zip(&notes_prepared).map(|(n, p)| Doc { id: n.id, title: &n.title, passages: p, updated: n.updated_ts }).collect();
    let convs: Vec<crate::chat::chats::Conversation> = crate::chat::chats::with_all(|c| c.to_vec());
    let chats_prepared = crate::search::index::prepared_chats(&convs);
    let chat_docs: Vec<Doc> = convs.iter().zip(&chats_prepared).map(|(c, p)| Doc { id: c.id, title: &c.title, passages: p, updated: c.updated_ts }).collect();
    let mem = crate::memory::items();
    let mem_prepared = crate::search::index::prepared_memory(&mem);
    let mem_passages: Vec<Option<&crate::search::index::Prepared>> = mem_prepared.iter().map(|p| p.first()).collect();
    let now = chrono::DateTime::parse_from_rfc3339(&l.now).map_err(|e| e.to_string())?.with_timezone(&chrono::Local);

    // Each search's ranks, by name.
    let mut ranks: Vec<(String, String, Vec<Option<usize>>)> = Vec::new(); // (kind, search, ranks)
    let mut push = |kind: &str, name: &str, r: Option<usize>| match ranks.iter_mut().find(|(k, n, _)| k == kind && n == name) {
        Some((_, _, v)) => v.push(r),
        None => ranks.push((kind.to_string(), name.to_string(), vec![r])),
    };
    let mut rows = Vec::new();
    // Memory: recall@3 and false positives, before and after.
    let (mut mem_gold, mut mem_before, mut mem_after, mut neg, mut neg_before, mut neg_after) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut neg_sims: Vec<f32> = Vec::new();
    let mut gold_sims: Vec<f32> = Vec::new();
    let mut typed_meaning_sims: Vec<(String, f32, bool)> = Vec::new();
    let mut box_sims: Vec<(String, f32, f32)> = Vec::new();
    for q in &l.questions {
        if q.kind == "chat" {
            continue;
        }
        let qv = crate::search::embedder::embed_query(&q.q).await?;
        let sim = |k: &Key| vectors.get(k).map(|v| crate::search::rank::cosine(&qv, v));
        let none = |_: &Key| None;
        let mut row = json!({ "kind": q.kind, "q": q.q, "tags": q.tags });
        match q.kind.as_str() {
            "box" => {
                let before = rank(&old_box(&q.q), &q.gold, &ids.notes);
                let words: Vec<u64> = library::search_typed(&note_docs, &q.q, &none, 20).iter().map(|h| h.id).collect();
                let hits = library::search_typed(&note_docs, &q.q, &sim, 20);
                let both: Vec<u64> = hits.iter().map(|h| h.id).collect();
                for h in hits.iter().filter(|h| !h.words) {
                    let gold = q.gold.iter().filter_map(|g| ids.notes.get(g)).any(|g| *g == h.id);
                    typed_meaning_sims.push((q.q.clone(), h.similarity.unwrap_or(0.0), gold));
                }
                let (w, b) = (rank(&words, &q.gold, &ids.notes), rank(&both, &q.gold, &ids.notes));
                push("box", "before: title and preview", before);
                push("box", "words", w);
                push("box", "words + meaning", b);
                // How close the gold notes and the closest other note come (the box's floor).
                let gold_ids: Vec<u64> = q.gold.iter().filter_map(|g| ids.notes.get(g).copied()).collect();
                let best_of = |gold: bool| {
                    note_docs.iter().filter(|d| gold_ids.contains(&d.id) == gold).flat_map(|d| d.passages.iter()).filter_map(|p| sim(&p.key)).fold(f32::MIN, f32::max)
                };
                let (g, o) = (best_of(true), best_of(false));
                box_sims.push((q.q.clone(), g, o));
                row["ranks"] = json!({ "before": before.map(|r| r + 1), "words": w.map(|r| r + 1), "meaning": b.map(|r| r + 1), "results": both.len(), "gold_similarity": g, "other_similarity": o });
            }
            "tool" => {
                let before: Vec<u64> = crate::chat::tools::search_notes(&q.q, 5).into_iter().map(|(_, n)| n.id).collect();
                let kw = crate::search::ask::find_in(&notes, &notes_prepared, &q.q, &now, &none, crate::search::rank::Mode::Keyword, crate::search::rank::TUNING, 15);
                let hy = crate::search::ask::find_in(&notes, &notes_prepared, &q.q, &now, &sim, crate::search::rank::Mode::Hybrid, crate::search::rank::TUNING, 15);
                let ids_of = |f: crate::search::ask::Found| library::group_by_note(f.passages, 5).into_iter().map(|n| n.id).collect::<Vec<_>>();
                let (b, k, h) = (rank(&before, &q.gold, &ids.notes), rank(&ids_of(kw), &q.gold, &ids.notes), rank(&ids_of(hy), &q.gold, &ids.notes));
                push("tool", "before: whole notes, keywords", b);
                push("tool", "passages, keywords", k);
                push("tool", "passages, hybrid", h);
                row["ranks"] = json!({ "before": b.map(|r| r + 1), "keywords": k.map(|r| r + 1), "hybrid": h.map(|r| r + 1) });
            }
            "chats" => {
                let before: Vec<u64> = crate::chat::chats::search(&q.q, None, 5).into_iter().map(|f| f.id).collect();
                let words: Vec<u64> = library::find_chats(&chat_docs, &q.q, &none, 5).into_iter().map(|f| f.id).collect();
                let both: Vec<u64> = library::find_chats(&chat_docs, &q.q, &sim, 5).into_iter().map(|f| f.id).collect();
                let (b, w, h) = (rank(&before, &q.gold, &ids.chats), rank(&words, &q.gold, &ids.chats), rank(&both, &q.gold, &ids.chats));
                push("chats", "before: chats::search", b);
                push("chats", "words (BM25)", w);
                push("chats", "words + meaning", h);
                row["ranks"] = json!({ "before": b.map(|r| r + 1), "words": w.map(|r| r + 1), "hybrid": h.map(|r| r + 1) });
            }
            "memory" => {
                let keyword = crate::memory::prompt::relevant_indexes(&mem, &q.q, 3);
                let after = library::memory_relevant(&keyword, &mem_passages, &sim, 3);
                let to_ids = |v: &[usize]| v.iter().map(|&i| mem[i].id).collect::<Vec<u64>>();
                let (before_ids, after_ids) = (to_ids(&keyword), to_ids(&after));
                let best = mem_passages.iter().flatten().filter_map(|p| sim(&p.key)).fold(f32::MIN, f32::max);
                if q.gold.is_empty() {
                    neg += 1;
                    neg_before += usize::from(!before_ids.is_empty());
                    neg_after += usize::from(!after_ids.is_empty());
                    neg_sims.push(best);
                } else {
                    let want: Vec<u64> = q.gold.iter().filter_map(|g| ids.memory.get(g).copied()).collect();
                    mem_gold += want.len();
                    mem_before += want.iter().filter(|w| before_ids.contains(w)).count();
                    mem_after += want.iter().filter(|w| after_ids.contains(w)).count();
                    for w in &want {
                        if let Some(p) = mem_passages.iter().flatten().find(|p| p.chunk.note == *w) {
                            gold_sims.push(sim(&p.key).unwrap_or(0.0));
                        }
                    }
                }
                let names = |v: &[u64]| v.iter().filter_map(|id| mem.iter().find(|i| i.id == *id).map(|i| i.title.clone())).collect::<Vec<_>>();
                row["found"] = json!({ "before": names(&before_ids), "after": names(&after_ids), "best_similarity": (best * 1000.0).round() / 1000.0 });
            }
            _ => {}
        }
        rows.push(row);
    }
    let summary: Vec<Value> = ranks
        .iter()
        .map(|(kind, name, r)| json!({ "kind": kind, "search": name, "n": r.len(), "at1": recall(r, 1), "at3": recall(r, 3), "at5": recall(r, 5), "mrr": mrr(r) }))
        .collect();
    let ratio = |a: usize, b: usize| if b == 0 { 0.0 } else { (a as f64 / b as f64 * 1000.0).round() / 1000.0 };
    neg_sims.sort_by(|a, b| b.total_cmp(a));
    gold_sims.sort_by(|a, b| a.total_cmp(b));
    let memory = json!({
        "gold_items": mem_gold,
        "recall_before": ratio(mem_before, mem_gold),
        "recall_after": ratio(mem_after, mem_gold),
        "unrelated_questions": neg,
        "unrelated_with_items_before": neg_before,
        "unrelated_with_items_after": neg_after,
        "unrelated_best_similarity": neg_sims.iter().map(|s| (s * 1000.0).round() / 1000.0).collect::<Vec<_>>(),
        "gold_similarity": gold_sims.iter().map(|s| (s * 1000.0).round() / 1000.0).collect::<Vec<_>>(),
        "meaning_min": library::memory_meaning_min(),
    });
    for s in &summary {
        println!("  {:<7} {:<34} n {:>2}  recall@1 {:>5}  @3 {:>5}  @5 {:>5}  MRR {:>5}", s["kind"].as_str().unwrap_or(""), s["search"].as_str().unwrap_or(""), s["n"], s["at1"], s["at3"], s["at5"], s["mrr"]);
    }
    println!(
        "  memory: gold items found {} → {}; unrelated questions given an item {}/{} → {}/{} (their closest: {:?}; gold: {:?})",
        memory["recall_before"], memory["recall_after"], neg_before, neg, neg_after, neg, memory["unrelated_best_similarity"], memory["gold_similarity"]
    );
    let typed: Vec<Value> = typed_meaning_sims.iter().map(|(q, s, g)| json!({ "q": q, "similarity": (s * 1000.0).round() / 1000.0, "gold": g })).collect();
    let r3 = |x: f32| (x * 1000.0).round() / 1000.0;
    let box_floor: Vec<Value> = box_sims.iter().map(|(q, g, o)| json!({ "q": q, "gold": r3(*g), "closest_other": r3(*o) })).collect();
    let closest_other = box_sims.iter().map(|b| b.2).fold(f32::MIN, f32::max);
    println!(
        "  box: the gold note's similarity {:?}; the closest other note's at most {:.3} (floor {})",
        box_sims.iter().map(|b| r3(b.1)).collect::<Vec<_>>(),
        closest_other,
        library::typed_meaning_min()
    );
    Ok(json!({
        "model": crate::search::embedder::model().name,
        "passages": passages.len(),
        "index_ms": index_ms,
        "summary": summary,
        "memory": memory,
        "box_meaning_only": typed,
        "box_similarities": box_floor,
        "box_closest_other": r3(closest_other),
        "rows": rows,
    }))
}

/// The answers half on one model (its settings in place): each `chat`
/// question in Chat, chats and memory seeded afresh (notes stay).
pub async fn answers(l: &Library, ids: &Ids, label: &str, only: &[String], cap: Option<u64>) -> Value {
    let only: Vec<String> = only.iter().map(|o| o.to_lowercase()).collect();
    let mut rows = Vec::new();
    let (mut used, mut passed, mut asked) = (0u64, 0usize, 0usize);
    let mut ids = ids.clone();
    for q in l.questions.iter().filter(|q| q.kind == "chat") {
        if !only.is_empty() && !only.iter().any(|o| q.q.to_lowercase().contains(o) || q.tags.iter().any(|t| t == o)) {
            continue;
        }
        if cap.is_some_and(|cap| used >= cap) {
            rows.push(json!({ "q": q.q, "skipped": "token cap reached (--max-total-tokens)" }));
            continue;
        }
        if let Err(e) = seed_chats_and_memory(l, &mut ids) {
            rows.push(json!({ "q": q.q, "error": e }));
            continue;
        }
        super::record::set_context(json!({ "model": label, "scenario": format!("library: {}", q.q) }));
        super::record::begin_turn();
        let t0 = Instant::now();
        let turn = Box::pin(crate::chat::turn::chat_turn(None, None, &q.q, None, false, |_, _| {}, None, None)).await;
        let stats = super::record::end_turn();
        used += stats.total_tokens;
        asked += 1;
        let tools: Vec<Value> = stats.tool_calls.iter().map(|t| json!({ "name": t.name, "args": t.args, "display": t.display })).collect();
        match turn {
            Ok(t) => {
                // Spaces aside (gpt-oss writes "5 %" and "2 May" with narrow ones).
                let squash = |s: &str| s.to_lowercase().chars().filter(|c| !c.is_whitespace() && *c != '*').collect::<String>();
                let reply = squash(&t.reply);
                let pass = q.expect.iter().any(|e| reply.contains(&squash(e)));
                passed += usize::from(pass);
                println!("  {} {:<60} {}", if pass { "PASS" } else { "FAIL" }, q.q.chars().take(60).collect::<String>(), tools.iter().filter_map(|t| t["display"].as_str()).collect::<Vec<_>>().join(" | "));
                rows.push(json!({ "q": q.q, "tags": q.tags, "pass": pass, "reply": t.reply, "tools": tools, "tokens": stats.total_tokens, "ms": t0.elapsed().as_millis() as u64 }));
            }
            Err(e) => rows.push(json!({ "q": q.q, "error": super::record::redact(&e), "tools": tools })),
        }
    }
    json!({ "asked": asked, "passed": passed, "tokens": used, "keyword": library::keyword_only(), "rows": rows })
}

/// The library's part of `search.md`.
pub fn markdown(retrieval: &Value, answers: &[(String, Value)]) -> String {
    let mut md = format!("\n# Search beyond the meetings ({})\n\n{} passages, embedded in {} ms.\n\n", retrieval["model"].as_str().unwrap_or(""), retrieval["passages"], retrieval["index_ms"]);
    md.push_str("| Kind | Search | n | Recall@1 | @3 | @5 | MRR |\n|---|---|---|---|---|---|---|\n");
    for s in retrieval["summary"].as_array().into_iter().flatten() {
        md.push_str(&format!("| {} | {} | {} | {} | {} | {} | {} |\n", s["kind"].as_str().unwrap_or(""), s["search"].as_str().unwrap_or(""), s["n"], s["at1"], s["at3"], s["at5"], s["mrr"]));
    }
    let m = &retrieval["memory"];
    md.push_str(&format!(
        "\nMemory: gold items with their question {} → {}; questions about nothing in memory given an item {} → {} of {} (closest {}; the gold items' {}; meaning's floor {}).\n",
        m["recall_before"], m["recall_after"], m["unrelated_with_items_before"], m["unrelated_with_items_after"], m["unrelated_questions"], m["unrelated_best_similarity"], m["gold_similarity"], m["meaning_min"]
    ));
    md.push_str(&format!("\nNotes found by meaning alone in the search box: {}\n", retrieval["box_meaning_only"]));
    md.push_str("\n| Kind | Question | Before | After |\n|---|---|---|---|\n");
    for r in retrieval["rows"].as_array().into_iter().flatten() {
        let (b, a) = if r["kind"] == "memory" {
            (r["found"]["before"].to_string(), r["found"]["after"].to_string())
        } else {
            let after = if r["kind"] == "box" { &r["ranks"]["meaning"] } else { &r["ranks"]["hybrid"] };
            (r["ranks"]["before"].to_string(), after.to_string())
        };
        md.push_str(&format!("| {} | {} | {} | {} |\n", r["kind"].as_str().unwrap_or(""), r["q"].as_str().unwrap_or(""), b, a));
    }
    for (label, a) in answers {
        md.push_str(&format!(
            "\n## Chat's answers: {label}{}\n\n{}/{} pass; {} tokens.\n\n| Question | Pass | Tools | Answer |\n|---|---|---|---|\n",
            if a["keyword"] == json!(true) { " (keyword searches, as before)" } else { "" },
            a["passed"],
            a["asked"],
            a["tokens"]
        ));
        for r in a["rows"].as_array().into_iter().flatten() {
            let reply: String = r["reply"].as_str().unwrap_or(r["error"].as_str().unwrap_or("")).replace('\n', " ").chars().take(200).collect();
            let tools: Vec<&str> = r["tools"].as_array().into_iter().flatten().filter_map(|t| t["display"].as_str()).collect();
            md.push_str(&format!("| {} | {} | {} | {} |\n", r["q"].as_str().unwrap_or(""), if r["pass"] == json!(true) { "yes" } else { "no" }, tools.join("; ").replace('|', "/"), reply.replace('|', "\\|")));
        }
    }
    md
}
