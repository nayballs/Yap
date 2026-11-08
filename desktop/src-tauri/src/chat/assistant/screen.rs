//! **The Voice Assistant sees the window in front** (2026-10-29): a spoken
//! question about what's on screen — "what's this error?", "summarise this
//! page", "what does this chart say?" — takes a screenshot of the window
//! that was in front when the assistant's shortcut was pressed, and the
//! model answers from it.
//!
//! **Only when the words say so** ([`about_in`], pure, in the languages
//! Yap's speech models know best: English, French, German, Spanish,
//! Italian, Portuguese and Dutch): words that point at the screen ("on my
//! screen", "this error", "summarise this", "qu'est-ce que c'est", "dieser
//! Fehler") take the window at once; anything else never does here — "is
//! this right?", "how many people go to that one?", "how long do I boil an
//! egg". **No guessing** (the maintainer, 8 Nov 2026): a lone "this" or
//! "that" was once put to Chat's model in a one-second yes/no request with a
//! slice of the chat, and two spoken follow-ups about a convention just
//! discussed each sent a screenshot of the game in front. Now the answering
//! model, with the whole conversation, looks itself when it needs to
//! (`look_at_screen`, `chat/see_screen.rs`: away from Chat it takes this
//! window in front, [`take_in_front`]). An ask for something to be made —
//! "…in a document", "make me a guide to…", in the same languages
//! ([`asks_to_make`]) — has "this document" as the new one, never the
//! screen's.
//!
//! **What's taken** ([`capture`]): the window's visible frame
//! (`snip/pick.rs`, DWM's frame; the snip's window pick), cut from every
//! monitor captured at once the way a snip is (`snip/capture.rs`: HDR
//! right), with every Yap window left out for the moment of the capture
//! (`calls/capture.rs` `for_snip`) — the bar's capsule included — and Yap's
//! meeting windows kept out as the meeting setting wants (the Voice
//! Assistant is off during meetings anyway). When the window in front can't
//! be taken itself — one of Yap's own, minimised, cloaked, gone since the
//! press — the monitor it's on (else the pointer's), Yap's windows still
//! left out (`snip/pick.rs` `monitor_of`; why, at debug). With Yap's main
//! window in front a model that calls tools — your Claude plan's too, over
//! its run endpoint since 7 Nov — looks itself instead (`look_at_screen`:
//! every monitor and the page beside the chat). Saved like a snip
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
//! Assistant + the window in front · Groq", `privacy/net.rs`). The model's
//! own look away from Chat goes by the same rule ([`allowed`]: offered to a
//! cloud model only once allowed).
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
    /// Nothing that does — a "this" or "that" alone included: the answering
    /// model looks itself if it needs to (`look_at_screen`).
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

/// **Asks for something to be made** — a document ("…in a document",
/// "make me a document"), or a guide, a plan, a build to keep ("write me a
/// guide to…", "fais-moi un plan…") — in each language: the answer comes
/// from the model and the web, not the screen, so a "this" in it ("can you
/// build me this level by level… in a document") is never the screen's,
/// and "make this a document" is the new one (2026-10-31: a spoken
/// Divinity: Original Sin 2 build "in a document"). English's other asks for
/// something to keep are `documents::asks_for_document`'s.
static MAKES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?x)",
        // English
        r"\b(in|into|as|on)\ (a|an|one)\ (new\ |separate\ |proper\ )?(document|doc)\b",
        r"|\b(make|write|create|start|draft|open|give)\ (me\ |us\ )?(a|an)\ (new\ )?(document|doc)\b",
        r"|\b(make|turn|put|write|save)\ (this|that|it)\ (up\ )?(into\ |as\ |in\ )?(a|an)\ (new\ )?(document|doc)\b",
        // French
        r"|\b(dans|en|sous\ forme\ d')\ ?un\ (nouveau\ )?document\b",
        r"|\b(fais|faites|écris|écrivez|crée|créez|rédige|rédigez|prépare|préparez|donne|donnez)(-moi|-nous)?\ (un|une|mon|ma)\ [^.?!]{0,60}?\b(document|guide|plan|build|itinéraire|liste|programme|planning|comparatif|tutoriel)\b",
        // German
        r"|\bin\ (ein|einem|ein\ neues|einem\ neuen)\ dokument\b|\bals\ dokument\b",
        r"|\b(erstell|erstelle|schreib|schreibe|mach|mache|gib)(\ mir|\ uns)?\ (ein|eine|einen|mein|meine)\ [^.?!]{0,60}?\b(dokument|anleitung|leitfaden|guide|plan|build|reiseplan|liste|vergleich|tutorial)\b",
        // Spanish
        r"|\ben\ un\ (nuevo\ )?documento\b",
        r"|\b(haz|hazme|escribe|escríbeme|crea|créame|redacta|prepara|prepárame|dame)\ (un|una|mi)\ [^.?!]{0,60}?\b(documento|guía|plan|build|itinerario|lista|comparativa|tutorial)\b",
        // Italian
        r"|\bin\ un\ (nuovo\ )?documento\b",
        r"|\b(fai|fammi|scrivi|scrivimi|crea|creami|prepara|preparami|dammi)\ (un|una|uno)\ [^.?!]{0,60}?\b(documento|guida|piano|build|itinerario|lista|elenco|confronto|tutorial)\b",
        // Portuguese
        r"|\b(num|em\ um|em\ uma)\ (novo\ )?documento\b",
        r"|\b(faz|faça|faz-me|escreve|escreva|cria|crie|prepara|prepare|dá-me)\ (um|uma)\ [^.?!]{0,60}?\b(documento|guia|plano|build|roteiro|lista|comparação|tutorial)\b",
        // Dutch
        r"|\bin\ een\ (nieuw\ )?document\b",
        r"|\b(maak|schrijf|geef)(\ me|\ mij|\ ons)?\ (een|mijn)\ [^.?!]{0,60}?\b(document|gids|handleiding|plan|build|reisplan|lijst|vergelijking|tutorial)\b",
    ))
    .expect("static regex")
});

