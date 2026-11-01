//! **Which free sources a search asks** ([`route`], pure): a cheap rule on
//! the words, in the person's languages where Yap has them (English always;
//! the weather, prices, the news and "where is" also in French, German,
//! Spanish, Italian, Portuguese and Dutch, as `command_screen.rs` has its
//! rules), read first on **the model's own query** — its choice of what to
//! search ("weather Glenbrackie tomorrow") — and on the person's question
//! only when the query says nothing (a game the query left out, "And
//! tomorrow?").
//!
//! Order matters: a game's question goes to its wiki even when it asks a
//! price ("How much does the Blood of Lathander cost?" is in-game gold), with
//! the browser offered too ([`Route::offer`]); a programming question that
//! says "best" is still a programming question.

use std::sync::LazyLock;

use regex::Regex;

use super::super::wiki::game_for;

/// The kind of question, as the free sources see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Weather,
    /// Where a place is, its address.
    Place,
    /// An error, a how-to in a language or tool.
    Programming,
    /// A library, a tool, a repository.
    Code,
    /// Research papers.
    Papers,
    /// A game Yap knows a wiki for.
    Game,
    /// Anything else: Wikipedia.
    Facts,
    /// None of the free sources covers it.
    NotCovered(NotCovered),
}

/// What the free sources don't cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotCovered {
    Prices,
    News,
    Reviews,
    Timetables,
    Local,
    Recipes,
}

impl NotCovered {
    /// In a sentence: "don't cover {what}".
    pub fn what(self) -> &'static str {
        match self {
            NotCovered::Prices => "prices",
            NotCovered::News => "the news",
            NotCovered::Reviews => "reviews and rankings",
            NotCovered::Timetables => "travel tickets, timetables and opening hours",
            NotCovered::Local => "local businesses",
            NotCovered::Recipes => "recipes",
        }
    }
}

/// Where a search goes: its kind, and — when a free source still has a go
/// at a question it may not answer (a game's price) — the browser offered
/// too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub kind: Kind,
    pub offer: Option<NotCovered>,
}

/// One language's rules (the text lower-cased, apostrophes straight).
struct Lang {
    code: &'static str,
    weather: &'static str,
    prices: &'static str,
    news: &'static str,
    place: &'static str,
}

