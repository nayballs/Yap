//! **The words Yap's page rules know, one table per language** (pure):
//! English, French, German, Spanish, Italian, Portuguese and Dutch — the
//! languages Yap speaks. Every table applies to every page, whatever the
//! person's own languages: a German shop asks before "Jetzt kaufen" for
//! someone who only speaks English too.
//!
//! The words are only one of the rules' signals (`rules.rs`): a field's
//! type and `autocomplete`, a form's payment fields, a price on a button and
//! the typed text itself (a card number, an IBAN) don't depend on any
//! language.
//!
//! **How an entry matches** a label's words (`rules::words`: lower-cased,
//! accents folded — "Réserver" is "reserver", "Bestätigen" "bestatigen",
//! "ß" "ss"):
//! - `"kaufen"`: one of the words;
//! - `"sign in"`: those words side by side, or run together ("signin");
//! - `"*kartennummer"`: a word ending so — German and Dutch run words
//!   together ("Kreditkartennummer", "Bankwachtwoord").
//!
//! Entries are written folded (no accents) and lower-case.

/// One language's words, by what they'd do.
pub struct Lang {
    /// Its language (the tests list the tables by it).
    #[allow(dead_code)]
    pub code: &'static str,
    // ---- a click that asks the person first (rules::needs_ok) ----
    pub sign_in: &'static [&'static str],
    pub buy: &'static [&'static str],
    pub pay: &'static [&'static str],
    pub book: &'static [&'static str],
    pub order: &'static [&'static str],
    pub confirm: &'static [&'static str],
    pub subscribe: &'static [&'static str],
    /// Cancelling something paid for ("Cancel membership"): a phrase, never
    /// "Cancel" alone (a dialog's).
    pub cancel: &'static [&'static str],
    pub send: &'static [&'static str],
    pub delete: &'static [&'static str],
    /// Agreeing to terms ("I agree"): never a cookie banner's "Accept all".
    pub agree: &'static [&'static str],
    /// A trash or bin to drop something into (`rules::drag_needs_ok`: a drag
    /// there deletes).
    pub trash: &'static [&'static str],
    // ---- fields Yap never types into (rules::sensitive) ----
    pub password: &'static [&'static str],
    pub card: &'static [&'static str],
    pub bank: &'static [&'static str],
    pub code_field: &'static [&'static str],
    // ---- sign-in forms (rules::sign_in_field) ----
    /// A user name or email field's words.
    pub user: &'static [&'static str],
    /// A page that says it's for signing in (its title, a heading).
    pub sign_in_page: &'static [&'static str],
    // ---- a search box (Enter there needs no asking) ----
    pub search: &'static [&'static str],
}

pub const EN: Lang = Lang {
    code: "en",
    sign_in: &["sign in", "log in", "log on", "login", "logon", "signin"],
    buy: &["buy", "purchase", "checkout", "check out", "basket", "cart", "bag"],
    pay: &["pay", "payment", "paynow", "donate", "transfer", "withdraw", "deposit", "bid"],
    book: &["book", "booking", "reserve", "reservation"],
    order: &["order", "preorder"],
    confirm: &["confirm", "confirmation"],
    subscribe: &[
        "subscribe", "unsubscribe", "subscription", "register", "signup", "sign up", "enrol", "enroll", "free trial", "start trial", "upgrade",
        "renew",
    ],
    cancel: &[
        "cancel membership", "cancel subscription", "cancel order", "cancel booking", "cancel reservation", "cancel account", "cancel plan",
        "cancel my", "cancel your",
    ],
    send: &["send", "submit", "post", "publish", "reply", "share", "tweet"],
    delete: &["delete", "remove", "erase", "discard", "close account", "close my account", "deactivate"],
    agree: &["i agree", "agree", "accept terms", "accept the terms", "i accept"],
    trash: &["trash", "bin", "recycle bin", "rubbish", "wastebasket", "deleted items"],
    password: &["password", "passwd", "passcode", "passphrase"],
    card: &["card", "cardnumber", "ccnumber", "ccnum", "creditcard", "debitcard", "cvv", "cvv2", "cvc", "cvc2", "csc", "cardholder", "card number", "expiry date", "expiration date"],
    bank: &["iban", "bic", "swift", "routing", "sort code", "sortcode", "account number", "account no"],
    code_field: &["pin", "otp", "2fa", "totp", "security code", "verification code"],
    user: &["username", "user", "login", "email", "userid", "user name", "e mail"],
    sign_in_page: &["sign in", "log in", "login", "logon"],
    search: &["search", "find"],
};

