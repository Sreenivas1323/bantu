//! Web Push alerts sent by the daemon itself, with no server of ours in the path.
//!
//! The browser subscribes through its own push service. To send an alert, the
//! daemon encrypts it for that browser (RFC 8291), signs a VAPID token with a key
//! it generated (RFC 8292) and posts it to the subscription's endpoint. The push
//! service only ever sees ciphertext. `curl` does the sending, so the daemon needs
//! no TLS stack of its own.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};

use aes_gcm::aead::{Aead, KeyInit as _};
use aes_gcm::Aes128Gcm;
use anyhow::{anyhow, Context, Result};
use bantu_protocol::{EventData, InboxKind, SessionStatus};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use serde_json::json;
use sha2::Sha256;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast::error::RecvError;

use crate::ops::{CommandError, Daemon};

const RECORD_SIZE: u32 = 4096;
/// Some push services reject tokens that last longer than 24 hours.
const VAPID_LIFETIME_SECS: i64 = 12 * 60 * 60;
/// How long the push service keeps trying if the device is offline.
const TTL_SECS: u32 = 24 * 60 * 60;
const MAX_BODY_CHARS: usize = 300;
/// RFC 8292 asks for a contact. Apple rejects `localhost` subjects, so this is the
/// project's page rather than anything about the user.
const VAPID_SUBJECT: &str = "https://github.com/Sreenivas1323/bantu";

#[derive(Default)]
pub struct Push {
    key: OnceLock<SecretKey>,
}

/// A browser's subscription, from `PushSubscription.toJSON()`.
#[derive(Debug, Clone)]
pub struct Subscription {
    pub endpoint: String,
    /// The browser's public key, base64url.
    pub p256dh: String,
    /// The shared auth secret, base64url.
    pub auth: String,
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::getrandom(&mut out).map_err(|e| anyhow!("no system randomness: {e}"))?;
    Ok(out)
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key size");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// The content key and nonce both sides derive (RFC 8291 section 3.4).
fn derive(
    shared_secret: &[u8],
    auth: &[u8],
    salt: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
) -> ([u8; 16], [u8; 12]) {
    let prk_key = hmac_sha256(auth, &[shared_secret]);
    let ikm = hmac_sha256(&prk_key, &[b"WebPush: info\0", ua_public, as_public, &[1]]);
    let prk = hmac_sha256(salt, &[&ikm]);
    let cek = hmac_sha256(&prk, &[b"Content-Encoding: aes128gcm\0", &[1]]);
    let nonce = hmac_sha256(&prk, &[b"Content-Encoding: nonce\0", &[1]]);
    (
        cek[..16].try_into().expect("16 bytes"),
        nonce[..12].try_into().expect("12 bytes"),
    )
}

/// Encrypts a message for one browser, as an `aes128gcm` body.
pub fn encrypt(
    plaintext: &[u8],
    ua_public: &[u8],
    auth: &[u8],
    as_secret: &SecretKey,
    salt: &[u8; 16],
) -> Result<Vec<u8>> {
    let peer = PublicKey::from_sec1_bytes(ua_public).context("bad subscription key")?;
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), peer.as_affine());
    let as_public = as_secret.public_key().to_encoded_point(false);
    let as_public = as_public.as_bytes();
    let (cek, nonce) = derive(
        shared.raw_secret_bytes().as_ref(),
        auth,
        salt,
        ua_public,
        as_public,
    );
    let mut record = plaintext.to_vec();
    // 0x02 marks the last record of the message (RFC 8188).
    record.push(2);
    let ciphertext = Aes128Gcm::new_from_slice(&cek)
        .expect("16-byte key")
        .encrypt(&nonce.into(), record.as_slice())
        .map_err(|_| anyhow!("encryption failed"))?;

    let mut body = Vec::with_capacity(21 + as_public.len() + ciphertext.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(as_public.len() as u8);
    body.extend_from_slice(as_public);
    body.extend_from_slice(&ciphertext);
    Ok(body)
}

/// What the browser does on receipt. Only used to test [`encrypt`] end to end.
pub fn decrypt(body: &[u8], ua_secret: &SecretKey, auth: &[u8]) -> Result<Vec<u8>> {
    let salt = body.get(..16).context("short body")?;
    let id_len = *body.get(20).context("short body")? as usize;
    let as_public = body.get(21..21 + id_len).context("short body")?;
    let ciphertext = body.get(21 + id_len..).context("short body")?;
    let peer = PublicKey::from_sec1_bytes(as_public).context("bad sender key")?;
    let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), peer.as_affine());
    let ua_public = ua_secret.public_key().to_encoded_point(false);
    let (cek, nonce) = derive(
        shared.raw_secret_bytes().as_ref(),
        auth,
        salt,
        ua_public.as_bytes(),
        as_public,
    );
    let mut record = Aes128Gcm::new_from_slice(&cek)
        .expect("16-byte key")
        .decrypt(&nonce.into(), ciphertext)
        .map_err(|_| anyhow!("decryption failed"))?;
    anyhow::ensure!(record.pop() == Some(2), "not a final record");
    Ok(record)
}