const LANGS: &[Lang] = &[
    Lang {
        code: "en",
        weather: concat!(
            r"\b(weather|forecast|forecasts)\b",
            r"|\bwill it (rain|snow|be (sunny|windy|cold|hot|warm|wet|dry|frosty|icy|stormy))\b",
            r"|\b(is it|is it going to be|it's going to be) (raining|snowing|sunny|windy|cold|hot|warm|frosty|icy|stormy)\b",
            r"|\b(temperature|degrees|rain|snow|wind) (today|tonight|tomorrow|outside|right now|this (morning|afternoon|evening|weekend))\b",
            r"|\bdo i need (an umbrella|a coat|sunscreen)\b",
        ),
        prices: "", // English uses `asks_about_prices` (grounding.rs), the price rule Yap already has.
        news: concat!(
            r"\b(news|headlines|breaking)\b",
            r"|\bwho (won|is winning|scored|got knocked out)\b",
            r"|\b(final )?(score|scores|result|results) (of|for|in) (the|last night|today|yesterday)\b",
            r"|\bwhat happened (today|yesterday|this week|last night|in the)\b",
            r"|\b(election|match|game) results?\b",
        ),
        place: concat!(
            r"^(where is|where's|where are|wheres)\b",
            r"|\b(address|location|coordinates|postcode|post code|zip code) (of|for)\b",
            r"|\bwhich (country|county|state|region|province|city|town|continent) is\b",
            r"|\bwhere (exactly )?(is|are) [a-z]",
            r"|\b(located|situated)\b",
        ),
    },
    Lang {
        code: "fr",
        weather: r"\b(météo|meteo|prévisions météo)\b|\b(va-t-il|est-ce qu'il va) (pleuvoir|neiger)\b|\bquel temps (fait-il|va-t-il faire|fera-t-il|il fait|fait il)\b|\btemps qu'il fait\b|\btempérature (aujourd'hui|demain|ce soir|dehors)\b",
        prices: r"\b(prix|tarif|tarifs|coûte|coute|coûtent|abonnement|abonnements|promo|promotion|soldes)\b|\bcombien (coûte|coute|ça coûte|pour)\b",
        news: r"\b(actualités|actualité|actus|infos du jour|nouvelles|dernières nouvelles|titres)\b|\bqui a gagné\b",
        place: r"^(où est|où se trouve|où sont|où se situe|ou est)\b|\badresse (de|du|des)\b|\bse (trouve|situe)\b",
    },
    Lang {
        code: "de",
        weather: r"\b(wetter|wettervorhersage|vorhersage)\b|\bwird es (regnen|schneien)\b|\bregnet es\b|\btemperatur (heute|morgen|draußen)\b",
        prices: r"\b(preis|preise|kostet|kosten|abo|abonnement|angebot|rabatt)\b|\bwie viel kostet\b|\bwieviel kostet\b",
        news: r"\b(nachrichten|neuigkeiten|schlagzeilen|aktuelles)\b|\bwer hat gewonnen\b",
        place: r"^(wo ist|wo liegt|wo sind|wo befindet sich)\b|\badresse (von|der|des)\b|\b(liegt|befindet sich) in\b",
    },
    Lang {
        code: "es",
        weather: r"\b(pronóstico del tiempo|pronostico del tiempo|pronóstico|clima)\b|\b(lloverá|nevará|va a llover|va a nevar)\b|\bqué tiempo hace\b|\bqué tiempo va a hacer\b|\btemperatura (hoy|mañana|esta noche|afuera)\b",
        prices: r"\b(precio|precios|cuesta|cuestan|tarifa|tarifas|suscripción|oferta|ofertas|descuento)\b|\bcuánto (cuesta|vale|cuestan)\b",
        news: r"\b(noticias|titulares|últimas noticias|actualidad)\b|\bquién ganó\b",
        place: r"^(dónde está|donde esta|dónde queda|dónde se encuentra|dónde están)\b|\bdirección de\b|\bse encuentra en\b",
    },
    Lang {
        code: "it",
        weather: r"\b(meteo|previsioni del tempo|previsioni meteo)\b|\b(pioverà|nevicherà)\b|\bche tempo\b|\btemperatura (oggi|domani|stasera|fuori)\b",
        prices: r"\b(prezzo|prezzi|costa|costano|tariffa|tariffe|abbonamento|offerta|offerte|sconto)\b|\bquanto (costa|costano|viene)\b",
        news: r"\b(notizie|ultime notizie|titoli|cronaca)\b|\bchi ha vinto\b",
        place: r"^(dove si trova|dov'è|dove è|dove sono)\b|\bindirizzo (di|del|della)\b|\bsi trova (a|in)\b",
    },
    Lang {
        code: "pt",
        weather: r"\b(previsão do tempo|previsao do tempo|meteorologia)\b|\bcomo (está|vai estar) o tempo\b|\b(vai chover|vai nevar)\b|\btemperatura (hoje|amanhã|esta noite|lá fora)\b",
        prices: r"\b(preço|preços|preco|custa|custam|tarifa|assinatura|oferta|promoção|desconto)\b|\bquanto (custa|custam|é)\b",
        news: r"\b(notícias|noticias|manchetes|últimas notícias)\b|\bquem ganhou\b",
        place: r"^(onde fica|onde está|onde é|onde ficam)\b|\bendereço (de|do|da)\b|\bfica em\b",
    },
    Lang {
        code: "nl",
        weather: r"\b(weer|weerbericht|weersverwachting|weersvoorspelling)\b|\bgaat het (regenen|sneeuwen)\b|\btemperatuur (vandaag|morgen|buiten)\b",
        prices: r"\b(prijs|prijzen|kost|kosten|abonnement|aanbieding|korting)\b|\bhoeveel kost\b",
        news: r"\b(nieuws|nieuwsberichten|krantenkoppen)\b|\bwie (heeft gewonnen|won)\b",
        place: r"^(waar is|waar ligt|waar zijn|waar bevindt)\b|\badres van\b|\bligt in\b",
    },
];

