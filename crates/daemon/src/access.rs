//! Cloudflare Access in front of the daemon.
//!
//! With Access on a hostname, Cloudflare signs in each visitor and adds a JWT
//! (`Cf-Access-Jwt-Assertion`) to every request it forwards. Checking it here
//! means a mistake in the Access setup, like a missing policy or a hostname left
//! out, fails closed instead of leaving the daemon open to anyone holding its
//! token. Only requests for a public hostname are checked: the daemon listens on
//! loopback, so anything else comes from this machine.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::sha2::Sha256;
use rsa::signature::Verifier;
use rsa::{BigUint, RsaPublicKey};
use serde::Deserialize;
use tokio::sync::Mutex;

/// Access rotates its keys every few weeks; an unknown key id triggers a refresh,
/// but at most this often, so forged tokens can't make the daemon hammer Cloudflare.
const MIN_REFRESH: Duration = Duration::from_secs(60);
/// Clock skew allowed on `exp` and `nbf`.
const LEEWAY_SECS: i64 = 60;

#[derive(Debug, Clone)]
pub struct AccessConfig {
    /// `https://<team>.cloudflareaccess.com`, which is also the token's issuer.
    pub team_domain: String,
    /// Application audience (AUD) tags. A token must be for one of them.
    pub audiences: Vec<String>,
    /// Where the signing keys are published.
    pub certs_url: String,
}

impl AccessConfig {
    /// From a team name (`myteam`) or domain, and the audience tags.
    pub fn new(team: &str, audiences: Vec<String>) -> Result<Self> {
        let team = team.trim().trim_end_matches('/');
        let team_domain = if team.starts_with("https://") {
            team.to_string()
        } else if team.contains('.') {
            format!("https://{team}")
        } else {
            format!("https://{team}.cloudflareaccess.com")
        };
        let audiences: Vec<String> = audiences
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        if audiences.is_empty() {
            bail!("Cloudflare Access needs at least one application audience (AUD) tag");
        }
        Ok(Self {
            certs_url: format!("{team_domain}/cdn-cgi/access/certs"),
            team_domain,
            audiences,
        })
    }
}

pub struct AccessVerifier {
    config: AccessConfig,
    keys: Mutex<Keys>,
}

#[derive(Default)]
struct Keys {
    by_id: HashMap<String, RsaPublicKey>,
    fetched: Option<Instant>,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    #[serde(default, deserialize_with = "one_or_many")]
    aud: Vec<String>,
    iss: String,
    exp: i64,
    #[serde(default)]
    nbf: Option<i64>,
    #[serde(default)]
    email: Option<String>,
}

fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Aud {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Aud::deserialize(d)? {
        Aud::One(a) => vec![a],
        Aud::Many(a) => a,
    })
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    n: String,
    e: String,
}

/// Public keys from a JWKS document, by key id. Keys that aren't RSA are skipped.
fn parse_jwks(body: &str) -> Result<HashMap<String, RsaPublicKey>> {
    let jwks: Jwks = serde_json::from_str(body).context("Access keys aren't a JWKS document")?;
    let mut out = HashMap::new();
    for key in jwks.keys.into_iter().filter(|k| k.kty == "RSA") {
        let n = BigUint::from_bytes_be(&URL_SAFE_NO_PAD.decode(&key.n)?);
        let e = BigUint::from_bytes_be(&URL_SAFE_NO_PAD.decode(&key.e)?);
        out.insert(key.kid, RsaPublicKey::new(n, e)?);
    }
    Ok(out)
}

impl AccessVerifier {
    pub fn new(config: AccessConfig) -> Self {
        Self {
            config,
            keys: Mutex::new(Keys::default()),
        }
    }

    /// Checks a token at time `now` (seconds since the epoch), and returns the
    /// signed-in email if there is one.
    pub async fn verify(&self, token: &str, now: i64) -> Result<Option<String>> {
        let mut parts = token.split('.');
        let (Some(h), Some(p), Some(s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            bail!("not a JWT");
        };
        let header: Header = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(h)?)?;
        if header.alg != "RS256" {
            bail!("unexpected signing algorithm {}", header.alg);
        }
        let kid = header
            .kid
            .ok_or_else(|| anyhow!("the token names no key"))?;
        let key = self.key(&kid).await?;
        let signature = Signature::try_from(URL_SAFE_NO_PAD.decode(s)?.as_slice())?;
        VerifyingKey::<Sha256>::new(key)
            .verify(format!("{h}.{p}").as_bytes(), &signature)
            .map_err(|_| anyhow!("bad signature"))?;

        let claims: Claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(p)?)?;
        if claims.iss.trim_end_matches('/') != self.config.team_domain {
            bail!("issued by {}, not {}", claims.iss, self.config.team_domain);
        }
        if !claims.aud.iter().any(|a| self.config.audiences.contains(a)) {
            bail!("the token is for another Access application");
        }
        if claims.exp + LEEWAY_SECS < now {
            bail!("the Access sign-in has expired");
        }
        if claims.nbf.is_some_and(|nbf| nbf - LEEWAY_SECS > now) {
            bail!("the token isn't valid yet");
        }
        Ok(claims.email)
    }

    async fn key(&self, kid: &str) -> Result<RsaPublicKey> {
        let mut keys = self.keys.lock().await;
        if let Some(key) = keys.by_id.get(kid) {
            return Ok(key.clone());
        }
        let due = keys.fetched.is_none_or(|t| t.elapsed() >= MIN_REFRESH);
        if due {
            keys.fetched = Some(Instant::now());
            match fetch(&self.config.certs_url)
                .await
                .and_then(|b| parse_jwks(&b))
            {
                Ok(fresh) => keys.by_id = fresh,
                Err(e) => tracing::warn!("couldn't fetch Cloudflare Access keys: {e:#}"),
            }
        }
        keys.by_id
            .get(kid)
            .cloned()
            .ok_or_else(|| anyhow!("unknown signing key {kid}"))
    }
}

