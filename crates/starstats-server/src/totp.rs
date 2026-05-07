//! TOTP (RFC 6238) code generation, verification, and provisioning
//! URI building.
//!
//! We use [`totp_lite`] for the actual SHA-1 + truncation step and
//! hand-roll the rest because the moving parts (skew window,
//! provisioning URI) are too project-specific to want a wrapper:
//!
//! * **Skew window**: we accept the previous, current, and next
//!   30-second slot, so a user whose phone clock drifts a few
//!   seconds doesn't bounce off a perfectly-typed code. Three
//!   slots is the standard practice (Google Authenticator, Authy,
//!   1Password all do this).
//!
//! * **Provisioning URI**: the `otpauth://totp/...` link the user
//!   scans into their authenticator app. Built by hand because the
//!   URL-encoding requirements are picky and a bad encode shows up
//!   as "wrong code" with no debugging breadcrumb.
//!
//! Secret sizing: 20 bytes (160 bits). RFC 4226 §4 says "any HOTP
//! generator ... MUST support tokens up to 20 bytes." Most
//! authenticator apps require exactly 20 to render correctly, even
//! though 32 would be cryptographically stronger.

use rand::RngCore;
use totp_lite::{totp_custom, Sha1};

/// Length of the TOTP shared secret in bytes. 20 bytes = 160 bits =
/// the canonical RFC 4226 size.
pub const SECRET_LEN: usize = 20;

/// Canonical 30-second time step.
const STEP_SECS: u64 = 30;

/// Number of digits in the TOTP code. 6 is the universally-supported
/// authenticator-app default.
const DIGITS: u32 = 6;

/// Generate a fresh 20-byte secret. The caller encrypts this under
/// the KEK before storing it; the plaintext is shown to the user
/// once (as the QR-code base32) and never again.
pub fn generate_secret() -> [u8; SECRET_LEN] {
    let mut bytes = [0u8; SECRET_LEN];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

/// Encode `secret` for the `secret=` field of a provisioning URI.
/// RFC 6238 mandates RFC 4648 base32 with NO padding; trailing `=`
/// signs are valid but interfere with QR scanners that expect a
/// strict alphabet (Authy in particular has historically been
/// picky).
pub fn secret_to_base32(secret: &[u8]) -> String {
    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, secret)
}

/// Build the `otpauth://totp/{label}?secret=...` URI an authenticator
/// app reads from a QR code.
///
/// `label` is shown in the user's authenticator list; we use
/// `{issuer}:{username}` so two accounts on the same issuer don't
/// collide visually. `issuer` is also passed as a separate query
/// parameter — RFC 6238 says recent apps prefer this over the path
/// label and de-dupe accordingly.
pub fn provisioning_uri(secret_b32: &str, issuer: &str, username: &str) -> String {
    let label = format!("{}:{}", issuer, username);
    format!(
        "otpauth://totp/{label_enc}?secret={secret}&issuer={issuer_enc}&algorithm=SHA1&digits={digits}&period={period}",
        label_enc = encode_uri_segment(&label),
        secret = secret_b32,
        issuer_enc = encode_uri_segment(issuer),
        digits = DIGITS,
        period = STEP_SECS,
    )
}

/// Generate the TOTP code for `unix_seconds`. Returns the 6-digit
/// number as a zero-padded string. Used by tests + the verify
/// helper; production handlers don't call this directly.
pub fn code_at(secret: &[u8], unix_seconds: u64) -> String {
    totp_custom::<Sha1>(STEP_SECS, DIGITS, secret, unix_seconds)
}

/// Verify a candidate code against `secret` at the current wall-
/// clock time. Accepts the previous, current, and next slot. Codes
/// are normalised (whitespace stripped) so a paste-from-app with
/// an extra space passes. Constant-time-ish comparison: we do six
/// digit-equality checks, no early return.
pub fn verify_now(secret: &[u8], candidate: &str) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    verify_at(secret, candidate, now)
}