/// A language's four rules (weather, prices, news, place), compiled.
type Compiled = (&'static str, [Option<Regex>; 4]);

/// Each language's rules, compiled once (an empty rule never matches).
static RULES: LazyLock<Vec<Compiled>> = LazyLock::new(|| {
    let re = |s: &str| (!s.is_empty()).then(|| Regex::new(s).expect("static regex"));
    LANGS.iter().map(|l| (l.code, [re(l.weather), re(l.prices), re(l.news), re(l.place)])).collect()
});

/// Code that looks like code: a backtick, `::`, `()`, `=>`, `->`, a source
/// file's name, a `--flag`, an error code ("E0502", "TS2345"), a
/// snake_case or camelCase name.
static CODE_LIKE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"`|::|\(\)|=>|->|\b\w+\.(py|js|ts|tsx|jsx|rs|go|java|kt|cs|cpp|c|h|rb|php|swift|sh|ps1|toml|yaml|yml|json)\b|(^|\s)--[a-z][a-z-]+|\b(E|TS|CS|C|LNK|MSB)\d{4}\b|\bORA-\d{5}\b|\b[a-z]+_[a-z_]+\b|\b[a-z]{2,}[A-Z][a-z]+[A-Za-z]*\b")
        .expect("static regex")
});

/// Words only programming uses (the text lower-cased).
static PROGRAMMING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:^|\s)(c\+\+|c#)(?:\s|[?.,!]|$)",
        r"|\b(javascript|typescript|python3|kotlin|haskell|php|golang|rustc|rustup|node\.?js|npm|npx|yarn (install|add)|pnpm|pip install|pip3|pypi|conda|virtualenv|venv",
        r"|flask app|fastapi|react(\.js| hook| component| native)|vue\.js|angularjs|angular component|svelte|next\.js|tauri|electron app|webpack|vite",
        r"|docker|dockerfile|kubernetes|kubectl|k8s|terraform|ansible|nginx|apache server|sql|mysql|postgres(ql)?|sqlite|mongodb|redis",
        r"|git (push|pull|merge|rebase|commit|clone|stash|checkout|branch)|pull request|merge conflict|github actions|gitlab|bitbucket",
        r"|regex|regexp|regular expression|json|yaml|toml|html|css|tailwind|bash (script|command)|in bash|zsh|powershell|cmd\.exe|shell script|linux command|ubuntu|debian|wsl",
        r"|vs ?code|visual studio|intellij|xcode|android studio|jupyter|pandas dataframe|numpy|tensorflow|pytorch",
        r"|compiler|compile error|stack ?trace|traceback|segfault|segmentation fault|null pointer|nullpointerexception|syntax error",
        r"|typeerror|valueerror|keyerror|indexerror|attributeerror|importerror|modulenotfounderror|referenceerror|undefined is not|cannot find module",
        r"|borrow checker|lifetime parameter|async/await|unit test|stack overflow|stackoverflow|api endpoint|rest api|graphql|sdk|cli flag)\b",
    ))
    .expect("static regex")
});

/// A language or tool whose name is also an ordinary word ("rust", "go",
/// "swift"…): programming only beside a word that says so.
static AMBIGUOUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(python|rust|go|swift|ruby|java|dart|julia|elixir|crystal|scala|perl|lua|cargo|git|django)\b").expect("static regex"));
static DEV_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(error|errors|exception|compiler|code|coding|function|functions|install|installing|library|crate|crates|package|packages|module|modules|syntax|programming|script|array|vector|string|strings|variable|method|methods|struct|trait|enum|bug|debug|build|import|framework|api|macro|lambda|closure|async|generics|iterator|hashmap|comprehension|dataframe|decorator|tutorial|in (python|rust|go|swift|ruby|java|dart|scala|lua))\b")
        .expect("static regex")
});

/// Asking for a library, a tool or a repository: GitHub's repositories.
static CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\bgithub\b|\b(repo|repos|repository|repositories)\b|\bopen[- ]source (library|libraries|tool|tools|alternative|alternatives|project|projects|app|apps|version)\b",
        r"|\b(js|javascript|typescript|python|rust|go|java|kotlin|c#|c\+\+|swift|php|ruby|node|react|svelte|vue|dotnet|\.net) (library|libraries|package|packages|module|modules|framework|frameworks|crate|crates|gem|gems)\b",
        r"|\b(crate|crates) (for|to|that)\b|\bnpm package\b|\bsource code (of|for)\b",
    ))
    .expect("static regex")
});

