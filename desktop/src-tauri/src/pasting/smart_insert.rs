//! **Match the text around the cursor** (`config.smart_spacing`): fit a
//! dictation to the spot it's pasted into, from the text Yap read beside the
//! caret at record start (`selection::begin_dictation`, UI Automation).
//!
//! Pure and unit-tested; `text_injector::prepare_dictation` applies it. With
//! no context (UI Automation couldn't read the field: some Electron apps,
//! terminals, games) nothing here runs and a dictation pastes exactly as it
//! always has, `append_trailing_space` included.
//!
//! The rules, conservative by design (a fix that's wrong is worse than the
//! capital letter that was there before):
//!
//! - **Mid-sentence** (the text before the caret ends in a letter, a digit, a
//!   comma, a semicolon or a dash — not after `.?!`, a colon, a bullet or a
//!   new line): the first letter goes lower case, but only for a word that's
//!   plainly sentence-case ("The", never "NASA", "iPhone", "I", "I'm"), isn't
//!   one of the dictionary's spellings, and is known to be an ordinary word —
//!   a common English (or French/German/Spanish/Italian/Portuguese/Dutch)
//!   function word, or one the surrounding text writes in lower case. A name
//!   ("Sarah", "Python") is never lowered: there's no telling it apart from a
//!   noun, so it keeps its capital, as before.
//! - **Text after the caret** that starts with a lower-case letter or
//!   punctuation (`,.;:!?)…`): a final full stop is dropped.
//! - **Spaces**: one before the dictation only when the text before ends in a
//!   letter, a digit or closing punctuation (never after a space, an opening
//!   bracket or quote, or at the start of the field); one after it only when
//!   the text after starts with a letter or a digit. Never between Chinese or
//!   Japanese characters (scripts written without spaces).
//! - **Replacing a selection**: the dictation follows the selection's first
//!   letter's case (with the same safety rules) and its end punctuation, and
//!   keeps any space the selection started or ended with.

/// The text around the caret in the field a dictation will be pasted into.
/// Lives only in memory for one dictation; never logged, never sent anywhere.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldContext {
    /// Up to [`CONTEXT_CHARS`] characters before the caret (or the selection).
    pub before: String,
    /// Up to [`CONTEXT_CHARS`] characters after the caret (or the selection).
    pub after: String,
    /// The selected text the paste will replace ("" = just a caret).
    pub selection: String,
}

/// How much text Yap reads on each side of the caret.
pub const CONTEXT_CHARS: usize = 500;

/// The last `max` characters of `s`, cut at a word boundary when it had to
/// be shortened (so a half word never leads the text).
pub fn tail_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let cut: String = s.chars().skip(n - max).collect();
    match cut.find(char::is_whitespace) {
        Some(i) => cut[i..].to_string(),
        None => cut,
    }
}

/// The first `max` characters of `s`, cut at a word boundary when it had to
/// be shortened.
pub fn head_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    match cut.rfind(char::is_whitespace) {
        Some(i) => cut[..i].to_string(),
        None => cut,
    }
}

/// Fit `text` (a finished dictation) to the spot described by `ctx`. `names`
/// are the correction dictionary's spellings (`to`): never lower-cased.
pub fn fit_to_context(text: &str, ctx: &FieldContext, names: &[&str]) -> String {
    let body = text.trim();
    if body.is_empty() {
        return String::new();
    }
    let selection = ctx.selection.as_str();
    let replacing = !selection.trim().is_empty();
    let mut body = body.to_string();

    // Casing of the first letter.
    if replacing {
        body = follow_selection_case(&body, selection.trim(), ctx, names);
    } else if mid_sentence(&ctx.before) && may_lower(&body, ctx, names) {
        body = lower_first(&body);
    }

    // End punctuation.
    if replacing {
        body = follow_selection_end(&body, selection.trim());
    }
    if continues_after(&ctx.after) {
        body = drop_full_stop(&body);
    }

    // Spaces.
    let lead_ws: String = selection.chars().take_while(|c| c.is_whitespace()).collect();
    let trail_ws: String = if selection.trim().is_empty() {
        String::new()
    } else {
        let rev: String = selection.chars().rev().take_while(|c| c.is_whitespace()).collect();
        rev.chars().rev().collect()
    };
    let lead = if replacing && !lead_ws.is_empty() {
        lead_ws
    } else if space_before(&ctx.before, &body) {
        " ".to_string()
    } else {
        String::new()
    };
    let trail = if replacing && !trail_ws.is_empty() {
        trail_ws
    } else if space_after(&body, &ctx.after) {
        " ".to_string()
    } else {
        String::new()
    };
    format!("{lead}{body}{trail}")
}

