//! Protocol v2 HMAC-SHA256 helpers. Hex is hand-rolled (no extra crate).

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::protocol::PROTOCOL_VERSION;

type HmacSha256 = Hmac<Sha256>;

/// Challenge nonce length in bytes.
pub const NONCE_LEN: usize = 32;

/// Wire challenge written immediately after accept when the host has a token.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Challenge {
    /// Protocol version (`v`).
    pub v: u32,
    /// Always `"challenge"`.
    pub op: String,
    /// The nonce, hex-encoded.
    pub nonce: String,
}

/// Lowercase hex of `bytes`.
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0xf)]));
    }
    out
}

/// Bytes of a hex string (case-insensitive).
///
/// # Errors
///
/// Odd length or a non-hex character.
pub fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err("hex length must be even".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("invalid hex".into()),
    }
}

/// 32-byte nonce from `/dev/urandom` (Unix) or `BCryptGenRandom` (Windows).
///
/// # Errors
///
/// The platform CSPRNG fails or the target has none.
pub fn random_nonce() -> Result<[u8; NONCE_LEN], String> {
    let mut buf = [0u8; NONCE_LEN];
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut buf))
            .map_err(|err| format!("urandom: {err}"))?;
        Ok(buf)
    }
    #[cfg(windows)]
    {
        // System-preferred RNG via CNG (same family getrandom uses on Windows).
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                h_algorithm: *mut core::ffi::c_void,
                pb_buffer: *mut u8,
                cb_buffer: u32,
                dw_flags: u32,
            ) -> i32;
        }
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
        let status = unsafe {
            BCryptGenRandom(
                core::ptr::null_mut(),
                buf.as_mut_ptr(),
                u32::try_from(NONCE_LEN).unwrap_or(u32::MAX),
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            return Err(format!("BCryptGenRandom failed: 0x{status:08x}"));
        }
        Ok(buf)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = buf;
        Err("CSPRNG unavailable on this target".into())
    }
}

/// `HMAC-SHA256(token, nonce)`, hex-encoded.
///
/// # Errors
///
/// The key cannot initialize (does not happen for arbitrary bytes).
pub fn hmac_hex(token: &str, nonce: &[u8]) -> Result<String, String> {
    let mut mac =
        HmacSha256::new_from_slice(token.as_bytes()).map_err(|_| "invalid HMAC key".to_string())?;
    mac.update(nonce);
    Ok(hex_encode(&mac.finalize().into_bytes()))
}

/// Whether `auth_hex` is `HMAC-SHA256(token, nonce)` (constant-time compare).
#[must_use]
pub fn hmac_verify(token: &str, nonce: &[u8], auth_hex: &str) -> bool {
    let Ok(got) = hex_decode(auth_hex) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(token.as_bytes()) else {
        return false;
    };
    mac.update(nonce);
    mac.verify_slice(&got).is_ok()
}

/// The `{"op":"challenge",...}` the server writes on connect.
#[must_use]
pub fn challenge_for_nonce(nonce: &[u8; NONCE_LEN]) -> Challenge {
    Challenge {
        v: PROTOCOL_VERSION,
        op: "challenge".into(),
        nonce: hex_encode(nonce),
    }
}

/// The nonce of a challenge line the server sent.
///
/// # Errors
///
/// Bad JSON, a version or op mismatch, a non-hex nonce, or the wrong
/// nonce length.
pub fn parse_challenge_line(line: &[u8]) -> Result<[u8; NONCE_LEN], String> {
    let challenge: Challenge =
        serde_json::from_slice(line).map_err(|err| format!("bad challenge: {err}"))?;
    if challenge.v != PROTOCOL_VERSION {
        return Err(format!(
            "unsupported challenge version {} (want {PROTOCOL_VERSION})",
            challenge.v
        ));
    }
    if challenge.op != "challenge" {
        return Err(format!("expected op=challenge, got {}", challenge.op));
    }
    let bytes = hex_decode(&challenge.nonce)?;
    if bytes.len() != NONCE_LEN {
        return Err("challenge nonce must be 32 bytes".into());
    }
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes);
    Ok(nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_nonce_is_32_nonzero_bytes() {
        let a = random_nonce().expect("cspng");
        let b = random_nonce().expect("cspng");
        assert_eq!(a.len(), NONCE_LEN);
        assert_ne!(a, [0u8; NONCE_LEN], "nonce must not be all zeros");
        assert_ne!(a, b, "two nonces must differ");
    }
}