/// Research papers: arXiv.
static PAPERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\barxiv\b|\b(paper|papers|preprint|preprints|study|studies|research) (on|about|into|showing|that show|by|from)\b|\b(research|scientific|academic|recent|latest) (papers?|preprints?|research|studies)\b")
        .expect("static regex")
});

/// Reviews and rankings, timetables and opening hours, local businesses,
/// recipes (English).
static REVIEWS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\breviews?\b|\bratings?\b|\bworth (it|buying|the money)\b|\bwhich (one )?should i (buy|get|choose|pick)\b|\b(best|top) (\d+ |ten |five )?[a-z' -]{0,40}\b(of|in|for) (20\d\d|this year|the year)\b|\b(best|top) (\d+|ten|five|10|5)\b|\b(best|top)\b[a-z' -]{0,50}\b20\d\d\b")
        .expect("static regex")
});
static TIMETABLES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(timetables?|train times|bus times|departures?|arrivals?|next (train|bus|tram|ferry|flight)|opening (hours|times)|open (now|today|tomorrow|on (sunday|monday|tuesday|wednesday|thursday|friday|saturday))|what time does .{1,40} (open|close|leave|depart|arrive|start|kick off))\b")
        .expect("static regex")
});
/// Travel: tickets, fares and journeys ("train tickets from Kellbrook to
/// Port Ellis", "flights to Lisbon") — what a timetable's site or a booking
/// site answers, never a wiki (a tester's "train tickets from … to Perth"
/// got wrestling pages, 1 Nov 2026).
static TRAVEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b((train|rail|railway|bus|coach|ferry|plane|air|flight) (tickets?|fares?|journeys?|routes?|connections?|times?)|tickets? (from|to|between)|fares? (from|to|between)|(flights?|trains?|buses|coaches|ferries) (from|to|between))\b")
        .expect("static regex")
});
static LOCAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bnear me\b|\bnearby\b|\baround here\b|\b(nearest|closest) [a-z]+\b|\b(restaurants?|cafes?|pubs?|bars?|shops?|stores?|supermarkets?|pharmac(y|ies)|chemists?|dentists?|garages?|hotels?|takeaways?) (in|near|around)\b")
        .expect("static regex")
});
static RECIPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\brecipes?\b|\bhow (do i|to|long (do|to|should) i) (cook|bake|roast|boil|fry|grill|poach|steam)\b").expect("static regex")
});

/// The text as the rules read it: lower case, straight apostrophes, one
/// space between words, its end's punctuation off. Pure.
pub(super) fn normal(text: &str) -> String {
    let t = text.to_lowercase().replace(['\u{2019}', '\u{2018}', '`'], "'");
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    t.trim_matches(|c: char| matches!(c, '?' | '.' | '!' | '"' | '\'' | ',' | ':') || c.is_whitespace()).to_string()
}

/// What one text's words say: the kind a free source covers, and one none
/// covers.
#[derive(Debug, Default)]
struct Signals {
    covered: Option<Kind>,
    uncovered: Option<NotCovered>,
}

fn signals(text: &str, langs: &[String]) -> Signals {
    let lower = normal(text);
    if lower.is_empty() {
        return Signals::default();
    }
    let all = langs.iter().all(|c| !LANGS.iter().any(|l| c.to_ascii_lowercase().starts_with(l.code)));
    let mine = |code: &str| all || code == "en" || langs.iter().any(|c| c.to_ascii_lowercase().starts_with(code));
    let rules: Vec<&[Option<Regex>; 4]> = RULES.iter().filter(|(code, _)| mine(code)).map(|(_, r)| r).collect();
    let any = |i: usize| rules.iter().any(|r| r[i].as_ref().is_some_and(|re| re.is_match(&lower)));
    let prices = super::super::asks_about_prices(text) || any(1);
    let programming = PROGRAMMING.is_match(&lower) || CODE_LIKE.is_match(text) || (AMBIGUOUS.is_match(&lower) && DEV_WORDS.is_match(&lower));
    let covered = if any(0) {
        Some(Kind::Weather)
    } else if game_for(text).is_some() {
        Some(Kind::Game)
    } else if CODE.is_match(&lower) {
        Some(Kind::Code)
    } else if programming {
        Some(Kind::Programming)
    } else if PAPERS.is_match(&lower) {
        Some(Kind::Papers)
    } else if any(3) && !LOCAL.is_match(&lower) {
        Some(Kind::Place)
    } else {
        None
    };
    let uncovered = if prices {
        Some(NotCovered::Prices)
    } else if any(2) {
        Some(NotCovered::News)
    } else if TIMETABLES.is_match(&lower) || TRAVEL.is_match(&lower) {
        Some(NotCovered::Timetables)
    } else if LOCAL.is_match(&lower) {
        Some(NotCovered::Local)
    } else if RECIPES.is_match(&lower) {
        Some(NotCovered::Recipes)
    } else if REVIEWS.is_match(&lower) {
        Some(NotCovered::Reviews)
    } else {
        None
    };
    Signals { covered, uncovered }
}

