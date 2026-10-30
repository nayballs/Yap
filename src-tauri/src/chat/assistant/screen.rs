//! **The Voice Assistant sees the window in front** (2026-10-29): a spoken
//! question about what's on screen — "what's this error?", "summarise this
//! page", "what does this chart say?" — takes a screenshot of the window
//! that was in front when the assistant's shortcut was pressed, and the
//! model answers from it.
//!
//! **Never for every question.** Cheap rules first ([`about`], pure, in the
//! languages Yap's speech models know best: English, French, German,
//! Spanish, Italian, Portuguese and Dutch): words that point at the screen
//! ("on my screen", "this error", "summarise this", "qu'est-ce que c'est",
//! "dieser Fehler") say yes; a demonstrative on its own ("is this right?",
//! "how do I fix this?") is a maybe; anything else, no — "how long do I
//! boil an egg" is never a screenshot. A maybe is the model's call
//! ([`model_says`]: one short yes/no request to Chat's own model, with the
//! last answer for context, so "what does this mean?" after an answer is
//! about the answer).
//!
//! **What's taken** ([`capture`]): the window's visible frame
//! (`snip/pick.rs`, DWM's frame; the snip's window pick), cut from every
//! monitor captured at once the way a snip is (`snip/capture.rs`: HDR
//! right), with every Yap window left out for the moment of the capture
//! (`calls/capture.rs` `for_snip`) — the bar's capsule included — and Yap's
//! meeting windows kept out as the meeting setting wants (the Voice
//! Assistant is off during meetings anyway). Not when the window in front
//! is one of Yap's own, minimised or gone. Saved like a snip
//! (`chat-images/`, source "window"): it shows on the question in Chat and
//! is kept as Chat's screenshots are.
//!
//! **Where it goes** ([`allowed`]): "Let the Voice Assistant see the window
//! in front when you ask about it" (`assistant_screen`, on) for a model on
//! this PC — the picture never leaves it. A cloud model gets it only with
//! "…with cloud models too" (`assistant_screen_cloud`, off): the first time
//! a question about the screen meets one, Yap answers without it, the model
//! is told so (it says it couldn't see the screen), and an alert asks once
//! ([`ask_once`]: **Allow**). The network receipts name the upload ("Voice
//! Assistant + the window in front · Groq", `privacy/net.rs`).
//!
//! **A model that can't read images** (`snip/vision.rs`; the on-device model
//! without its projector, or whose projector won't load): the window's text,
//! read on this PC by Windows' OCR (`snip/ocr.rs`), goes after the question
//! instead, the screenshot kept with it marked "Sent as text", and the
//! answer names it in its sources ("The window's text, read on this PC").
//!
//! Logs: counts, sizes and timings only — never the question, a title or
//! what was on screen.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::chat::chats::{ChatImage, Source};
use crate::snip::turn::TurnImages;

/// The window that was in front when the assistant's shortcut was pressed
/// (`pipeline::capture_target`): a handle, never read until a question about
/// the screen asks for it.
static WINDOW: AtomicIsize = AtomicIsize::new(0);

/// The window in front at the press (0: none).
pub fn window_in_front(hwnd: isize) {
    WINDOW.store(hwnd, Ordering::SeqCst);
}

// ---- is it about the screen? -------------------------------------------------------------

/// What a question's words say about the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum About {
    /// It points at the screen: take the window.
    Yes,
    /// "this" with nothing saying what: the model decides.
    Maybe,
    /// Nothing about the screen.
    No,
}