/// Through `curl`, like Web Push, so the daemon needs no TLS stack of its own.
async fn fetch(url: &str) -> Result<String> {
    let out = tokio::process::Command::new("curl")
        .args(["-fsS", "--max-time", "10", "--proto", "=https,http", url])
        .kill_on_drop(true)
        .output()
        .await
        .context("couldn't run curl")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8(out.stdout)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::traits::PublicKeyParts;
    use rsa::RsaPrivateKey;

    /// A key and the JWKS publishing it, for tests here and in the API tests.
    pub fn test_key(kid: &str) -> (RsaPrivateKey, String) {
        let mut rng = rsa::rand_core::OsRng;
        // Small, because generating keys is slow in debug builds; verifying
        // works the same at any size.
        let private = RsaPrivateKey::new(&mut rng, 1024).unwrap();
        let public = private.to_public_key();
        let jwks = serde_json::json!({"keys": [{
            "kid": kid, "kty": "RSA", "alg": "RS256", "use": "sig",
            "n": URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
        }]});
        (private, jwks.to_string())
    }

    pub fn sign(key: &RsaPrivateKey, kid: &str, claims: serde_json::Value) -> String {
        let header =
            URL_SAFE_NO_PAD.encode(serde_json::json!({"alg":"RS256","kid":kid}).to_string());
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signing = SigningKey::<Sha256>::new(key.clone());
        let signature = signing.sign(format!("{header}.{payload}").as_bytes());
        format!(
            "{header}.{payload}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    fn verifier(jwks: &str) -> AccessVerifier {
        let config =
            AccessConfig::new("myteam", vec!["app-aud".into(), "preview-aud".into()]).unwrap();
        let v = AccessVerifier::new(config);
        let keys = parse_jwks(jwks).unwrap();
        v.keys.try_lock().unwrap().by_id = keys;
        v.keys.try_lock().unwrap().fetched = Some(Instant::now());
        v
    }

    #[test]
    fn reads_team_names() {
        let c = AccessConfig::new("myteam", vec!["a".into()]).unwrap();
        assert_eq!(c.team_domain, "https://myteam.cloudflareaccess.com");
        assert_eq!(
            c.certs_url,
            "https://myteam.cloudflareaccess.com/cdn-cgi/access/certs"
        );
        let c =
            AccessConfig::new("https://myteam.cloudflareaccess.com/", vec!["a".into()]).unwrap();
        assert_eq!(c.team_domain, "https://myteam.cloudflareaccess.com");
        assert!(AccessConfig::new("myteam", vec![" ".into()]).is_err());
    }

    #[tokio::test]
    async fn accepts_only_good_tokens() {
        let (key, jwks) = test_key("k1");
        let (other_key, _) = test_key("k1");
        let v = verifier(&jwks);
        let now = 1_800_000_000;
        let claims = |aud: &str, iss: &str, exp: i64| serde_json::json!({"aud": [aud], "iss": iss, "exp": exp, "nbf": now - 10, "email": "me@example.com"});
        let iss = "https://myteam.cloudflareaccess.com";

        let good = sign(&key, "k1", claims("preview-aud", iss, now + 300));
        assert_eq!(
            v.verify(&good, now).await.unwrap().as_deref(),
            Some("me@example.com")
        );
        // A single audience string is fine too.
        let single = sign(
            &key,
            "k1",
            serde_json::json!({"aud": "app-aud", "iss": iss, "exp": now + 300}),
        );
        assert!(v.verify(&single, now).await.is_ok());

        let cases = [
            (
                "another app",
                sign(&key, "k1", claims("someone-else", iss, now + 300)),
            ),
            (
                "another team",
                sign(
                    &key,
                    "k1",
                    claims("app-aud", "https://evil.cloudflareaccess.com", now + 300),
                ),
            ),
            (
                "expired",
                sign(&key, "k1", claims("app-aud", iss, now - 3600)),
            ),
            (
                "forged",
                sign(&other_key, "k1", claims("app-aud", iss, now + 300)),
            ),
            (
                "unknown key",
                sign(&key, "k2", claims("app-aud", iss, now + 300)),
            ),
            ("garbage", "a.b".to_string()),
        ];
        for (what, token) in cases {
            assert!(v.verify(&token, now).await.is_err(), "{what} was accepted");
        }
        // Tampering with the payload breaks the signature.
        let mut parts: Vec<&str> = good.split('.').collect();
        let evil = URL_SAFE_NO_PAD.encode(claims("app-aud", iss, now + 999_999).to_string());
        parts[1] = &evil;
        assert!(v.verify(&parts.join("."), now).await.is_err());
        // So does claiming another algorithm.
        let none = format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none","kid":"k1"}"#),
            parts[1]
        );
        assert!(v.verify(&none, now).await.is_err());
    }
}