/// Chinese and Japanese (Han, kana, their punctuation and full-width forms):
/// written without spaces between words. (Hangul is not: Korean uses spaces.)
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F   // CJK symbols and punctuation
        | 0x3040..=0x30FF // hiragana, katakana
        | 0x31F0..=0x31FF // katakana phonetic extensions
        | 0x3400..=0x4DBF // CJK extension A
        | 0x4E00..=0x9FFF // CJK unified ideographs
        | 0xF900..=0xFAFF // CJK compatibility ideographs
        | 0xFF00..=0xFFEF // half-width and full-width forms
        | 0x20000..=0x3FFFF) // CJK extensions B+
}

/// Spaces and tabs (not new lines), incl. a no-break space.
fn is_inline_space(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\u{a0}' || c == '\u{202f}'
}

/// The caret sits inside a sentence: what's before it ends in a letter, a
/// digit, a comma, a semicolon or a dash, on a line that isn't just a list
/// marker. Field start, a new line, `.?!…`, a colon or a bracket/quote: no.
fn mid_sentence(before: &str) -> bool {
    let t = before.trim_end_matches(is_inline_space);
    let Some(last) = t.chars().last() else {
        return false;
    };
    if last == '\n' || last == '\r' {
        return false;
    }
    // A dash on a line of its own is a bullet ("– "), not a pause. (Other
    // list markers — "-", "*", "•", "2." — never count as mid-sentence.)
    let line = t.rsplit(['\n', '\r']).next().unwrap_or(t).trim();
    if matches!(line, "–" | "—") {
        return false;
    }
    if is_cjk(last) {
        return false;
    }
    last.is_alphanumeric() || matches!(last, ',' | ';' | '–' | '—')
}

/// The first word, without the punctuation that follows it ("So," → "So").
fn first_word(text: &str) -> &str {
    let w = text.split_whitespace().next().unwrap_or("");
    let end = w
        .char_indices()
        .find(|&(_, c)| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .map(|(i, _)| i)
        .unwrap_or(w.len());
    &w[..end]
}

#[derive(Debug, PartialEq, Eq)]
enum Case {
    /// "the", "don't"
    Lower,
    /// "The", "Don't", "A"
    Sentence,
    /// "NASA", "iPhone", "McDonald", "I", digits…
    Other,
}

fn case_of(word: &str) -> Case {
    let mut chars = word.chars().filter(|c| c.is_alphabetic());
    let Some(first) = chars.next() else {
        return Case::Other;
    };
    let rest_lower = chars.all(|c| c.is_lowercase());
    if !word.starts_with(first) || !rest_lower {
        return Case::Other;
    }
    if first.is_lowercase() {
        Case::Lower
    } else if first.is_uppercase() {
        Case::Sentence
    } else {
        Case::Other
    }
}

/// "I", "I'm", "I'll", "I'd", "I've" stay capitalised wherever they land.
fn is_i_word(word: &str) -> bool {
    word == "I" || word.starts_with("I'") || word.starts_with("I’")
}

/// Ordinary words that open a sentence's continuation and are never names,
/// lower-cased mid-sentence. Words that double as common first names ("Will",
/// "May", "Mark", "Bill", "June", "Grace", "Hope"), start place names ("El
/// Paso", "Las Vegas", "Los Angeles", "Le Mans") or are single letters with
/// other uses ("E", "Y") are deliberately absent.
const COMMON_WORDS: &[&str] = &[
    // English
    "a", "about", "actually", "after", "again", "all", "also", "although", "always", "am", "an",
    "and", "another", "any", "anyway", "are", "as", "at", "basically", "be", "because", "been",
    "before", "being", "besides", "both", "but", "by", "can", "could", "did", "do", "does",
    "don't", "during", "each", "either", "even", "every", "few", "for", "from", "had", "has",
    "have", "he", "her", "here", "his", "how", "however", "if", "in", "instead", "into", "is",
    "it", "it's", "its", "just", "less", "let's", "like", "maybe", "me", "meanwhile", "might",
    "more", "most", "much", "must", "my", "neither", "never", "no", "nor", "not", "now", "of",
    "often", "on", "once", "only", "or", "other", "otherwise", "our", "perhaps", "please",
    "plus", "probably", "quite", "rather", "really", "she", "should", "since", "so", "some",
    "sometimes", "still", "such", "than", "thank", "thanks", "that", "that's", "the", "their",
    "them", "then", "there", "there's", "therefore", "these", "they", "they're", "this",
    "those", "though", "through", "to", "today", "tomorrow", "too", "under", "unless", "until",
    "us", "usually", "very", "was", "we", "we'll", "we're", "were", "what", "what's", "when",
    "where", "whether", "which", "while", "who", "whose", "why", "with", "without", "would",
    "yes", "yesterday", "yet", "you", "you'll", "you're", "your",
    // French
    "alors", "c'est", "donc", "elle", "et", "il", "je", "mais", "nous", "ou", "parce", "puis",
    "que", "qui", "un", "une", "vous",
    // German (no "Sie": the polite "you" keeps its capital)
    "aber", "auch", "dann", "das", "dass", "der", "die", "ein", "eine", "ich", "ist", "nicht",
    "oder", "und", "weil", "wenn", "wir",
    // Spanish / Italian / Portuguese / Dutch
    "con", "cuando", "en", "entonces", "eu", "het", "io", "maar", "mas", "pero", "poi",
    "porque", "perché", "también", "una", "yo",
];

/// Whole words of `s` (letters, digits and apostrophes).
fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .filter(|w| !w.is_empty())
}

