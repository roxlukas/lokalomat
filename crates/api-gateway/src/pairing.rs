//! Pairing a phone with the computer (PRD §15 item 12):
//!
//! 1. the computer shows a QR code with its address, port, the SHA-256
//!    fingerprint of its TLS certificate and a one-time token (~2 min);
//! 2. the phone connects over TLS pinned to that fingerprint and sends its
//!    Ed25519 public key, signed together with the token;
//! 3. both sides show a short comparison code (`ABC-DEF`) derived from the
//!    certificate fingerprint, the phone key and the token – the user
//!    confirms on the computer that both codes are the same. A man in the
//!    middle has to use his own key or certificate, so the codes differ;
//! 4. the computer issues a per-device access key.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};

/// How long a pairing QR code stays valid.
pub const TOKEN_TTL_SECS: u64 = 120;
/// How long the computer waits for the user to compare the codes.
pub const CONFIRM_TTL_SECS: u64 = 120;
const DOMAIN: &[u8] = b"lokalomat-pair-v1";
/// 32 symbols without look-alikes (no I, O, 0, 1).
const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const SCHEME: &str = "lokalomat://pair";

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim()).ok()
}

/// `n` random bytes from the OS.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the OS random number generator is unavailable");
    b
}

/// Comparison without early exit (tokens, keys).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let d = ring::digest::digest(&ring::digest::SHA256, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}

/// What the QR code carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QrPayload {
    pub host: String,
    pub port: u16,
    /// SHA-256 of the server certificate (DER).
    pub fingerprint: [u8; 32],
    pub token: String,
    /// Computer name shown on the phone.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("this is not a Lokalomat pairing code")]
pub struct BadPayload;

impl QrPayload {
    /// `lokalomat://pair?h=…&p=…&f=…&t=…&n=…` (name base64url-encoded).
    pub fn to_uri(&self) -> String {
        format!(
            "{SCHEME}?h={}&p={}&f={}&t={}&n={}",
            self.host,
            self.port,
            b64(&self.fingerprint),
            self.token,
            b64(self.name.as_bytes())
        )
    }

    pub fn parse(uri: &str) -> Result<Self, BadPayload> {
        let query = uri
            .trim()
            .strip_prefix(SCHEME)
            .and_then(|r| r.strip_prefix('?'))
            .ok_or(BadPayload)?;
        let mut host = None;
        let mut port = None;
        let mut fp = None;
        let mut token = None;
        let mut name = String::new();
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').ok_or(BadPayload)?;
            match k {
                "h" => host = Some(v.to_owned()),
                "p" => port = v.parse::<u16>().ok(),
                "f" => fp = unb64(v).and_then(|b| <[u8; 32]>::try_from(b).ok()),
                "t" => token = Some(v.to_owned()),
                "n" => {
                    name = unb64(v)
                        .and_then(|b| String::from_utf8(b).ok())
                        .unwrap_or_default()
                }
                _ => {}
            }
        }
        let host = host.filter(|h| {
            !h.is_empty()
                && h.len() <= 253
                && h.chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c))
        });
        let token = token.filter(|t| !t.is_empty() && t.len() <= 64);
        match (host, port, fp, token) {
            (Some(host), Some(port), Some(fingerprint), Some(token)) if port > 0 => Ok(QrPayload {
                host,
                port,
                fingerprint,
                token,
                name,
            }),
            _ => Err(BadPayload),
        }
    }
}

/// A fresh one-time pairing token (base64url, 128 bits).
pub fn new_token() -> String {
    b64(&random_bytes::<16>())
}

/// The short code both screens show, e.g. `K7M-Q2X` (30 bits).
pub fn comparison_code(fingerprint: &[u8; 32], phone_key: &[u8], token: &str) -> String {
    let mut data = Vec::with_capacity(DOMAIN.len() + 32 + phone_key.len() + token.len() + 3);
    data.extend_from_slice(DOMAIN);
    data.push(0);
    data.extend_from_slice(fingerprint);
    data.push(0);
    data.extend_from_slice(phone_key);
    data.push(0);
    data.extend_from_slice(token.as_bytes());
    let h = sha256(&data);
    let bits = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) >> 2;
    let chars: String = (0..6)
        .map(|i| ALPHABET[((bits >> (25 - 5 * i)) & 31) as usize] as char)
        .collect();
    format!("{}-{}", &chars[..3], &chars[3..])
}