/// Phrases that point at the screen, in each language (the question
/// lower-cased, curly apostrophes made straight).
static SCREEN_PHRASES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?x)",
        // English
        r"\b(on|in)\ (my|the|this)\ (screen|display|monitor)\b",
        r"|\bon[-\ ]screen\b|\bmy\ screen\b|\bthis\ screen\b",
        r"|\bwhat\ am\ i\ (looking\ at|seeing)\b|\bcan\ you\ see\ (this|my\ screen|the\ screen)\b|\blook\ at\ (this|my\ screen|the\ screen)\b",
        r"|\bin\ front\ of\ me\b",
        r"|^\s*what('s|\ is|\ are)\ (this|these)\s*(thing|one|here|about)?\s*$",
        r"|\bwhat\ (does|do)\ (this|these)(\ \w+)?\ (say|says|mean|means|show|shows)\b",
        r"|\bwhat('s|\ is)\ (it\ saying|going\ on|wrong|happening)\ here\b|\bwhat\ does\ it\ say\ here\b",
        r"|\b(summari[sz]e|explain|translate|read\ out|describe)\ (what's|what\ is)\ on\b",
        // French
        r"|\b(sur|à)\ (mon\ |l'|cet\ )?écran\b|\bmon\ écran\b",
        r"|\bqu'est-ce\ que\ c'est\b|\bc'est\ quoi\ (ça|ceci|cela)\b|\bça\ veut\ dire\ quoi\b",
        r"|\bqu'est-ce\ que\ (ça|cela|ceci)\ (veut\ dire|signifie|dit)\b",
        r"|\b(résume|résumez|explique|expliquez|traduis|traduisez|lis|lisez|décris|décrivez|analyse|analysez)(-moi)?\ (ça|ceci|cela)\b",
        // German
        r"|\bauf\ (dem|meinem)\ bildschirm\b|\bmeine[mn]?\ bildschirm\b",
        r"|^\s*was\ ist\ (das|dies)(\ hier|\ da)?\s*$|\bwas\ (bedeutet|heißt|heisst|sagt)\ (das|dies)\b",
        r"|\b(fass|fasse|erklär|erkläre|übersetz|übersetze|lies|beschreib|beschreibe|analysier|analysiere)(\ mir)?\ (das|dies)\b",
        // Spanish
        r"|\ben\ (la|mi)\ pantalla\b",
        r"|\bqué\ es\ esto\b|\bqué\ (significa|dice|quiere\ decir)\ esto\b|\bqué\ dice\ aquí\b",
        r"|\b(resume|resúmeme|explica|explícame|traduce|tradúceme|lee|léeme|describe|analiza)\ (esto|esta|este)\b",
        // Italian
        r"|\bsullo\ schermo\b|\bsul\ mio\ schermo\b",
        r"|\bcos'è\ (questo|questa)\b|\bche\ cos'è\b|\bcosa\ (significa|dice|vuol\ dire)\ (questo|questa)\b",
        r"|\b(riassumi|spiega|spiegami|traduci|leggi|descrivi|analizza)\ (questo|questa|questi|queste)\b",
        // Portuguese
        r"|\bna\ (minha\ )?tela\b|\bno\ (meu\ )?ecrã\b",
        r"|\bo\ que\ é\ (isto|isso)\b|\bo\ que\ (significa|quer\ dizer|diz)\ (isto|isso)\b",
        r"|\b(resume|resuma|explica|explique|traduz|traduza|lê|leia|descreve|descreva|analisa|analise)\ (isto|isso)\b",
        // Dutch
        r"|\bop\ (het|mijn)\ scherm\b",
        r"|\bwat\ is\ dit\b|\bwat\ (betekent|zegt)\ dit\b",
        r"|\b(vat|leg|vertaal|lees|beschrijf|analyseer)\ dit\b",
    ))
    .expect("static regex")
});

/// "This" in each language (its code, as My languages has it): before a
/// screen word, the question is about the screen; alone, it may be. Only
/// the person's own languages count (English always): Dutch "dit" is
/// French for "said".
const DEMONSTRATIVES: &[(&str, &[&str])] = &[
    ("en", &["this", "these"]), // ("that" is weaker: `THAT`)
    ("fr", &["ce", "cet", "cette", "ces", "ça", "ceci"]),
    ("de", &["dies", "diese", "dieser", "dieses", "diesen", "diesem"]),
    ("es", &["este", "esta", "esto", "estos", "estas"]),
    ("it", &["questo", "questa", "questi", "queste", "quest"]),
    ("pt", &["este", "esta", "isto", "isso", "estes", "estas"]),
    ("nl", &["dit", "deze"]),
];

/// The demonstratives of `langs` (codes; English always; none known: all).
fn demonstratives(langs: &[String]) -> Vec<&'static str> {
    let all = langs.iter().all(|l| !DEMONSTRATIVES.iter().any(|(c, _)| l.starts_with(c)));
    DEMONSTRATIVES
        .iter()
        .filter(|(code, _)| all || *code == "en" || langs.iter().any(|l| l.starts_with(code)))
        .flat_map(|(_, words)| words.iter().copied())
        .collect()
}

/// "That", "those": a screen word after them may be the screen or an earlier
/// answer (a maybe).
const THAT: &[&str] = &["that", "those"];

/// Asking for something done to "this": "summarise this", "explain these"
/// (not "review this week": [`NOT_AFTER`]).
const VERBS: &[&str] =
    &["summarise", "summarize", "explain", "translate", "read", "describe", "proofread", "analyse", "analyze", "debug", "review", "check"];

