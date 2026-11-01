//! The ID token's checks (OpenAI's "Registration and sign-in", step 4):
//! its RS256 signature against the issuer's published keys (`jwks_uri`),
//! then the issuer, the audience (the issued client id), the expiry, the
//! nonce of this attempt and the subject — the account a later sign-in or
//! refresh must match. `ring` verifies (already in the tree through rustls).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde_json::Value;

/// Who signed in, from a checked ID token.
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    /// The account (`sub`): never shown, compared on every sign-in.
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Why a token wasn't accepted.
#[derive(Debug, PartialEq)]
pub enum Bad {
    /// No key in the set matches its `kid`: the set may be stale (fetch it
    /// again once).
    NoKey,
    /// Anything else: not a token Yap accepts.
    Invalid(&'static str),
}

/// Clock skew allowed on `exp` (seconds), as OpenAI's DevKit allows.
const SKEW: i64 = 5;

/// A JWT's header, payload, the signed input and the signature.
fn parts(token: &str) -> Result<(Value, Value, &str, Vec<u8>), Bad> {
    let (signed, sig) = token.rsplit_once('.').ok_or(Bad::Invalid("not a JWT"))?;
    let (header, payload) = signed.split_once('.').ok_or(Bad::Invalid("not a JWT"))?;
    let json = |s: &str| -> Result<Value, Bad> {
        let raw = URL_SAFE_NO_PAD.decode(s).map_err(|_| Bad::Invalid("not base64url"))?;
        serde_json::from_slice(&raw).map_err(|_| Bad::Invalid("not JSON"))
    };
    let sig = URL_SAFE_NO_PAD.decode(sig).map_err(|_| Bad::Invalid("not base64url"))?;
    Ok((json(header)?, json(payload)?, signed, sig))
}

/// **Check `token`** against the key set `jwks` (`{"keys": [...]}`), for
/// `client_id` (its audience), `nonce` when this attempt sent one, at
/// `now` (Unix seconds). Pure.
pub fn verify(token: &str, jwks: &Value, issuer: &str, client_id: &str, nonce: Option<&str>, now: i64) -> Result<Identity, Bad> {
    let (header, claims, signed, sig) = parts(token)?;
    if header["alg"] != "RS256" {
        return Err(Bad::Invalid("not RS256"));
    }
    let kid = header["kid"].as_str();
    let key = jwks["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|k| k["kty"] == "RSA" && k["use"].as_str().is_none_or(|u| u == "sig") && (kid.is_none() || k["kid"].as_str() == kid))
        .ok_or(Bad::NoKey)?;
    let b64 = |s: &Value| s.as_str().and_then(|s| URL_SAFE_NO_PAD.decode(s).ok()).ok_or(Bad::Invalid("bad key"));
    let (n, e) = (b64(&key["n"])?, b64(&key["e"])?);
    ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(&ring::signature::RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &sig)
        .map_err(|_| Bad::Invalid("signature"))?;
    check_claims(&claims, issuer, client_id, nonce, now)
}

/// The claims of a token whose signature checked out. Pure.
fn check_claims(c: &Value, issuer: &str, client_id: &str, nonce: Option<&str>, now: i64) -> Result<Identity, Bad> {
    if c["iss"] != issuer {
        return Err(Bad::Invalid("issuer"));
    }
    // `aud`: the client id, or a list holding it (with `azp` saying so when
    // it names others too).
    let aud_ok = match &c["aud"] {
        Value::String(a) => a == client_id,
        Value::Array(list) => list.iter().any(|a| a == client_id) && (list.len() == 1 || c["azp"] == client_id),
        _ => false,
    };
    if !aud_ok || c.get("azp").is_some_and(|a| a != client_id) {
        return Err(Bad::Invalid("audience"));
    }
    let exp = c["exp"].as_i64().ok_or(Bad::Invalid("expiry"))?;
    if exp + SKEW < now || c["iat"].as_i64().is_none() {
        return Err(Bad::Invalid("expiry"));
    }
    if let Some(n) = nonce {
        if c["nonce"].as_str() != Some(n) {
            return Err(Bad::Invalid("nonce"));
        }
    }
    let sub = c["sub"].as_str().filter(|s| !s.is_empty()).ok_or(Bad::Invalid("subject"))?;
    let text = |k: &str| c[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(Identity { sub: sub.to_string(), email: text("email"), name: text("name") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A throwaway 2048-bit RSA key made for these tests (PKCS#1, DER):
    /// it signs nothing anywhere else.
    const TEST_KEY: &str = include_str!("test_key.b64");
    const ISS: &str = "https://auth.openai.com";
    const CLIENT: &str = "oaiapp_example";

    fn key() -> ring::rsa::KeyPair {
        let der = base64::engine::general_purpose::STANDARD.decode(TEST_KEY.trim()).unwrap();
        ring::rsa::KeyPair::from_der(&der).unwrap()
    }

    fn jwks(kid: &str) -> Value {
        let k = key();
        let parts: ring::rsa::PublicKeyComponents<Vec<u8>> = k.public().into();
        let enc = |b: &[u8]| URL_SAFE_NO_PAD.encode(b);
        json!({ "keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid,
            "n": enc(&parts.n),
            "e": enc(&parts.e),
        }]})
    }

    fn sign(header: Value, claims: Value) -> String {
        let enc = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
        let signed = format!("{}.{}", enc(&header), enc(&claims));
        let k = key();
        let mut sig = vec![0; k.public().modulus_len()];
        k.sign(&ring::signature::RSA_PKCS1_SHA256, &ring::rand::SystemRandom::new(), signed.as_bytes(), &mut sig).unwrap();
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(sig))
    }

    fn claims() -> Value {
        json!({ "iss": ISS, "aud": CLIENT, "sub": "user-123", "exp": 2_000, "iat": 1_000, "nonce": "n1", "email": "sam@example.com", "name": "Sam" })
    }

    #[test]
    fn a_token_signed_by_the_issuers_key_is_the_account() {
        let t = sign(json!({ "alg": "RS256", "kid": "k1" }), claims());
        let id = verify(&t, &jwks("k1"), ISS, CLIENT, Some("n1"), 1_500).unwrap();
        assert_eq!(id, Identity { sub: "user-123".into(), email: Some("sam@example.com".into()), name: Some("Sam".into()) });
    }

    #[test]
    fn a_forged_or_mismatched_token_is_refused() {
        let good = sign(json!({ "alg": "RS256", "kid": "k1" }), claims());
        let keys = jwks("k1");
        // The payload changed after signing.
        let (h, rest) = good.split_once('.').unwrap();
        let (_, s) = rest.split_once('.').unwrap();
        let other = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({ "iss": ISS, "aud": CLIENT, "sub": "someone-else", "exp": 2_000, "iat": 1_000 })).unwrap());
        assert_eq!(verify(&format!("{h}.{other}.{s}"), &keys, ISS, CLIENT, None, 1_500), Err(Bad::Invalid("signature")));
        // Another app's token, an old one, another attempt's, no account.
        assert_eq!(verify(&good, &keys, ISS, "oaiapp_other", Some("n1"), 1_500), Err(Bad::Invalid("audience")));
        assert_eq!(verify(&good, &keys, ISS, CLIENT, Some("n1"), 2_100), Err(Bad::Invalid("expiry")));
        assert_eq!(verify(&good, &keys, ISS, CLIENT, Some("n2"), 1_500), Err(Bad::Invalid("nonce")));
        assert_eq!(verify(&good, &keys, "https://auth.example.com", CLIENT, None, 1_500), Err(Bad::Invalid("issuer")));
        let mut no_sub = claims();
        no_sub["sub"] = json!("");
        assert_eq!(verify(&sign(json!({ "alg": "RS256", "kid": "k1" }), no_sub), &keys, ISS, CLIENT, None, 1_500), Err(Bad::Invalid("subject")));
        // Not RS256 ("none" above all).
        assert_eq!(verify(&sign(json!({ "alg": "none", "kid": "k1" }), claims()), &keys, ISS, CLIENT, None, 1_500), Err(Bad::Invalid("not RS256")));
    }

    #[test]
    fn a_key_the_set_doesnt_have_asks_for_the_set_again() {
        let t = sign(json!({ "alg": "RS256", "kid": "k2" }), claims());
        assert_eq!(verify(&t, &jwks("k1"), ISS, CLIENT, None, 1_500), Err(Bad::NoKey));
    }

    #[test]
    fn a_list_audience_needs_azp_when_it_names_others() {
        let mut c = claims();
        c["aud"] = json!([CLIENT, "https://api.openai.com/v1"]);
        let keys = jwks("k1");
        assert_eq!(verify(&sign(json!({ "alg": "RS256", "kid": "k1" }), c.clone()), &keys, ISS, CLIENT, None, 1_500), Err(Bad::Invalid("audience")));
        c["azp"] = json!(CLIENT);
        assert!(verify(&sign(json!({ "alg": "RS256", "kid": "k1" }), c), &keys, ISS, CLIENT, None, 1_500).is_ok());
    }
}