/// `word` appears capitalised in `s` somewhere other than a sentence start
/// (after a letter, digit or comma): someone's name, a product, a place.
fn capitalised_mid_sentence(s: &str, word: &str) -> bool {
    let mut search = 0;
    while let Some(i) = s[search..].find(word) {
        let at = search + i;
        let end = at + word.len();
        search = end;
        let whole_before = s[..at].chars().last().is_none_or(|c| !(c.is_alphanumeric() || c == '\''));
        let whole_after = s[end..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '\''));
        if whole_before && whole_after && mid_sentence(&s[..at]) {
            return true;
        }
    }
    false
}

/// May the dictation's sentence-case first word be lower-cased here?
fn may_lower(body: &str, ctx: &FieldContext, names: &[&str]) -> bool {
    let word = first_word(body);
    if case_of(word) != Case::Sentence || is_i_word(word) {
        return false;
    }
    // One of the dictionary's spellings ("Yap", "Parakeet", "Imogen"), or the
    // first word of one ("Yap Notes").
    if names.iter().any(|n| n.split_whitespace().next() == Some(word)) {
        return false;
    }
    let rest = &body[body.find(word).map(|i| i + word.len()).unwrap_or(0)..];
    // Written with its capital mid-sentence nearby: a name.
    if [ctx.before.as_str(), ctx.after.as_str(), rest]
        .iter()
        .any(|s| capitalised_mid_sentence(s, word))
    {
        return false;
    }
    let lower = word.to_lowercase();
    COMMON_WORDS.contains(&lower.replace('’', "'").as_str())
        || [ctx.before.as_str(), ctx.after.as_str(), rest]
            .iter()
            .any(|s| words(s).any(|w| w == lower))
}