pub const FR: Lang = Lang {
    code: "fr",
    sign_in: &["se connecter", "connexion", "connecter", "identifier", "s identifier"],
    buy: &["acheter", "panier", "achat", "acheter maintenant"],
    pay: &["payer", "paiement", "faire un don", "virement"],
    book: &["reserver", "reservation"],
    order: &["commander", "commande"],
    confirm: &["confirmer", "valider", "confirmation"],
    subscribe: &["abonner", "s abonner", "abonnement", "desabonner", "inscrire", "s inscrire", "inscription", "essai gratuit"],
    cancel: &["resilier", "annuler l abonnement", "annuler mon abonnement", "annuler la commande", "annuler la reservation"],
    send: &["envoyer", "publier", "repondre", "partager", "soumettre"],
    delete: &["supprimer", "effacer", "fermer mon compte", "fermer le compte", "cloturer"],
    agree: &["j accepte", "accepter les conditions", "je suis d accord"],
    trash: &["corbeille", "poubelle"],
    password: &["mot de passe", "motdepasse", "mdp"],
    card: &["carte", "numero de carte", "carte bancaire", "titulaire", "cryptogramme", "date d expiration"],
    bank: &["rib", "numero de compte", "code banque"],
    code_field: &["code de securite", "code de verification", "code secret", "code pin"],
    user: &["identifiant", "utilisateur", "nom d utilisateur", "courriel", "adresse e mail"],
    sign_in_page: &["se connecter", "connexion", "identifiez vous"],
    search: &["rechercher", "recherche", "chercher"],
};

pub const DE: Lang = Lang {
    code: "de",
    sign_in: &["anmelden", "einloggen", "login", "anmeldung", "einlogen"],
    buy: &["kaufen", "jetzt kaufen", "kauf", "warenkorb", "*warenkorb", "kasse", "zur kasse"],
    pay: &["bezahlen", "jetzt zahlen", "zahlung", "*zahlung", "zahlungspflichtig", "spenden", "uberweisen", "gebot abgeben"],
    book: &["buchen", "buchung", "reservieren", "reservierung", "jetzt buchen"],
    order: &["bestellen", "vorbestellen", "bestellung", "*bestellung"],
    confirm: &["bestatigen", "bestatigung"],
    subscribe: &["abonnieren", "abonnement", "abo", "abbestellen", "registrieren", "registrierung", "probeabo", "kostenlos testen"],
    cancel: &["kundigen", "*kundigen", "kundigung", "abo beenden", "mitgliedschaft beenden"],
    send: &["senden", "absenden", "abschicken", "schicken", "veroffentlichen", "posten", "antworten", "teilen"],
    delete: &["loschen", "entfernen", "konto schliessen", "konto loschen"],
    agree: &["ich stimme zu", "zustimmen", "einverstanden", "agb akzeptieren"],
    trash: &["papierkorb", "*papierkorb", "mulleimer"],
    password: &["passwort", "*passwort", "kennwort", "*kennwort"],
    card: &["kartennummer", "*kartennummer", "kreditkarte", "*kreditkarte", "karteninhaber", "prufnummer", "*prufnummer", "prufziffer", "ablaufdatum", "gultig bis"],
    bank: &["kontonummer", "*kontonummer", "bankleitzahl", "blz", "bankverbindung"],
    code_field: &["sicherheitscode", "*sicherheitscode", "bestatigungscode", "tan", "pin", "pincode"],
    user: &["benutzername", "benutzer", "nutzername", "e mail adresse"],
    sign_in_page: &["anmelden", "anmeldung", "einloggen"],
    search: &["suche", "suchen"],
};

pub const ES: Lang = Lang {
    code: "es",
    sign_in: &["iniciar sesion", "acceder", "entrar", "identificarse"],
    buy: &["comprar", "compra", "carrito", "cesta", "comprar ahora"],
    pay: &["pagar", "pago", "donar", "transferir"],
    book: &["reservar", "reserva"],
    order: &["pedido", "realizar pedido", "tramitar pedido", "pedir", "encargar"],
    confirm: &["confirmar", "confirmacion"],
    subscribe: &["suscribirse", "suscribir", "suscripcion", "registrarse", "registrar", "darse de alta", "prueba gratuita"],
    cancel: &["darse de baja", "dar de baja", "darme de baja", "cancelar suscripcion", "cancelar la suscripcion", "cancelar pedido", "cancelar reserva"],
    send: &["enviar", "publicar", "responder", "compartir"],
    delete: &["eliminar", "borrar", "cerrar cuenta", "cerrar mi cuenta"],
    agree: &["acepto", "aceptar las condiciones", "estoy de acuerdo"],
    trash: &["papelera", "basura"],
    password: &["contrasena", "clave de acceso"],
    card: &["tarjeta", "numero de tarjeta", "titular de la tarjeta", "caducidad", "vencimiento"],
    bank: &["numero de cuenta", "cuenta bancaria"],
    code_field: &["codigo de seguridad", "codigo de verificacion"],
    user: &["usuario", "nombre de usuario", "correo", "correo electronico"],
    sign_in_page: &["iniciar sesion", "acceder", "inicia sesion"],
    search: &["buscar", "busqueda"],
};