/// "Document" in each language: after "this" in an ask to make one ("make
/// this a document") it's the new document, not one on screen.
const DOCUMENT_WORDS: &[&str] = &["document", "doc", "dokument", "documento"];

/// "This" in each language (its code, as My languages has it): before a
/// screen word, the question is about the screen ("that" never is: it's
/// mostly something said earlier). Only the person's own languages count
/// (English always): Dutch "dit" is French for "said".
const DEMONSTRATIVES: &[(&str, &[&str])] = &[
    ("en", &["this", "these"]),
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

/// Does a spoken question ask for something to be made — a document, or a
/// guide, plan or build to keep ([`MAKES`], `documents::asks_for_document`)?
/// Then "this document" in it is the new one, never the screen's
/// ([`about_in`]). Pure.
pub fn asks_to_make(question: &str) -> bool {
    let text = normal(question);
    MAKES.is_match(&text) || crate::chat::documents::asks_for_document(question)
}

/// [`about`] in the person's own languages (My languages' codes; none: all
/// of them): **yes only when its words point at the screen** — a phrase
/// ("on my screen", "what's this?", "summarise what's on…"), "this" before a
/// screen word ("this error", "cette page", "dieser Fehler"), or a verb
/// before "this" ("summarise this", "explain these"). Anything else is no: a
/// lone "this" or "that" ("is this right?", "how many go to that one?") is
/// the answering model's to look at if it needs to (`look_at_screen`), with
/// the whole conversation in view. **An ask for something to be made**
/// ([`asks_to_make`]: "…in a document", "make me a guide…"): "this
/// document" in it is the new one — only words that point at the screen
/// itself ("on my screen", "summarise this page into a document") take it.
/// Pure.
pub fn about_in(question: &str, langs: &[String]) -> About {
    let text = normal(question);
    let text = text.trim_end_matches(['?', '.', '!', ' ']);
    if SCREEN_PHRASES.is_match(text) {
        return About::Yes;
    }
    let making = asks_to_make(question);
    let demonstratives = demonstratives(langs);
    let w = words(text);
    for (i, word) in w.iter().enumerate() {
        if !demonstratives.contains(word) {
            continue;
        }
        let next = w.get(i + 1).copied();
        if next.is_some_and(|n| NOT_AFTER.contains(&n)) {
            continue;
        }
        // "this error", "this weird error", "cette page", "dieser Fehler"
        // (not "make this a document": that's the one to make).
        let screen_word = [w.get(i + 1), w.get(i + 2)].into_iter().flatten().any(|n| SCREEN_WORDS.contains(n) && !(making && DOCUMENT_WORDS.contains(n)));
        if screen_word {
            return About::Yes;
        }
        // "summarise this", "explain these".
        if (*word == "this" || *word == "these") && i > 0 && VERBS.contains(&w[i - 1]) {
            return About::Yes;
        }
    }
    About::No
}

// ---- may it go? -----------------------------------------------------------------------------

/// Whether the window may go to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allowed {
    Yes,
    /// It's a cloud model and "…with cloud models too" is off.
    NotToTheCloud,
    /// "Let Yap's AI see your screen" is off.
    Off,
}