fn lower_first(body: &str) -> String {
    let mut chars = body.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn upper_first(body: &str) -> String {
    let mut chars = body.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Replacing a selection: start the way it started. A lower-case selection
/// lowers the dictation's first letter (with [`may_lower`]'s safety rules, or
/// when it's the same word again); a sentence-case one capitalises it.
fn follow_selection_case(body: &str, selection: &str, ctx: &FieldContext, names: &[&str]) -> String {
    let sel_word = first_word(selection);
    let word = first_word(body);
    match (case_of(sel_word), case_of(word)) {
        (Case::Lower, Case::Sentence)
            if !is_i_word(word) && (sel_word == word.to_lowercase() || may_lower(body, ctx, names)) =>
        {
            lower_first(body)
        }
        (Case::Sentence, Case::Lower) => upper_first(body),
        _ => body.to_string(),
    }
}

/// Replacing a selection: end the way it ended. No final punctuation → no
/// full stop; a final `.?!` → the dictation gets it if it has none.
fn follow_selection_end(body: &str, selection: &str) -> String {
    let Some(sel_end) = selection.chars().last() else {
        return body.to_string();
    };
    let Some(body_end) = body.chars().last() else {
        return body.to_string();
    };
    if matches!(sel_end, '.' | '?' | '!') {
        if body_end.is_alphanumeric() {
            return format!("{body}{sel_end}");
        }
        body.to_string()
    } else if sel_end.is_alphanumeric() {
        drop_full_stop(body)
    } else {
        body.to_string()
    }
}

/// The sentence carries on after the caret: what follows starts with a
/// lower-case letter or punctuation that belongs to it.
fn continues_after(after: &str) -> bool {
    let t = after.trim_start_matches(is_inline_space);
    match t.chars().next() {
        Some(c) => {
            (c.is_lowercase() && !is_cjk(c))
                || matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '…')
        }
        None => false,
    }
}

/// One final full stop off (not an ellipsis).
fn drop_full_stop(body: &str) -> String {
    if body.ends_with('.') && !body.ends_with("..") {
        body[..body.len() - 1].to_string()
    } else {
        body.to_string()
    }
}

/// Punctuation that closes something, so a word may follow after a space.
fn closes(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '…' | '%' | '”' | '’' | '»')
}

/// A space between what's before the caret and the dictation?
fn space_before(before: &str, body: &str) -> bool {
    let (Some(last), Some(first)) = (before.chars().last(), body.chars().next()) else {
        return false; // the start of the field
    };
    if last.is_whitespace() || is_cjk(last) || is_cjk(first) {
        return false;
    }
    // The dictation itself starts by closing something (",", ")").
    if matches!(first, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '…' | '%') {
        return false;
    }
    if last.is_alphanumeric() || closes(last) {
        return true;
    }
    let mut rev = before.chars().rev();
    rev.next();
    let prev = rev.next();
    match last {
        // A straight quote closes when it follows a word, opens after a space.
        '"' | '\'' => prev.is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '{')),
        // A spaced dash ("worry — it's") takes a space after it too.
        '–' | '—' => prev.is_some_and(char::is_whitespace),
        _ => false,
    }
}