pub const IT: Lang = Lang {
    code: "it",
    sign_in: &["accedi", "accesso", "entra"],
    buy: &["acquista", "compra", "carrello", "cassa", "acquista ora"],
    pay: &["paga", "pagamento", "dona", "bonifico"],
    book: &["prenota", "prenotazione"],
    order: &["ordina", "ordine"],
    confirm: &["conferma", "confermare"],
    subscribe: &["iscriviti", "abbonati", "abbonamento", "registrati", "registrazione", "prova gratuita", "disiscriviti"],
    cancel: &["disdici", "disdetta", "annulla abbonamento", "annulla ordine", "annulla prenotazione"],
    send: &["invia", "pubblica", "rispondi", "condividi"],
    delete: &["elimina", "cancella", "rimuovi", "chiudi account", "chiudi il conto"],
    agree: &["accetto", "accetta i termini", "sono d accordo"],
    trash: &["cestino"],
    password: &["password", "parola d ordine"],
    card: &["carta", "numero della carta", "carta di credito", "titolare", "scadenza"],
    bank: &["numero di conto", "conto corrente"],
    code_field: &["codice di sicurezza", "codice di verifica"],
    user: &["utente", "nome utente"],
    sign_in_page: &["accedi", "accesso"],
    search: &["cerca", "cercare", "ricerca"],
};

pub const PT: Lang = Lang {
    code: "pt",
    sign_in: &["entrar", "iniciar sessao", "fazer login", "acessar"],
    buy: &["comprar", "compra", "carrinho", "sacola", "finalizar compra", "cesto"],
    pay: &["pagar", "pagamento", "doar", "transferir"],
    book: &["reservar", "reserva"],
    order: &["encomendar", "encomenda", "pedido", "fazer pedido"],
    confirm: &["confirmar", "confirmacao"],
    subscribe: &["assinar", "assinatura", "inscrever", "inscreva", "inscrever se", "cadastrar", "cadastre", "registar", "registrar", "teste gratis"],
    cancel: &["cancelar assinatura", "cancelar a assinatura", "cancelar pedido", "cancelar reserva", "cancelar subscricao"],
    send: &["enviar", "publicar", "responder", "compartilhar", "partilhar"],
    delete: &["excluir", "apagar", "eliminar", "remover", "encerrar conta"],
    agree: &["aceito", "concordo", "aceitar os termos"],
    trash: &["lixeira", "lixo"],
    password: &["senha", "palavra passe"],
    card: &["cartao", "numero do cartao", "titular do cartao", "validade"],
    bank: &["numero da conta", "conta bancaria", "nib"],
    code_field: &["codigo de seguranca", "codigo de verificacao"],
    user: &["usuario", "utilizador", "nome de usuario"],
    sign_in_page: &["entrar", "iniciar sessao", "fazer login"],
    search: &["pesquisar", "pesquisa", "procurar", "buscar"],
};

pub const NL: Lang = Lang {
    code: "nl",
    sign_in: &["inloggen", "aanmelden", "log in"],
    buy: &["kopen", "nu kopen", "winkelwagen", "*winkelwagen", "winkelmand", "*winkelmand", "afrekenen"],
    pay: &["betalen", "betaling", "doneren", "overmaken"],
    book: &["boeken", "reserveren", "reservering", "boeking"],
    order: &["bestellen", "bestelling", "*bestelling"],
    confirm: &["bevestigen", "bevestiging"],
    subscribe: &["abonneren", "abonnement", "registreren", "inschrijven", "uitschrijven", "proefabonnement"],
    cancel: &["opzeggen", "*opzeggen", "abonnement beeindigen"],
    send: &["verzenden", "versturen", "verstuur", "plaatsen", "publiceren", "beantwoorden", "delen"],
    delete: &["verwijderen", "wissen", "account sluiten", "account opheffen"],
    agree: &["ik ga akkoord", "akkoord", "ga akkoord"],
    trash: &["prullenbak", "*prullenbak"],
    password: &["wachtwoord", "*wachtwoord"],
    card: &["kaartnummer", "*kaartnummer", "creditcard", "pasnummer", "kaarthouder", "vervaldatum"],
    bank: &["rekeningnummer", "*rekeningnummer"],
    code_field: &["beveiligingscode", "verificatiecode", "pincode"],
    user: &["gebruikersnaam", "gebruiker"],
    sign_in_page: &["inloggen", "aanmelden"],
    search: &["zoeken", "zoek"],
};