/// `scheme://host[:port]`, which is what a VAPID token is scoped to.
fn origin(endpoint: &str) -> Option<String> {
    let (scheme, rest) = endpoint.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

/// The `Authorization` header for one push service (RFC 8292).
pub fn vapid_header(key: &SecretKey, endpoint: &str, now_unix: i64) -> Result<String> {
    let aud = origin(endpoint).context("bad endpoint")?;
    let header = B64.encode(json!({"typ": "JWT", "alg": "ES256"}).to_string());
    let claims = B64.encode(
        json!({"aud": aud, "exp": now_unix + VAPID_LIFETIME_SECS, "sub": VAPID_SUBJECT})
            .to_string(),
    );
    let signing_input = format!("{header}.{claims}");
    let signature: Signature = SigningKey::from(key).sign(signing_input.as_bytes());
    let token = format!("{signing_input}.{}", B64.encode(signature.to_bytes()));
    Ok(format!("vapid t={token}, k={}", public_key_string(key)))
}

fn public_key_string(key: &SecretKey) -> String {
    B64.encode(key.public_key().to_encoded_point(false).as_bytes())
}

fn load_or_create_key(home: &Path) -> Result<SecretKey> {
    let path = home.join("vapid-key");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let bytes = B64.decode(text.trim()).context("reading vapid-key")?;
        return SecretKey::from_slice(&bytes).context("reading vapid-key");
    }
    // Almost every 32-byte value is a valid key; the rest are astronomically rare.
    let key = loop {
        if let Ok(key) = SecretKey::from_slice(&random_bytes::<32>()?) {
            break key;
        }
    };
    crate::write_private(&path, &B64.encode(key.to_bytes()))?;
    Ok(key)
}

/// Only HTTPS endpoints can be real push services. Plain HTTP stays on this
/// machine, which is enough for testing.
fn valid_endpoint(endpoint: &str) -> bool {
    let Some(origin) = origin(endpoint) else {
        return false;
    };
    origin.starts_with("https://")
        || origin.starts_with("http://127.0.0.1")
        || origin.starts_with("http://localhost")
}

impl Daemon {
    /// Loads the VAPID key and starts sending alerts.
    pub fn start_push(self: &Arc<Self>) -> Result<()> {
        let key = load_or_create_key(&self.config.home)?;
        let _ = self.push.key.set(key);
        let this = self.clone();
        tokio::spawn(async move { this.watch_for_alerts().await });
        Ok(())
    }

    /// The key browsers need to subscribe, base64url.
    pub fn push_public_key(&self) -> Option<String> {
        self.push.key.get().map(public_key_string)
    }

    pub async fn push_subscribe(
        &self,
        endpoint: String,
        p256dh: String,
        auth: String,
    ) -> Result<(), CommandError> {
        if !valid_endpoint(&endpoint) {
            return Err(CommandError::BadRequest(
                "push endpoints must use https".into(),
            ));
        }
        let valid_key = B64
            .decode(&p256dh)
            .ok()
            .is_some_and(|k| PublicKey::from_sec1_bytes(&k).is_ok());
        if !valid_key || B64.decode(&auth).is_err() {
            return Err(CommandError::BadRequest("bad subscription keys".into()));
        }
        self.store
            .upsert_push_subscription(&Subscription {
                endpoint,
                p256dh,
                auth,
            })
            .await?;
        Ok(())
    }

    /// Turns events that need a person into alerts.
    async fn watch_for_alerts(self: Arc<Self>) {
        let mut events = self.store.subscribe();
        let mut running: HashMap<String, bool> = HashMap::new();
        loop {
            let data = match events.recv().await {
                Ok(event) => event.data,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            };
            let alert = match &data {
                EventData::PermissionRequested { request } => Some((
                    request.workspace_id.clone(),
                    "needs your approval".to_string(),
                    request.tool_name.clone(),
                )),
                EventData::InboxAdded { item } => Some((
                    item.workspace_id.clone(),
                    match item.kind {
                        InboxKind::Question => "has a question".to_string(),
                        InboxKind::Notice => "sent a notice".to_string(),
                    },
                    item.title.clone(),
                )),
                EventData::SessionUpdated { session } => {
                    let now_running = session.status == SessionStatus::Running;
                    let was_running = running.insert(session.id.clone(), now_running);
                    (was_running == Some(true) && !now_running).then(|| {
                        (
                            session.workspace_id.clone(),
                            "is done".to_string(),
                            session.title.clone(),
                        )
                    })
                }
                _ => None,
            };
            if let Some((workspace_id, what, body)) = alert {
                let this = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = this.send_alert(&workspace_id, &what, &body).await {
                        tracing::warn!("couldn't send a push alert: {e:#}");
                    }
                });
            }
        }
    }

    async fn send_alert(&self, workspace_id: &str, what: &str, body: &str) -> Result<()> {
        let subscriptions = self.store.push_subscriptions().await?;
        if subscriptions.is_empty() {
            return Ok(());
        }
        let Some(ws) = self.store.workspace(workspace_id).await? else {
            return Ok(());
        };
        // Only runs a person started are worth a buzz when they finish. Loops
        // still alert when they need someone.
        if ws.loop_id.is_some() && what == "is done" {
            return Ok(());
        }
        let body: String = body.chars().take(MAX_BODY_CHARS).collect();
        let payload = json!({
            "title": format!("{} {what}", ws.name),
            "body": body,
            // A newer alert replaces an older one about the same workspace.
            "tag": format!("{workspace_id}-{what}"),
        })
        .to_string();
        let key = self.push.key.get().context("push isn't started")?;
        for sub in subscriptions {
            match deliver(key, &sub, payload.as_bytes()).await {
                // The browser unsubscribed, or the subscription expired.
                Ok(404 | 410) => {
                    self.store.delete_push_subscription(&sub.endpoint).await?;
                }
                Ok(status) if !(200..300).contains(&status) => {
                    tracing::warn!("push service answered HTTP {status}");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("couldn't reach a push service: {e:#}"),
            }
        }
        Ok(())
    }
}