/// What the phone signs: the domain, the token and the pinned fingerprint.
pub fn signed_message(token: &str, fingerprint: &[u8; 32]) -> Vec<u8> {
    let mut m = DOMAIN.to_vec();
    m.push(0);
    m.extend_from_slice(token.as_bytes());
    m.push(0);
    m.extend_from_slice(fingerprint);
    m
}

pub fn verify_signature(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    public_key.len() == 32
        && UnparsedPublicKey::new(&ED25519, public_key)
            .verify(message, signature)
            .is_ok()
}

/// The phone's long-term Ed25519 key, kept as its 32-byte seed.
pub struct DeviceIdentity {
    seed: [u8; 32],
    pair: Ed25519KeyPair,
}

impl DeviceIdentity {
    pub fn generate() -> Self {
        Self::from_seed(random_bytes::<32>())
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        let pair =
            Ed25519KeyPair::from_seed_unchecked(&seed).expect("a 32-byte seed is always valid");
        DeviceIdentity { seed, pair }
    }

    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    pub fn public_key(&self) -> Vec<u8> {
        self.pair.public_key().as_ref().to_vec()
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.pair.sign(message).as_ref().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> QrPayload {
        QrPayload {
            host: "192.168.1.20".into(),
            port: 47821,
            fingerprint: [7u8; 32],
            token: new_token(),
            name: "Komputer Łukasza".into(),
        }
    }

    #[test]
    fn qr_payload_roundtrip_and_rejects_garbage() {
        let p = payload();
        let uri = p.to_uri();
        assert!(uri.starts_with("lokalomat://pair?h=192.168.1.20&p=47821&f="));
        assert_eq!(QrPayload::parse(&uri).unwrap(), p);
        assert!(QrPayload::parse("https://example.com").is_err());
        assert!(QrPayload::parse("lokalomat://pair?h=a&p=0&f=AA&t=x").is_err());
        let evil = uri.replace("192.168.1.20", "a/b?c");
        assert!(QrPayload::parse(&evil).is_err());
    }

    #[test]
    fn codes_match_only_for_the_same_keys_and_token() {
        let fp = [1u8; 32];
        let phone = DeviceIdentity::from_seed([2u8; 32]);
        let code = comparison_code(&fp, &phone.public_key(), "tok");
        assert_eq!(code.len(), 7);
        assert_eq!(&code[3..4], "-");
        assert!(
            code.chars()
                .filter(|c| *c != '-')
                .all(|c| ALPHABET.contains(&(c as u8)))
        );
        assert_eq!(code, comparison_code(&fp, &phone.public_key(), "tok"));
        // Another phone key (man in the middle), another certificate or token → another code.
        let mitm = DeviceIdentity::from_seed([3u8; 32]);
        assert_ne!(code, comparison_code(&fp, &mitm.public_key(), "tok"));
        assert_ne!(
            code,
            comparison_code(&[9u8; 32], &phone.public_key(), "tok")
        );
        assert_ne!(code, comparison_code(&fp, &phone.public_key(), "tok2"));
    }

    #[test]
    fn signatures_verify() {
        let id = DeviceIdentity::generate();
        let msg = signed_message("t", &[0u8; 32]);
        let sig = id.sign(&msg);
        assert!(verify_signature(&id.public_key(), &msg, &sig));
        assert!(!verify_signature(
            &id.public_key(),
            &signed_message("u", &[0u8; 32]),
            &sig
        ));
        let again = DeviceIdentity::from_seed(*id.seed());
        assert_eq!(again.public_key(), id.public_key());
        assert!(ct_eq(b"abc", b"abc") && !ct_eq(b"abc", b"abd") && !ct_eq(b"ab", b"abc"));
    }
}