/// What's on a screen, in each language (a demonstrative before one of these
/// points at it: "this error", "cette page", "dieser Fehler").
const SCREEN_WORDS: &[&str] = &[
    // English
    "error", "errors", "warning", "message", "messages", "email", "e-mail", "mail", "page", "tab", "website", "site", "window",
    "dialog", "dialogue", "popup", "pop-up", "document", "doc", "file", "pdf", "code", "function", "chart", "graph", "plot",
    "table", "spreadsheet", "sheet", "diagram", "picture", "image", "photo", "screenshot", "form", "article", "text",
    "paragraph", "sentence", "slide", "presentation", "map", "post", "tweet", "comment", "review", "recipe", "receipt",
    "invoice", "bill", "contract", "letter", "log", "output", "button", "setting", "settings", "menu", "screen", "word",
    "equation", "formula", "stack", "traceback", "exception", "notification", "webpage", "video",
    "app", "graphic", "headline", "caption", "label", "icon", "link", "listing", "product",
    // French
    "erreur", "erreurs", "courriel", "onglet", "fenêtre", "fichier", "graphique", "graphe", "tableau", "diagramme", "capture",
    "formulaire", "texte", "paragraphe", "phrase", "diapositive", "carte", "commentaire", "recette", "reçu", "facture",
    "contrat", "lettre", "journal", "bouton", "paramètre", "paramètres", "réglage", "réglages", "écran", "mot", "équation",
    "formule", "vidéo", "appli", "application", "lien", "publication",
    // German
    "fehler", "fehlermeldung", "meldung", "nachricht", "seite", "webseite", "fenster", "dokument", "datei", "funktion",
    "diagramm", "grafik", "tabelle", "bild", "foto", "formular", "artikel", "absatz", "satz", "folie", "karte", "liste",
    "beitrag", "kommentar", "bewertung", "rezept", "quittung", "rechnung", "vertrag", "brief", "ausgabe", "knopf",
    "einstellung", "einstellungen", "menü", "bildschirm", "wort", "gleichung", "formel",
    // Spanish
    "mensaje", "correo", "página", "pestaña", "sitio", "ventana", "documento", "archivo", "código", "función", "gráfico",
    "gráfica", "tabla", "imagen", "formulario", "artículo", "párrafo", "frase", "diapositiva", "mapa", "lista",
    "publicación", "comentario", "reseña", "receta", "recibo", "factura", "contrato", "carta", "botón", "ajuste",
    "ajustes", "configuración", "pantalla", "palabra", "ecuación", "fórmula", "enlace",
    // Italian
    "errore", "errori", "messaggio", "pagina", "scheda", "finestra", "codice", "funzione", "grafico", "immagine",
    "modulo", "articolo", "testo", "paragrafo", "elenco", "commento", "recensione", "ricetta", "ricevuta", "fattura",
    "contratto", "lettera", "pulsante", "impostazione", "impostazioni", "schermo", "parola", "equazione",
    // Portuguese
    "erro", "erros", "mensagem", "aba", "janela", "ficheiro", "função", "formulário", "parágrafo", "comentário",
    "botão", "definições", "tela", "ecrã",
    // Dutch
    "fout", "foutmelding", "melding", "bericht", "tabblad", "venster", "bestand", "functie", "grafiek", "tabel",
    "afbeelding", "formulier", "tekst", "alinea", "zin", "lijst", "reactie", "factuur", "knop", "instelling",
    "instellingen", "scherm", "woord", "vergelijking", "formule",
];

/// Time and the like after a demonstrative: "this morning", "cette
/// semaine", "dieses Jahr" are never the screen.
const NOT_AFTER: &[&str] = &[
    // English
    "morning", "afternoon", "evening", "night", "week", "weekend", "month", "year", "time", "season", "summer", "winter",
    "spring", "autumn", "fall", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "term",
    "quarter", "semester", "round", "sprint", "holiday", "christmas", "easter", "is", "was", "will", "would",
    // French
    "matin", "soir", "après-midi", "semaine", "week-end", "mois", "année", "an", "été", "hiver", "printemps", "automne",
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche", "que", "qui", "qu", "dont", "sont", "soir-là",
    // German
    "woche", "wochenende", "monat", "jahr", "mal", "morgen", "abend", "sommer", "winter", "montag", "dienstag", "mittwoch",
    "donnerstag", "freitag", "samstag", "sonntag",
    // Spanish
    "mañana", "tarde", "noche", "semana", "mes", "año", "fin", "verano", "invierno", "lunes", "martes", "miércoles",
    "jueves", "viernes", "sábado", "domingo", "vez",
    // Italian
    "mattina", "sera", "settimana", "mese", "anno", "estate", "inverno", "volta", "lunedì", "venerdì", "sabato", "domenica",
    // Portuguese
    "manhã", "noite", "ano", "verão", "fim-de-semana", "segunda", "sexta",
    // Dutch
    "ochtend", "middag", "avond", "week", "maand", "jaar", "keer", "zomer", "maandag", "vrijdag", "zaterdag", "zondag",
];