/// Posts one encrypted message. Returns the HTTP status.
async fn deliver(key: &SecretKey, sub: &Subscription, payload: &[u8]) -> Result<u16> {
    let ua_public = B64.decode(&sub.p256dh)?;
    let auth = B64.decode(&sub.auth)?;
    let body = encrypt(payload, &ua_public, &auth, key, &random_bytes::<16>()?)?;
    let authorization = vapid_header(key, &sub.endpoint, chrono::Utc::now().timestamp())?;
    let mut child = tokio::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "30",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
        ])
        .args(["-X", "POST", "--data-binary", "@-"])
        .args(["-H", "Content-Encoding: aes128gcm"])
        .args(["-H", "Content-Type: application/octet-stream"])
        .args(["-H", "Urgency: high"])
        .args(["-H", &format!("TTL: {TTL_SECS}")])
        .args(["-H", &format!("Authorization: {authorization}")])
        .arg("--")
        .arg(&sub.endpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("running curl")?;
    let mut stdin = child.stdin.take().expect("piped");
    stdin.write_all(&body).await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    let status = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    if status == 0 {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;

    fn b64(s: &str) -> Vec<u8> {
        B64.decode(s).unwrap()
    }

    /// The worked example from RFC 8291, Appendix A.
    #[test]
    fn matches_the_rfc_8291_test_vector() {
        let plaintext = b64("V2hlbiBJIGdyb3cgdXAsIEkgd2FudCB0byBiZSBhIHdhdGVybWVsb24");
        let as_secret =
            SecretKey::from_slice(&b64("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        let ua_secret =
            SecretKey::from_slice(&b64("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94")).unwrap();
        let ua_public = b64(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
        );
        let salt: [u8; 16] = b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let auth = b64("BTBZMqHH6r4Tts7J_aSIgg");
        let expected = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";

        let body = encrypt(&plaintext, &ua_public, &auth, &as_secret, &salt).unwrap();
        assert_eq!(B64.encode(&body), expected);
        assert_eq!(decrypt(&body, &ua_secret, &auth).unwrap(), plaintext);
        assert!(decrypt(&body, &ua_secret, b"wrong secret").is_err());
    }

    #[test]
    fn signs_vapid_tokens() {
        let key = SecretKey::from_slice(&[7u8; 32]).unwrap();
        let header = vapid_header(&key, "https://fcm.googleapis.com/fcm/send/abc", 1000).unwrap();
        let rest = header.strip_prefix("vapid t=").unwrap();
        let (token, public) = rest.split_once(", k=").unwrap();
        assert_eq!(b64(public).len(), 65);
        let (signed, signature) = token.rsplit_once('.').unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&b64(signed.split('.').nth(1).unwrap())).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["exp"], 1000 + VAPID_LIFETIME_SECS);
        assert!(claims["sub"].as_str().unwrap().starts_with("https://"));
        let signature = Signature::from_slice(&b64(signature)).unwrap();
        VerifyingKey::from(&SigningKey::from(&key))
            .verify(signed.as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn keeps_the_key_and_checks_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create_key(dir.path()).unwrap();
        let second = load_or_create_key(dir.path()).unwrap();
        assert_eq!(first.to_bytes(), second.to_bytes());
        assert!(valid_endpoint(
            "https://updates.push.services.mozilla.com/wpush/v2/x"
        ));
        assert!(valid_endpoint("http://127.0.0.1:9000/push"));
        assert!(!valid_endpoint("http://evil.example/push"));
        assert!(!valid_endpoint("file:///etc/passwd"));
        assert!(!valid_endpoint("not a url"));
    }
}