/// A space between the dictation and what's after the caret?
fn space_after(body: &str, after: &str) -> bool {
    let (Some(last), Some(first)) = (body.chars().last(), after.chars().next()) else {
        return false;
    };
    if is_cjk(last) || is_cjk(first) || last.is_whitespace() {
        return false;
    }
    first.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(before: &str, after: &str) -> FieldContext {
        FieldContext {
            before: before.into(),
            after: after.into(),
            selection: String::new(),
        }
    }

    fn sel(before: &str, selection: &str, after: &str) -> FieldContext {
        FieldContext {
            before: before.into(),
            after: after.into(),
            selection: selection.into(),
        }
    }

    fn fit(text: &str, c: &FieldContext) -> String {
        fit_to_context(text, c, &[])
    }

    #[test]
    fn an_empty_field_takes_the_dictation_as_it_is() {
        assert_eq!(fit("Hello there.", &ctx("", "")), "Hello there.");
        assert_eq!(fit("  Hello there.  ", &ctx("", "")), "Hello there.");
        assert_eq!(fit("   ", &ctx("Hi", "")), "");
    }

    #[test]
    fn after_a_full_sentence_it_adds_a_space_and_keeps_the_capital() {
        assert_eq!(fit("The rest is fine.", &ctx("I checked it.", "")), " The rest is fine.");
        assert_eq!(fit("The rest is fine.", &ctx("I checked it. ", "")), "The rest is fine.");
        assert_eq!(fit("And then?", &ctx("Really?!", "")), " And then?");
        assert_eq!(fit("And so on.", &ctx("He said \"stop.\"", "")), " And so on.");
    }

    #[test]
    fn mid_sentence_lowers_an_ordinary_first_word() {
        assert_eq!(fit("And then we left.", &ctx("We had dinner ", "")), "and then we left.");
        assert_eq!(fit("Because it was late.", &ctx("We went home", "")), " because it was late.");
        assert_eq!(fit("Which is fine.", &ctx("It costs 30,", "")), " which is fine.");
        assert_eq!(fit("The blue one.", &ctx("I want 2 ", "")), "the blue one.");
        assert_eq!(fit("So, yes.", &ctx("Well; ", "")), "so, yes.");
        assert_eq!(fit("It's done.", &ctx("Don't worry —", "")), " it's done.");
    }

    #[test]
    fn mid_sentence_keeps_names_acronyms_and_i() {
        // Can't tell a name from a noun: names keep their capital.
        assert_eq!(fit("Sarah said yes.", &ctx("I asked ", "")), "Sarah said yes.");
        assert_eq!(fit("Python is great.", &ctx("I think ", "")), "Python is great.");
        assert_eq!(fit("NASA called.", &ctx("Guess what ", "")), "NASA called.");
        assert_eq!(fit("I think so.", &ctx("Well ", "")), "I think so.");
        assert_eq!(fit("I'm sure.", &ctx("Well ", "")), "I'm sure.");
        assert_eq!(fit("I’ll do it.", &ctx("Fine, ", "")), "I’ll do it.");
        assert_eq!(fit("McDonald's again.", &ctx("Lunch at ", "")), "McDonald's again.");
        // Words that are often names stay too.
        assert_eq!(fit("Will can come.", &ctx("Ask whether ", "")), "Will can come.");
    }

    #[test]
    fn dictionary_spellings_are_never_lowered() {
        let c = ctx("I opened ", "");
        assert_eq!(fit_to_context("The app.", &c, &["The"]), "The app.");
        assert_eq!(fit_to_context("So Notes.", &c, &["So Notes"]), "So Notes.");
        assert_eq!(fit_to_context("So it goes.", &c, &["Yap"]), "so it goes.");
    }

    #[test]
    fn the_text_around_tells_names_from_words() {
        // "Bananas" isn't a function word, but the field writes it in lower case.
        assert_eq!(fit("Bananas too.", &ctx("I bought apples and bananas, ", "")), "bananas too.");
        // A capital mid-sentence elsewhere means a name, even for a common word.
        assert_eq!(fit("So it is.", &ctx("We met So yesterday and ", "")), "So it is.");
        // The same word lower case later in the dictation itself.
        assert_eq!(fit("Apples and more apples.", &ctx("Buy ", "")), "apples and more apples.");
    }

    #[test]
    fn a_new_line_a_bullet_or_a_colon_starts_afresh() {
        assert_eq!(fit("The plan.", &ctx("Notes\n", "")), "The plan.");
        assert_eq!(fit("The plan.", &ctx("Notes\r\n  ", "")), "The plan.");
        assert_eq!(fit("The plan.", &ctx("Todo:\n- ", "")), "The plan.");
        assert_eq!(fit("The plan.", &ctx("Todo:\n2. ", "")), "The plan.");
        assert_eq!(fit("The plan.", &ctx("Todo:\n• ", "")), "The plan.");
        assert_eq!(fit("The plan.", &ctx("Todo:\n— ", "")), "The plan.");
        assert_eq!(fit("The plan.", &ctx("Note: ", "")), "The plan.");
        assert_eq!(fit("The plan.", &ctx("(", "")), "The plan.");
    }

    #[test]
    fn a_lower_case_continuation_drops_the_full_stop() {
        assert_eq!(fit("The blue one.", &ctx("I want ", " please")), "the blue one");
        assert_eq!(fit("The blue one.", &ctx("I want ", "please")), "the blue one ");
        assert_eq!(fit("A dog.", &ctx("I saw ", ", then left.")), "a dog");
        assert_eq!(fit("A dog.", &ctx("I saw ", ".")), "a dog");
        // A capital after: a new sentence follows, the full stop stays.
        assert_eq!(fit("That's all.", &ctx("", " Then we left.")), "That's all.");
        // Never touches ? ! or an ellipsis.
        assert_eq!(fit("Really?", &ctx("", " right")), "Really?");
        assert_eq!(fit("Wait...", &ctx("", "and")), "Wait... ");
    }

    #[test]
    fn spaces_follow_the_neighbours() {
        // Before: a letter, digit or closing punctuation → space.
        assert_eq!(fit("Done.", &ctx("Step 3.", "")), " Done.");
        assert_eq!(fit("Then go.", &ctx("(see above)", "")), " Then go.");
        assert_eq!(fit("Fine.", &ctx("50%", "")), " Fine.");
        // Opening brackets/quotes, symbols, whitespace: no space.
        assert_eq!(fit("Hello.", &ctx("He said \"", "")), "Hello.");
        assert_eq!(fit("Hello.", &ctx("She said '", "")), "Hello.");
        assert_eq!(fit("Hello.", &ctx("[", "")), "Hello.");
        assert_eq!(fit("example.com", &ctx("mail me at name@", "")), "example.com");
        assert_eq!(fit("Hello.", &ctx("Tabbed\t", "")), "Hello.");
        // A closing straight quote after a word.
        assert_eq!(fit("Then he left.", &ctx("He said \"no\"", "")), " Then he left.");
        // After: a letter or digit → space; punctuation or a space → none.
        assert_eq!(fit("Very", &ctx("A ", "big dog")), "very ");
        assert_eq!(fit("Big", &ctx("A ", "dog")), "Big ");
        assert_eq!(fit("Big!", &ctx("", " dog")), "Big!");
        assert_eq!(fit("Hi", &ctx("", ", there")), "Hi");
        assert_eq!(fit("Hi", &ctx("", "\nNext")), "Hi");
        assert_eq!(fit("Hi", &ctx("", "42")), "Hi ");
    }

    #[test]
    fn chinese_and_japanese_get_no_spaces() {
        assert_eq!(fit("你好。", &ctx("我说", "")), "你好。");
        assert_eq!(fit("こんにちは", &ctx("Hello", "")), "こんにちは");
        assert_eq!(fit("Hello", &ctx("我说", "世界")), "Hello");
        // Korean writes spaces between words.
        assert_eq!(fit("안녕하세요", &ctx("말했다", "")), " 안녕하세요");
    }

    #[test]
    fn replacing_a_selection_follows_its_case_and_end() {
        // Lower-case selection mid-sentence: lower the dictation's first word.
        assert_eq!(fit("The new plan.", &sel("I like ", "the old plan", " a lot")), "the new plan");
        // Sentence-case selection: capitalise.
        assert_eq!(fit("we should wait", &sel("", "We should ship.", " Then")), "We should wait.");
        // The selection ended with a question mark.
        assert_eq!(fit("can you make it", &sel("", "Can you come?", "")), "Can you make it?");
        // The same word again follows the selection exactly.
        assert_eq!(fit("Apples.", &sel("I like ", "apples", ".")), "apples");
        // A name stays a name.
        assert_eq!(fit("Paris.", &sel("I love ", "london", ".")), "Paris");
        // I stays I.
        assert_eq!(fit("I did.", &sel("Then ", "we did", ".")), "I did");
        // Spaces the selection carried come back.
        assert_eq!(fit("These.", &sel("I like", " apples ", "and more")), " these ");
        // No selection spaces: the neighbours decide.
        assert_eq!(fit("These.", &sel("I like", "apples", "and more")), " these ");
    }

    #[test]
    fn context_is_cut_at_word_boundaries() {
        assert_eq!(tail_chars("one two three", 50), "one two three");
        assert_eq!(tail_chars("one two three", 9), " three");
        assert_eq!(tail_chars("abcdef", 3), "def");
        assert_eq!(head_chars("one two three", 9), "one two");
        assert_eq!(head_chars("abcdef", 3), "abc");
        assert_eq!(head_chars("héllo wörld", 8), "héllo");
        assert_eq!(tail_chars("héllo wörld", 8), " wörld");
    }

    #[test]
    fn case_classes() {
        assert_eq!(case_of("the"), Case::Lower);
        assert_eq!(case_of("The"), Case::Sentence);
        assert_eq!(case_of("A"), Case::Sentence);
        assert_eq!(case_of("Don't"), Case::Sentence);
        assert_eq!(case_of("NASA"), Case::Other);
        assert_eq!(case_of("iPhone"), Case::Other);
        assert_eq!(case_of("42"), Case::Other);
        assert_eq!(first_word("So, then"), "So");
        assert_eq!(first_word("\"Quoted\" word"), "");
        assert_eq!(first_word("It's late"), "It's");
    }
}