/// The question as the rules read it: lower case, straight apostrophes,
/// one space between words. Pure.
fn normal(question: &str) -> String {
    question
        .to_lowercase()
        .replace(['\u{2019}', '\u{2018}', '`'], "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Its words (an elided "quest'errore" or "l'écran" in two). Pure.
fn words(normal: &str) -> Vec<&str> {
    normal
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// **Is a spoken question about the screen?** By its words alone, in any of
/// the languages above ([`About`]). Pure.
#[cfg(test)]
pub fn about(question: &str) -> About {
    about_in(question, &[])
}

/// [`about`] in the person's own languages (My languages' codes; none: all
/// of them). Pure.
pub fn about_in(question: &str, langs: &[String]) -> About {
    let text = normal(question);
    let text = text.trim_end_matches(['?', '.', '!', ' ']);
    if SCREEN_PHRASES.is_match(text) {
        return About::Yes;
    }
    let demonstratives = demonstratives(langs);
    let w = words(text);
    let mut maybe = false;
    for (i, word) in w.iter().enumerate() {
        let this = demonstratives.contains(word);
        let that = THAT.contains(word);
        if !this && !that {
            continue;
        }
        let next = w.get(i + 1).copied();
        if next.is_some_and(|n| NOT_AFTER.contains(&n)) {
            continue;
        }
        // "this error", "this weird error", "cette page", "dieser Fehler".
        let screen_word = [w.get(i + 1), w.get(i + 2)].into_iter().flatten().any(|n| SCREEN_WORDS.contains(n));
        if this && screen_word {
            return About::Yes;
        }
        // "summarise this", "explain these".
        if (*word == "this" || *word == "these") && i > 0 && VERBS.contains(&w[i - 1]) {
            return About::Yes;
        }
        // A French "ce" is mostly "ce que", "c'est" or a noun that isn't
        // the screen: only before a screen word.
        if *word == "ce" {
            continue;
        }
        maybe = true;
    }
    if maybe {
        About::Maybe
    } else {
        About::No
    }
}

// ---- …or the model says it is -------------------------------------------------------------

/// The yes/no request for a maybe (no editable body: Yap's own question).
pub const ASK_PROMPT: &str = "You decide whether a question someone asked a voice assistant out loud is about something on their screen: the app, web page, email, document, error, chart or picture in front of them. Reply with one word: yes if answering it needs to see their screen, no if it doesn't (general knowledge, their notes or calendar, or something said earlier in the conversation, such as the assistant's last answer). The question may be in any language; reply in English.";

/// The yes/no request's messages: the last answer (when there is one, so a
/// question about it isn't taken for the screen), then the question. Pure.
pub fn ask_messages(question: &str, last_answer: Option<&str>) -> Value {
    let mut user = String::new();
    if let Some(a) = last_answer.map(str::trim).filter(|a| !a.is_empty()) {
        let a: String = a.chars().take(400).collect();
        user.push_str(&format!("The assistant's last answer: \"{a}\"\n\n"));
    }
    user.push_str(&format!("The question: \"{}\"\n\nIs it about something on their screen? yes or no.", question.trim()));
    json!([
        { "role": "system", "content": ASK_PROMPT },
        { "role": "user", "content": user },
    ])
}

/// Did the model say yes? Its first word, in English or the languages
/// above. Pure.
pub fn says_yes(reply: &str) -> bool {
    let first = reply
        .trim()
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("")
        .to_lowercase();
    matches!(first.as_str(), "yes" | "y" | "oui" | "ja" | "sí" | "si" | "sim" | "true")
}

/// The yes/no request's own fields: a short reply (a model on this PC), and
/// little thinking for gpt-oss (its reasoning would take the whole reply).
/// Pure.
pub fn ask_extra(model: &str) -> serde_json::Map<String, Value> {
    crate::chat::compact::extra(model)
}

/// Chat's model decides a maybe: one short request, ≤ 8 s (a model on this
/// PC: 15 s, it may be loading). Anything but a clear yes is a no.
async fn model_says(question: &str, conversation: Option<u64>, ep: &Endpoint<'_>) -> bool {
    let last = conversation
        .and_then(crate::chat::chats::get)
        .and_then(|c| c.messages.iter().rev().find(|m| m.role == "assistant").map(|m| m.text.clone()));
    let here = crate::ai::ai_model::runs_here(ep.provider, ep.base_url);
    let opts = crate::ai::llm::ChatOptions {
        temperature: 0.0,
        max_tokens: here.then_some(4),
        timeout: std::time::Duration::from_secs(if here { 15 } else { 8 }),
        disable_thinking: true,
    };
    let t = std::time::Instant::now();
    let asked = crate::privacy::net::within(
        crate::privacy::net::Feature::Assistant,
        crate::ai::llm::chat_with(ep.base_url, ep.api_key, ep.model, ep.provider, ask_messages(question, last.as_deref()), &opts, ask_extra(ep.model)),
    )
    .await;
    match asked {
        Ok(reply) => {
            let yes = says_yes(&reply);
            tracing::info!(yes, ms = t.elapsed().as_millis() as u64, "Voice Assistant: the model said whether the question is about the screen");
            yes
        }
        Err(e) => {
            tracing::info!(ms = t.elapsed().as_millis() as u64, "Voice Assistant: couldn't ask whether the question is about the screen ({})", e.chars().take(120).collect::<String>());
            false
        }
    }
}

// ---- may it go? -----------------------------------------------------------------------------

/// Whether the window may go to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allowed {
    Yes,
    /// It's a cloud model and "…with cloud models too" is off.
    NotToTheCloud,
    /// "Let the Voice Assistant see the window in front" is off.
    Off,
}

/// Does a call to `provider` at `base_url` leave this PC? The cloud
/// providers always do (whatever their address says); any other server
/// unless it's on this PC. Pure.
pub fn leaves_pc(provider: &str, base_url: &str) -> bool {
    const CLOUD: [&str; 5] = ["groq", "openai", "anthropic", "gemini", "openrouter"];
    CLOUD.contains(&provider) || !crate::ai::ai_model::runs_here(provider, base_url)
}

/// The settings' rule. Pure.
pub fn allowed(on: bool, cloud_on: bool, leaves: bool) -> Allowed {
    if !on {
        Allowed::Off
    } else if leaves && !cloud_on {
        Allowed::NotToTheCloud
    } else {
        Allowed::Yes
    }
}

/// May a screenshot of the window in front taken earlier (kept with its
/// question in Chat, source "window") go to `provider` at `base_url` now,
/// sent again with a later question (`snip::turn::with_history_images`)?
/// Only where the window itself would go ([`allowed`]): one taken for a
/// model on this PC never follows the conversation to a cloud model without
/// "…with cloud models too". Pure.
pub fn window_may_go(cfg: &crate::config::YapConfig, provider: &str, base_url: &str) -> bool {
    allowed(cfg.assistant_screen, cfg.assistant_screen_cloud, leaves_pc(provider, base_url)) == Allowed::Yes
}

/// The alert's **Allow** (`alert::ALLOW_SCREEN_CLOUD`): "…with cloud models
/// too" on (Settings shows it: config's save tells the windows).
pub fn allow_cloud() {
    match crate::config::update(|c| c.assistant_screen_cloud = true) {
        Ok(_) => tracing::info!("Voice Assistant: the window in front may go to cloud models now"),
        Err(e) => tracing::warn!("Voice Assistant: couldn't save the screen setting ({e})"),
    }
}

/// The first question about the screen that met a cloud model: asked once,
/// with **Allow**. Yap answered without the window meanwhile.
fn ask_once(app: &AppHandle, provider: &str) {
    let first = match crate::config::update(|c| c.assistant_screen_asked = true) {
        Ok(c) => !c.assistant_screen_cloud,
        Err(_) => false,
    };
    if !first {
        return;
    }
    let who = crate::privacy::net::provider_name(provider);
    crate::notices::alert::show_soon(
        app,
        crate::notices::alert::Alert {
            kind: KIND,
            icon: "assistant",
            title: "Let the Voice Assistant see the window in front?".into(),
            body: format!(
                "You asked about your screen, and Chat's model runs on {who}, so a screenshot of the window would go there. Yap answered without it. It only ever takes the window when you ask about it."
            ),
            action: Some(crate::notices::alert::Action { id: crate::notices::alert::ALLOW_SCREEN_CLOUD, label: "Allow" }),
            timeout_ms: 20_000,
        },
    );
}

/// The ask's alert kind (its rule in `notice.rs`).
pub const KIND: &str = "assistantScreen";

/// Was the ask already shown (`assistant_screen_asked`)?
fn asked_before(cfg: &crate::config::YapConfig) -> bool {
    cfg.assistant_screen_asked
}

// ---- the question's context ------------------------------------------------------------------

/// The line in the question's context with the window's picture.
pub const PICTURE_NOTE: &str = "The picture with this question is a screenshot Yap took of the window the user had in front of them when they asked: when they say \"this\" or \"here\", they mean what's in it. Answer from it.";

/// …with its text instead (a model that can't read images).
pub const TEXT_NOTE: &str = "The text after the question was read by Yap from the window the user had in front of them when they asked: when they say \"this\" or \"here\", they mean it. Answer from it.";

/// The window wasn't sent: the model says so, rather than guess.
pub fn not_sent_note(provider: &str) -> String {
    format!(
        "The user asked about something on their screen, but Yap didn't send a screenshot: they haven't let the Voice Assistant send their screen to {}. Say in one short sentence that you can't see their screen, and that Settings → Voice Assistant can allow it.",
        crate::privacy::net::provider_name(provider)
    )
}

/// The window couldn't be taken (none in front, or the capture failed).
pub const NO_WINDOW_NOTE: &str = "The user asked about something on their screen, but Yap couldn't take a screenshot of the window in front of them. Say in one short sentence that you can't see it, and that they can snip it to Chat instead.";

/// The source an answer from the window's text names.
pub fn text_source() -> Source {
    Source { title: "The window's text, read on this PC".into(), screen: true, ..Default::default() }
}

// ---- looking ---------------------------------------------------------------------------------

/// Chat's model, for a look (`turn::resolve_chat_endpoint`'s parts).
pub struct Endpoint<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
}

/// What goes with a question about the screen: its screenshot or text (and
/// the line saying what it is), or only a line saying why not.
pub struct Look {
    pub images: TurnImages,
    /// The question to send, the window's text after it (a model that
    /// can't read images): `None` — as asked.
    pub question: Option<String>,
    /// Something from the screen goes with it (the receipts' label).
    pub took: bool,
}

impl Look {
    fn note(note: String) -> Look {
        Look { images: TurnImages { note: Some(note), ..Default::default() }, question: None, took: false }
    }
}

/// **A spoken question, looked at**: `None` when it isn't about the screen
/// (or the setting's off); else what goes with it. `looking(true)` while the
/// window is taken and read (the capsule says so).
pub async fn look(
    app: &AppHandle,
    question: &str,
    cfg: &crate::config::YapConfig,
    ep: Endpoint<'_>,
    conversation: Option<u64>,
    looking: &(dyn Fn(bool) + Send + Sync),
) -> Option<Look> {
    let about = about_in(question, &crate::dictation::languages::my_languages(cfg));
    if about == About::No {
        return None;
    }
    let leaves = leaves_pc(ep.provider, ep.base_url);
    let may = allowed(cfg.assistant_screen, cfg.assistant_screen_cloud, leaves);
    let rule = about;
    tracing::info!(?rule, ?may, "Voice Assistant: a question that may be about the screen");
    match may {
        Allowed::Off => return None,
        // A maybe isn't worth asking about when it couldn't go.
        Allowed::NotToTheCloud if about == About::Maybe => return None,
        Allowed::NotToTheCloud => {
            if !asked_before(cfg) {
                ask_once(app, ep.provider);
            }
            return Some(Look::note(not_sent_note(ep.provider)));
        }
        Allowed::Yes => {}
    }
    // Never during a meeting or a call (the assistant is off then anyway).
    if crate::chat::assistant::is_off() {
        return None;
    }
    looking(true);
    let out = take(app, question, &ep, about, conversation).await;
    looking(false);
    out
}

/// [`look`]'s work once the window may go: the model's word on a maybe,
/// the capture, then its picture or its text.
async fn take(app: &AppHandle, question: &str, ep: &Endpoint<'_>, about: About, conversation: Option<u64>) -> Option<Look> {
    if about == About::Maybe && !model_says(question, conversation, ep).await {
        return None;
    }
    let hwnd = WINDOW.load(Ordering::SeqCst);
    let capture_app = app.clone();
    let t = std::time::Instant::now();
    let image = match tauri::async_runtime::spawn_blocking(move || capture(&capture_app, hwnd)).await {
        Ok(Ok(image)) => image,
        Ok(Err(e)) => {
            tracing::info!("Voice Assistant: no screenshot of the window in front ({e})");
            return Some(Look::note(NO_WINDOW_NOTE.into()));
        }
        Err(e) => {
            tracing::warn!("Voice Assistant: the capture didn't finish ({e})");
            return Some(Look::note(NO_WINDOW_NOTE.into()));
        }
    };
    tracing::info!(w = image.w, h = image.h, ms = t.elapsed().as_millis() as u64, "Voice Assistant: took the window in front");
    // A model that reads images gets the picture; the on-device model loads
    // its projector first (a failure: its text instead).
    let mut picture = crate::snip::vision::reads_images(ep.provider, ep.model) != Some(false);
    if picture && ep.provider == crate::ai::local_llm::PROVIDER_ONDEVICE {
        if let Err(e) = crate::ai::local_llm::vision::ensure(app).await {
            tracing::info!("Voice Assistant: the on-device model can't read the window now; its text instead ({e})");
            picture = false;
        }
    }
    if picture {
        match crate::snip::turn::prepare(std::slice::from_ref(&image.id), ep.provider, ep.base_url).await {
            Ok(mut prepared) => {
                prepared.note = Some(PICTURE_NOTE.into());
                prepared.images = from_the_window(prepared.images);
                return Some(Look { images: prepared, question: None, took: true });
            }
            Err(e) => tracing::info!("Voice Assistant: the window's picture couldn't go; its text instead ({e})"),
        }
    }
    match crate::snip::ocr::question_with_text(std::slice::from_ref(&image.id), question).await {
        Ok((with_text, images)) => Some(Look {
            images: TurnImages { images: from_the_window(images), note: Some(TEXT_NOTE.into()), source: Some(text_source()), ..Default::default() },
            question: Some(with_text),
            took: true,
        }),
        Err(e) => {
            tracing::info!("Voice Assistant: no text read from the window ({e})");
            crate::snip::images::delete(std::slice::from_ref(&image.id));
            Some(Look::note(NO_WINDOW_NOTE.into()))
        }
    }
}

/// The question's screenshots as the window in front (`source` "window":
/// read back from disk they'd say "snip"). Pure.
fn from_the_window(images: Vec<ChatImage>) -> Vec<ChatImage> {
    images.into_iter().map(|i| ChatImage { source: "window".into(), ..i }).collect()
}

/// **The window in front, captured** (blocking): its visible frame cut from
/// every monitor captured at once, every Yap window left out; saved as a
/// chat screenshot. A test run cuts the first of its fake windows from its
/// fake monitors (`e2e_snip`), never the real screen.
fn capture(app: &AppHandle, hwnd: isize) -> Result<ChatImage, String> {
    use crate::snip::geom::Rect;
    let (frames, rect) = if crate::snip::faking() {
        let (frames, windows) = crate::snip::take_fake().ok_or("no fake monitors set (e2e_snip)")?;
        let rect = windows.first().copied().ok_or("no window in front")?;
        (frames, rect)
    } else {
        let rect = crate::snip::pick::window_frame(hwnd).ok_or("no window in front Yap can take")?;
        let monitors = crate::snip::capture::monitors();
        if monitors.is_empty() {
            return Err("no monitors found".into());
        }
        crate::calls::capture::for_snip(app, true);
        let (results, _) = crate::snip::capture::capture(&monitors, crate::snip::capture::Force::Auto);
        crate::calls::capture::for_snip(app, false);
        let frames: Vec<crate::snip::capture::Frame> = results.into_iter().filter_map(Result::ok).collect();
        if frames.is_empty() {
            return Err("Windows didn't let Yap capture the screen".into());
        }
        (frames, rect)
    };
    let places: Vec<Rect> = frames.iter().map(|f| Rect::new(f.monitor.x, f.monitor.y, f.monitor.width as i32, f.monitor.height as i32)).collect();
    let rect = rect.intersect(&crate::snip::geom::desktop(&places));
    let (rgba, w, h) = crate::snip::cut_frames(&frames, &rect)?;
    let (image, _png) = crate::snip::keep_as(&rgba, w, h, "window")?;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yes(q: &str) {
        assert_eq!(about(q), About::Yes, "{q}");
    }
    fn maybe(q: &str) {
        assert_eq!(about(q), About::Maybe, "{q}");
    }
    fn no(q: &str) {
        assert_eq!(about(q), About::No, "{q}");
    }

    #[test]
    fn a_question_that_points_at_the_screen_takes_it() {
        for q in [
            "What's this error?",
            "What’s this?",
            "what is this",
            "Summarise this page.",
            "summarize this for me",
            "What does this chart say?",
            "what does this weird error mean",
            "Can you explain this email?",
            "Translate this.",
            "What's on my screen?",
            "what am I looking at",
            "Is there a typo in this paragraph?",
            "how do I fix this stack trace",
            "What's wrong here?",
            "read this out to me",
            "Who sent these messages?",
        ] {
            yes(q);
        }
    }

    #[test]
    fn in_the_other_languages_too() {
        for q in [
            // French
            "Qu'est-ce que c'est ?",
            "C’est quoi ça ?",
            "Résume cette page.",
            "Que veut dire cette erreur ?",
            "Il y a quoi sur mon écran ?",
            "Explique-moi ça.",
            // German
            "Was ist das?",
            "Was bedeutet dieser Fehler?",
            "Fass mir das zusammen.",
            "Was steht auf dem Bildschirm?",
            // Spanish
            "¿Qué es esto?",
            "Resume esta página.",
            "¿Qué significa este error?",
            // Italian
            "Cos'è questo?",
            "Riassumi questa pagina.",
            "Cosa vuol dire quest'errore?",
            // Portuguese
            "O que é isto?",
            "Resume esta página.",
            "O que diz isso?",
            // Dutch
            "Wat is dit?",
            "Wat betekent deze foutmelding?",
            "Vat dit samen.",
        ] {
            yes(q);
        }
    }

    #[test]
    fn this_alone_is_the_models_call() {
        for q in ["Is this right?", "how do I fix this", "will this fail", "Can you help with this one?", "what does that error mean", "Est-ce que ça marche ?", "Wie repariere ich diesen?", "¿Esto funciona?", "Klopt dit?"] {
            maybe(q);
        }
    }

    #[test]
    fn everything_else_never_takes_the_screen() {
        for q in [
            "How long do I boil an egg?",
            "what time does it start",
            "What's on today?",
            "What's the weather this weekend?",
            "Remind me what we said this morning",
            "who has the slides",
            "Open Teams.",
            "When is the budget review this week?",
            "What should I read this weekend?",
            "Where do I find Gale in Baldur’s Gate 3?",
            "Quel temps fera-t-il ce soir ?",
            "Was steht diese Woche an?",
            "¿Qué tiempo hará esta noche?",
            "Che tempo fa questa sera?",
            "Wat staat er deze week op de agenda?",
            "",
        ] {
            no(q);
        }
    }

    #[test]
    fn a_this_counts_only_in_the_persons_own_languages() {
        let fr = vec!["fr".to_string()];
        let nl = vec!["nl".to_string()];
        let en_fr = vec!["en".to_string(), "fr".to_string()];
        // French "dit" ("said") isn't Dutch "dit" ("this") to a French speaker.
        let said = "Ce que tu as dit tout à l'heure, c'est vrai ?";
        assert_eq!(about_in(said, &fr), About::No);
        assert_eq!(about_in(said, &en_fr), About::No);
        assert_eq!(about_in("Klopt dit?", &nl), About::Maybe);
        assert_eq!(about_in("Klopt dit?", &fr), About::No);
        // English always counts; the words that point at the screen always do.
        assert_eq!(about_in("Is this right?", &fr), About::Maybe);
        assert_eq!(about_in("Wat betekent deze foutmelding?", &fr), About::No);
        assert_eq!(about_in("Wat is dit?", &fr), About::Yes);
        // None known: every language.
        assert_eq!(about_in("Klopt dit?", &[]), About::Maybe);
        assert_eq!(about_in("Klopt dit?", &["ja".to_string()]), About::Maybe);
    }

    #[test]
    fn the_models_word_is_yes_or_no() {
        for r in ["yes", "Yes.", "YES", " yes, it is", "\"yes\"", "Oui", "ja", "Sí"] {
            assert!(says_yes(r), "{r}");
        }
        for r in ["no", "No.", "", "Not really", "I can't tell", "maybe"] {
            assert!(!says_yes(r), "{r}");
        }
    }

    #[test]
    fn the_yes_or_no_request_carries_the_last_answer_and_little_thinking() {
        let m = ask_messages("what does this mean", Some("Paris is the capital of France."));
        assert_eq!(m[0]["content"], ASK_PROMPT);
        let user = m[1]["content"].as_str().unwrap();
        assert!(user.contains("last answer: \"Paris is the capital of France.\""), "{user}");
        assert!(user.ends_with("Is it about something on their screen? yes or no."));
        let first = ask_messages("what is this", None);
        assert!(first[1]["content"].as_str().unwrap().starts_with("The question: \"what is this\""));
        // A long answer: its start only.
        let long = ask_messages("q", Some(&"word ".repeat(400)));
        assert!(long[1]["content"].as_str().unwrap().len() < 600);
        assert_eq!(ask_extra("openai/gpt-oss-20b").get("reasoning_effort"), Some(&json!("low")));
        assert!(ask_extra("claude-haiku-5-5").is_empty());
    }

    #[test]
    fn the_window_goes_to_a_model_on_this_pc_and_to_the_cloud_only_when_allowed() {
        assert_eq!(allowed(true, false, false), Allowed::Yes);
        assert_eq!(allowed(true, false, true), Allowed::NotToTheCloud);
        assert_eq!(allowed(true, true, true), Allowed::Yes);
        assert_eq!(allowed(false, true, false), Allowed::Off);
        assert_eq!(allowed(false, true, true), Allowed::Off);
        // The cloud providers leave the PC whatever their address; any other
        // server unless it's on this PC.
        assert!(leaves_pc("groq", "http://127.0.0.1:9000/v1"));
        assert!(leaves_pc("anthropic", "https://api.anthropic.com/v1"));
        assert!(!leaves_pc("ondevice", "http://127.0.0.1:8080/v1"));
        assert!(!leaves_pc("local", "http://localhost:11434/v1"));
        assert!(leaves_pc("custom", "http://192.168.1.20:8080/v1"));
        assert!(leaves_pc("custom", "https://llm.example.com/v1"));
    }

    #[test]
    fn the_model_is_told_what_came_with_the_question() {
        assert!(PICTURE_NOTE.contains("window the user had in front"));
        assert!(not_sent_note("groq").contains("to Groq"));
        assert!(text_source().screen && text_source().url.is_empty());
    }
}