/// Same as [`verify_now`] but with an explicit time. Test seam.
pub fn verify_at(secret: &[u8], candidate: &str, unix_seconds: u64) -> bool {
    let candidate = candidate
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    if candidate.len() != DIGITS as usize || !candidate.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // 30s skew either side covers wall-clock drift on the user's
    // phone. Wider windows trade off security for forgiveness; 90s
    // total (3 slots) is the consensus default.
    for offset in [-1i64, 0, 1] {
        let t = if offset.is_negative() {
            unix_seconds.saturating_sub(STEP_SECS)
        } else if offset == 0 {
            unix_seconds
        } else {
            unix_seconds.saturating_add(STEP_SECS)
        };
        if constant_time_eq(code_at(secret, t).as_bytes(), candidate.as_bytes()) {
            return true;
        }
    }
    false
}

/// Constant-time byte equality. Avoids the branch-and-leak that a
/// naive `==` would expose to a timing-side-channel attacker. 6
/// bytes is short enough that the leak is largely theoretical, but
/// the cost of doing it right is two lines.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Percent-encode a single URI path/query segment. Same scope as
/// `rsi_verify::encode_path_segment` — unreserved characters pass
/// through, everything else gets `%HH`.
fn encode_uri_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.as_bytes() {
        let c = *b;
        let unreserved = c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(c as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{:02X}", c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_at_is_six_digits() {
        let secret = [0u8; SECRET_LEN];
        let code = code_at(&secret, 1_700_000_000);
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn verify_at_accepts_current_slot() {
        let secret = generate_secret();
        let now = 1_700_000_000;
        let code = code_at(&secret, now);
        assert!(verify_at(&secret, &code, now));
    }

    #[test]
    fn verify_at_accepts_previous_slot() {
        let secret = generate_secret();
        let now = 1_700_000_000;
        let prev_code = code_at(&secret, now - STEP_SECS);
        assert!(verify_at(&secret, &prev_code, now));
    }

    #[test]
    fn verify_at_accepts_next_slot() {
        let secret = generate_secret();
        let now = 1_700_000_000;
        let next_code = code_at(&secret, now + STEP_SECS);
        assert!(verify_at(&secret, &next_code, now));
    }

    #[test]
    fn verify_at_rejects_far_future_code() {
        let secret = generate_secret();
        let now = 1_700_000_000;
        let far = code_at(&secret, now + STEP_SECS * 10);
        assert!(!verify_at(&secret, &far, now));
    }

    #[test]
    fn verify_at_rejects_wrong_code() {
        let secret = generate_secret();
        assert!(!verify_at(&secret, "000000", 1_700_000_000));
    }

    #[test]
    fn verify_at_rejects_non_digit_input() {
        let secret = generate_secret();
        assert!(!verify_at(&secret, "abcdef", 1_700_000_000));
        assert!(!verify_at(&secret, "12345", 1_700_000_000));
        assert!(!verify_at(&secret, "1234567", 1_700_000_000));
    }

    #[test]
    fn verify_at_strips_whitespace() {
        let secret = generate_secret();
        let now = 1_700_000_000;
        let code = code_at(&secret, now);
        let spaced = format!("{} {}", &code[0..3], &code[3..6]);
        assert!(verify_at(&secret, &spaced, now));
    }

    #[test]
    fn provisioning_uri_includes_required_fields() {
        let uri = provisioning_uri("JBSWY3DPEHPK3PXP", "StarStats", "alice@example.com");
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains("StarStats%3Aalice%40example.com"));
        assert!(uri.contains("secret=JBSWY3DPEHPK3PXP"));
        assert!(uri.contains("issuer=StarStats"));
        assert!(uri.contains("algorithm=SHA1"));
        assert!(uri.contains("digits=6"));
        assert!(uri.contains("period=30"));
    }

    #[test]
    fn secret_to_base32_round_trips() {
        let bytes = generate_secret();
        let b32 = secret_to_base32(&bytes);
        let decoded = base32::decode(base32::Alphabet::Rfc4648 { padding: false }, &b32).unwrap();
        assert_eq!(decoded.as_slice(), bytes.as_slice());
    }
}