/// Does a call to `provider` at `base_url` leave this PC? The cloud
/// providers always do (whatever their address says); any other server
/// unless it's on this PC. Pure.
pub fn leaves_pc(provider: &str, base_url: &str) -> bool {
    crate::ai::ai_model::KEYED_PROVIDERS.contains(&provider) || !crate::ai::ai_model::runs_here(provider, base_url)
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
pub(crate) fn ask_once(app: &AppHandle, provider: &str) {
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
            title: "Let Yap's AI see your screen?".into(),
            body: format!(
                "You asked about your screen, and Chat's model runs on {who}, so a screenshot would go there. Yap answered without it. It only ever looks when you ask about something on your screen."
            ),
            action: Some(crate::notices::alert::Action { id: crate::notices::alert::ALLOW_SCREEN_CLOUD, label: "Allow" }),
            timeout_ms: 20_000,
        },
    );
}

/// The ask's alert kind (its rule in `notice.rs`).
pub const KIND: &str = "assistantScreen";

/// Was the ask already shown (`assistant_screen_asked`)?
pub(crate) fn asked_before(cfg: &crate::config::YapConfig) -> bool {
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
        "The user asked about something on their screen, but Yap didn't send a screenshot: they haven't let Yap send their screen to {}. Say in one short sentence that you can't see their screen, and that Settings → Voice Assistant can allow it.",
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

/// **A spoken question, looked at**: `None` when its words don't point at
/// the screen ([`about_in`]), or the setting's off; else what goes with it.
/// No model is asked first. `looking(true)` while the window is taken and
/// read (the capsule says so).
pub async fn look(app: &AppHandle, question: &str, cfg: &crate::config::YapConfig, ep: Endpoint<'_>, looking: &(dyn Fn(bool) + Send + Sync)) -> Option<Look> {
    if about_in(question, &crate::dictation::languages::my_languages(cfg)) == About::No {
        return None;
    }
    let leaves = leaves_pc(ep.provider, ep.base_url);
    let may = allowed(cfg.assistant_screen, cfg.assistant_screen_cloud, leaves);
    tracing::info!(?may, "Voice Assistant: a question about the screen");
    match may {
        Allowed::Off => return None,
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
    let out = take(app, question, &ep).await;
    looking(false);
    out
}

/// [`look`]'s work once the window may go: the capture, then its picture or
/// its text.
async fn take(app: &AppHandle, question: &str, ep: &Endpoint<'_>) -> Option<Look> {
    let image = match take_in_front(app).await {
        Ok((image, _png)) => image,
        Err(e) => {
            tracing::info!("Voice Assistant: no screenshot of the window in front ({e})");
            return Some(Look::note(NO_WINDOW_NOTE.into()));
        }
    };
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

/// **The window that was in front at the shortcut's press, taken now**
/// ([`capture`] off the async threads): its shot kept as a chat screenshot
/// (source "window") and its PNG. The clear words' look here, and the
/// model's own `look_at_screen` for a spoken question asked away from Chat
/// (`chat/see_screen.rs`). The log has its size and time only.
pub(crate) async fn take_in_front(app: &AppHandle) -> Result<(ChatImage, Vec<u8>), String> {
    let hwnd = WINDOW.load(Ordering::SeqCst);
    let capture_app = app.clone();
    let t = std::time::Instant::now();
    let taken = tauri::async_runtime::spawn_blocking(move || capture(&capture_app, hwnd)).await.map_err(|e| format!("the capture didn't finish ({e})"))??;
    tracing::info!(w = taken.0.w, h = taken.0.h, ms = t.elapsed().as_millis() as u64, "Voice Assistant: took the window in front");
    Ok(taken)
}

/// The window `hwnd`, captured as the Voice Assistant takes it ([`capture`]):
/// a Command that reads the window in front and got next to no text from
/// it (`dictation/command_screen.rs`), for a model that reads images. Saved
/// as a chat screenshot; the caller deletes it once sent. Blocking.
pub(crate) fn capture_window(app: &AppHandle, hwnd: isize) -> Result<ChatImage, String> {
    capture(app, hwnd).map(|(image, _png)| image)
}

/// **The window in front, captured** (blocking): its visible frame cut from
/// every monitor captured at once, every Yap window left out; saved as a
/// chat screenshot, with its PNG. A test run cuts the first of its fake
/// windows from its fake monitors (`e2e_snip`), never the real screen.
fn capture(app: &AppHandle, hwnd: isize) -> Result<(ChatImage, Vec<u8>), String> {
    use crate::snip::geom::Rect;
    let (frames, rect) = if crate::snip::faking() {
        let (frames, windows) = crate::snip::take_fake().ok_or("no fake monitors set (e2e_snip)")?;
        let rect = windows.first().copied().ok_or("no window in front")?;
        (frames, rect)
    } else {
        // The window in front at the press; when it can't be taken itself
        // (Yap's own, minimised, cloaked, gone since the press…), the monitor
        // it's on, Yap's windows still left out — never nothing (8 Nov 2026:
        // "couldn't take a screenshot" while `look_at_screen` worked a minute
        // later). Why, at debug: a few words, never a title.
        let rect = match crate::snip::pick::window_frame(hwnd) {
            Ok(rect) => rect,
            Err(why) => {
                tracing::debug!(why, "Voice Assistant: the window in front can't be taken itself; its monitor instead");
                crate::snip::pick::monitor_of(hwnd).ok_or("no window in front Yap can take, nor its monitor")?
            }
        };
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
    crate::snip::keep_as(&rgba, w, h, "window")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yes(q: &str) {
        assert_eq!(about(q), About::Yes, "{q}");
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

    /// **No guessing** (the maintainer, 8 Nov 2026): a lone "this" or
    /// "that" was a maybe, put to Chat's model in a one-second yes/no
    /// request; two spoken follow-ups about a convention just discussed each
    /// sent a screenshot of the game in front. Now they take nothing: the
    /// answering model looks itself if it needs to (`look_at_screen`).
    #[test]
    fn a_lone_this_or_that_takes_nothing_the_answering_model_looks_if_it_needs_to() {
        for q in [
            // The two follow-ups from 8 Nov 2026.
            "The one that's on the 5th to the 8th of February, where is it and can you bring up the website in the browser for it?",
            "How many people usually attend that specific one? Can you look?",
            // What were maybes.
            "Is this right?",
            "how do I fix this",
            "will this fail",
            "Can you help with this one?",
            "what does that error mean",
            "Is that page still up?",
            "Est-ce que ça marche ?",
            "Wie repariere ich diesen?",
            "¿Esto funciona?",
            "Klopt dit?",
            // A follow-up about something just discussed (fictional).
            "Which of those is nearest to Kellbrook?",
            "Can you look up when that one opens?",
        ] {
            no(q);
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

    /// **An ask for something to be made** (2026-10-31: a spoken ask for a
    /// build "in a document"): its "this", and "this document" in it, are
    /// never the screen's, in every language the rules know; words that
    /// point at the screen itself still take it.
    #[test]
    fn an_ask_for_a_document_never_takes_the_screen() {
        for q in [
            // English: the maintainer's ask, and its kind.
            "Hey, can you build me this level by level, a solo Lone Wolf Hydro Aero mage for Divinity Original Sin 2, with civil skills and the best Act 1 gear and where to get it, in a document?",
            "Can you put this in a document?",
            "Make this a document please.",
            "Write me a guide to this boss fight.",
            "Make me a packing list for this trip.",
            // French
            "Tu peux me mettre ça dans un document ?",
            "Fais-moi un guide pour cette classe, niveau par niveau.",
            // German
            "Kannst du mir diesen Build Stufe für Stufe in einem Dokument zusammenstellen?",
            "Erstelle mir einen Plan für diese Woche.",
            // Spanish
            "¿Puedes ponerme esto en un documento?",
            "Hazme una guía para esta build.",
            // Italian
            "Puoi mettermi questo in un documento?",
            "Fammi una guida per questa classe.",
            // Portuguese
            "Podes pôr isto num documento?",
            "Faz-me um guia para esta classe.",
            // Dutch
            "Kun je dit in een document zetten?",
            "Maak een plan voor deze reis.",
        ] {
            assert!(asks_to_make(q), "{q}");
            no(q);
        }
        // "Make this a document" is the one to make; "this document" in
        // another question is the one on screen…
        yes("Is there a typo in this document?");
        // …and words that point at the screen still take it.
        yes("Summarise this page into a document.");
        yes("Put what's on my screen in a document.");
        yes("Résume cette page dans un document.");
        assert!(!asks_to_make("What's this error?"));
        assert!(!asks_to_make("Is this right?"));
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
        assert_eq!(about_in("Wat betekent deze foutmelding?", &nl), About::Yes);
        assert_eq!(about_in("Klopt dit bericht?", &nl), About::Yes);
        assert_eq!(about_in("Klopt dit bericht?", &fr), About::No);
        // English always counts; the words that point at the screen always do.
        assert_eq!(about_in("Is this error new?", &fr), About::Yes);
        assert_eq!(about_in("Wat betekent deze foutmelding?", &fr), About::No);
        assert_eq!(about_in("Wat is dit?", &fr), About::Yes);
        // None known: every language.
        assert_eq!(about_in("Klopt dit bericht?", &[]), About::Yes);
        assert_eq!(about_in("Klopt dit bericht?", &["ja".to_string()]), About::Yes);
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