/// Every language's table.
pub const LANGS: &[&Lang] = &[&EN, &FR, &DE, &ES, &IT, &PT, &NL];

/// `c` without its accent (lower-case Latin letters; anything else as it
/// is). Pure.
fn fold_char(c: char, out: &mut String) {
    let base = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ß' => {
            out.push_str("ss");
            return;
        }
        'œ' => {
            out.push_str("oe");
            return;
        }
        'æ' => {
            out.push_str("ae");
            return;
        }
        other => other,
    };
    out.push(base);
}

/// A lower-case word with its accents folded ("réserver" → "reserver",
/// "straße" → "strasse"). Pure.
pub fn fold(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        fold_char(c, &mut out);
    }
    out
}

/// Whether the words `ws` (`rules::words`) hold `entry` (see the module's
/// note: a word, words side by side or run together, `*` a word's ending).
/// Pure.
pub fn holds(ws: &[String], entry: &str) -> bool {
    if let Some(end) = entry.strip_prefix('*') {
        return ws.iter().any(|w| w.len() > end.len() && w.ends_with(end));
    }
    if entry.contains(' ') {
        let parts: Vec<&str> = entry.split(' ').collect();
        let joined: String = parts.concat();
        return ws.contains(&joined) || ws.windows(parts.len()).any(|win| win.iter().zip(&parts).all(|(w, p)| w == p));
    }
    ws.iter().any(|w| w == entry)
}

/// Whether `ws` hold any of `list`'s entries. Pure.
pub fn any(ws: &[String], list: &[&str]) -> bool {
    list.iter().any(|e| holds(ws, e))
}

/// One of a table's lists (`|l| l.buy`).
pub type Pick = fn(&Lang) -> &'static [&'static str];

/// Whether `ws` hold any entry of `pick`'s list in any language. Pure.
pub fn in_any(ws: &[String], pick: Pick) -> bool {
    LANGS.iter().any(|l| any(ws, pick(l)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(s: &str) -> Vec<String> {
        crate::chat::pages::act::rules::words(s)
    }

    #[test]
    fn entries_match_words_phrases_and_compound_endings() {
        assert!(holds(&ws("Jetzt kaufen"), "kaufen"));
        assert!(holds(&ws("Sign in"), "sign in") && holds(&ws("SignIn"), "sign in") && holds(&ws("signin"), "sign in"));
        assert!(holds(&ws("Kreditkartennummer"), "*kartennummer") && !holds(&ws("Kartennummer"), "*kartennummer"));
        assert!(holds(&ws("Réserver"), "reserver") && holds(&ws("BESTÄTIGEN"), "bestatigen") && holds(&ws("Konto schließen"), "konto schliessen"));
        assert!(!holds(&ws("Cancel"), "cancel membership"));
        assert_eq!(fold("contraseña"), "contrasena");
    }

    #[test]
    fn every_table_is_written_folded_and_lower_case() {
        for l in LANGS {
            let lists = [
                l.sign_in, l.buy, l.pay, l.book, l.order, l.confirm, l.subscribe, l.cancel, l.send, l.delete, l.agree, l.trash, l.password, l.card, l.bank, l.code_field, l.user,
                l.sign_in_page, l.search,
            ];
            for list in lists {
                for e in list {
                    let bare = e.trim_start_matches('*');
                    assert_eq!(bare, fold(&bare.to_lowercase()), "{}: {e}", l.code);
                    assert!(bare.split(' ').all(|w| !w.is_empty() && w.chars().all(|c| c.is_alphanumeric())), "{}: {e}", l.code);
                }
            }
        }
        assert_eq!(LANGS.iter().map(|l| l.code).collect::<Vec<_>>(), ["en", "fr", "de", "es", "it", "pt", "nl"]);
    }
}