/// **Where a keyless search goes**: the model's `query` read first, the
/// person's `question` when the query says nothing (or for the game it
/// names); `langs`: the person's languages (English always). A kind a free
/// source covers wins — the price of something in a game goes to its wiki
/// with the browser offered too —, else one none covers, else Wikipedia.
/// Pure.
pub fn route(query: &str, question: &str, langs: &[String]) -> Route {
    let q = signals(query, langs);
    let s = if q.covered.is_some() || q.uncovered.is_some() { q } else { signals(question, langs) };
    // The game a question names, though the model's query leaves it out
    // ("Gale location" for "Where is Gale in BG3?").
    let covered = match s.covered {
        Some(k) => Some(k),
        None if game_for(question).is_some() => Some(Kind::Game),
        None => None,
    };
    match (covered, s.uncovered) {
        (Some(kind), offer) => Route { kind, offer },
        (None, Some(what)) => Route { kind: Kind::NotCovered(what), offer: None },
        (None, None) => Route { kind: Kind::Facts, offer: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en() -> Vec<String> {
        vec!["en".into()]
    }
    fn kind(query: &str, question: &str) -> Kind {
        route(query, question, &en()).kind
    }

    #[test]
    fn the_weather_goes_to_open_meteo() {
        for q in [
            "What's the weather like in Glenbrackie today?",
            "weather Glenbrackie tomorrow",
            "Will it rain in Port Ellis this weekend?",
            "forecast for Kellbrook",
            "Is it going to be cold tonight in Fellshire?",
            "Do I need an umbrella tomorrow?",
        ] {
            assert_eq!(kind(q, q), Kind::Weather, "{q}");
        }
        // "Purple Rain" isn't the weather; nor "a temperature of 40 in a child".
        assert_ne!(kind("Who wrote Purple Rain?", "Who wrote Purple Rain?"), Kind::Weather);
        // In the person's language: French with French, not with English only.
        let fr = vec!["fr".to_string(), "en".to_string()];
        assert_eq!(route("météo Glenbrackie demain", "", &fr).kind, Kind::Weather);
        assert_eq!(route("Quel temps fait-il à Port Ellis ?", "", &fr).kind, Kind::Weather);
        assert_eq!(route("Wie wird das Wetter morgen in Kellbrook?", "", &["de".to_string()]).kind, Kind::Weather);
        // Dutch "weer" (also "again") only for a Dutch speaker.
        assert_eq!(route("weer Kellbrook morgen", "", &["nl".to_string()]).kind, Kind::Weather);
        assert_ne!(route("weer Kellbrook morgen", "", &fr).kind, Kind::Weather);
    }

    #[test]
    fn what_no_free_source_covers_is_handed_to_the_browser() {
        let not = |q: &str| kind(q, q);
        assert_eq!(not("How much is a Grok subscription?"), Kind::NotCovered(NotCovered::Prices));
        assert_eq!(not("Grok subscription price UK"), Kind::NotCovered(NotCovered::Prices));
        assert_eq!(not("What's in the news today?"), Kind::NotCovered(NotCovered::News));
        assert_eq!(not("Who won the match last night?"), Kind::NotCovered(NotCovered::News));
        assert_eq!(not("best indie games of 2026"), Kind::NotCovered(NotCovered::Reviews));
        assert_eq!(not("Emberfall Deluxe reviews"), Kind::NotCovered(NotCovered::Reviews));
        assert_eq!(not("next train from Port Ellis to Kellbrook"), Kind::NotCovered(NotCovered::Timetables));
        // Travel: tickets, fares, journeys (never a wiki's guess).
        for q in ["train tickets from Kellbrook to Port Ellis", "Can you search for train tickets from Kellbrook to Port Ellis?", "flights to Port Ellis", "cheapest fares from Kellbrook", "buses between Kellbrook and Glenbrackie"] {
            assert!(matches!(not(q), Kind::NotCovered(NotCovered::Timetables | NotCovered::Prices)), "{q}");
        }
        assert_eq!(not("What are the opening hours of Glenbrackie library?"), Kind::NotCovered(NotCovered::Timetables));
        assert_eq!(not("pizza near me"), Kind::NotCovered(NotCovered::Local));
        assert_eq!(not("restaurants in Kellbrook"), Kind::NotCovered(NotCovered::Local));
        assert_eq!(not("a recipe for shortbread"), Kind::NotCovered(NotCovered::Recipes));
        // Other languages, for a speaker of them.
        let es = vec!["es".to_string()];
        assert_eq!(route("¿Cuánto cuesta una suscripción a Grok?", "", &es).kind, Kind::NotCovered(NotCovered::Prices));
        assert_eq!(route("últimas noticias", "", &es).kind, Kind::NotCovered(NotCovered::News));
    }

    #[test]
    fn programming_code_and_papers_have_their_own_sources() {
        assert_eq!(kind("TypeError: undefined is not a function in React component", ""), Kind::Programming);
        assert_eq!(kind("rust borrow checker error E0502", ""), Kind::Programming);
        assert_eq!(kind("How do I fix a merge conflict?", ""), Kind::Programming);
        assert_eq!(kind("python list comprehension with if else", ""), Kind::Programming);
        assert_eq!(kind("how to read a file in go", ""), Kind::Programming, "go beside a word that says so");
        assert_eq!(kind("c++ template specialization", ""), Kind::Programming);
        assert_eq!(kind("async streams in C#", ""), Kind::Programming);
        // An ordinary word that's also a language: not programming alone.
        assert_ne!(kind("How do I get rust off my bike?", "How do I get rust off my bike?"), Kind::Programming);
        assert_ne!(kind("Where do pythons live?", "Where do pythons live?"), Kind::Programming);
        assert_eq!(kind("a Rust crate for parsing PDFs", ""), Kind::Code);
        assert_eq!(kind("open-source alternative to Photoshop", ""), Kind::Code);
        assert_eq!(kind("github repo for whisper.cpp", ""), Kind::Code);
        assert_eq!(kind("recent papers on speech recognition", ""), Kind::Papers);
        assert_eq!(kind("arxiv diffusion models survey", ""), Kind::Papers);
        // A best-of in programming is still programming, with the browser offered.
        let r = route("best python web framework 2026", "", &en());
        assert_eq!(r, Route { kind: Kind::Programming, offer: Some(NotCovered::Reviews) });
        // "Toilet paper" isn't research.
        assert_ne!(kind("toilet paper", "toilet paper"), Kind::Papers);
    }

    #[test]
    fn places_games_and_facts() {
        assert_eq!(kind("Where is Glenbrackie Castle?", "Where is Glenbrackie Castle?"), Kind::Place);
        assert_eq!(kind("address of Kellbrook town hall", ""), Kind::Place);
        assert_eq!(route("Où se trouve Port Ellis ?", "", &["fr".to_string()]).kind, Kind::Place);
        // A game's question goes to its wiki, the model's query or not.
        assert_eq!(kind("Gale location", "Where do I find Gale in Baldur's Gate 3?"), Kind::Game);
        assert_eq!(kind("Where is Gale in BG3?", ""), Kind::Game);
        // A price in a game: its wiki first, the browser offered too.
        assert_eq!(
            route("Blood of Lathander cost", "How much does the Blood of Lathander cost in BG3?", &en()),
            Route { kind: Kind::Game, offer: Some(NotCovered::Prices) }
        );
        assert_eq!(kind("tallest mountain in Scotland", "What's the tallest mountain in Scotland?"), Kind::Facts);
        assert_eq!(kind("Ben Nevis height", ""), Kind::Facts);
        // The model's query says nothing: the question decides.
        assert_eq!(kind("Glenbrackie tomorrow", "What's the weather in Glenbrackie tomorrow?"), Kind::Weather);
        // The query says what this search is for, whatever else was asked.
        assert_eq!(kind("Grok price UK", "What's the weather, and how much is Grok?"), Kind::NotCovered(NotCovered::Prices));
    }
}
