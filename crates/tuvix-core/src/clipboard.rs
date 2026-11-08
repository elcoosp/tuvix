//! OSC 52 clipboard: hand the terminal a string to put on the system
//! clipboard.
//!
//! The escape (`ESC ]52 ; <selection> ; <base64> ST`) is a *request* the
//! terminal may honor, ignore or prompt for — tuvix writes the sequence
//! and never assumes the clipboard changed. There is intentionally no
//! readback: OSC 52 clipboard *queries* are the variant terminals gate
//! behind prompts (and the reason many disable the escape entirely), so
//! the framework only ever writes.
//!
//! The payload is base64 (RFC 4648, standard alphabet, padded) — by
//! construction it can never contain `ESC`, ` BEL` or `ST`, so a
//! clipboard string can never break the sequence framing, the same
//! sanitization argument the M5 hyperlinks make at the type level. Empty
//! contents are the convention for *clearing* the clipboard.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::clipboard::{ClipboardSelection, encode_osc52};
//!
//! let sequence = encode_osc52(ClipboardSelection::System, "copied").unwrap();
//! assert_eq!(sequence, "\u{1b}]52;c;Y29waWVk\u{1b}\\");
//! ```

use std::fmt;

/// Which clipboard the sequence targets.
///
/// `System` is the `c` selection every OSC 52 terminal understands;
/// `Primary` is the X11/Wayland middle-click selection (`p`) — unix
/// terminals only, and commonly unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardSelection {
    /// The system clipboard (`c`).
    System,
    /// The primary selection (`p`, unix terminals).
    Primary,
}

impl ClipboardSelection {
    /// The OSC 52 selector character.
    #[must_use]
    pub fn selector(self) -> char {
        match self {
            Self::System => 'c',
            Self::Primary => 'p',
        }
    }
}

/// The encoded-payload cap: 64 KiB of base64 keeps the sequence inside
/// the limits terminals actually implement (tmux historically 16 KiB
/// (configurable), kitty 512 KiB, Windows Terminal 1 MiB) instead of
/// building a sequence no terminal will accept.
pub const OSC52_MAX_PAYLOAD: usize = 65_536;

/// What went wrong building a clipboard sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    /// The base64 payload would exceed [`OSC52_MAX_PAYLOAD`].
    TooLarge {
        /// The encoded size.
        size: usize,
        /// The cap.
        max: usize,
    },
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { size, max } => {
                write!(
                    f,
                    "clipboard payload {size} B exceeds the {max} B OSC 52 cap"
                )
            }
        }
    }
}

impl std::error::Error for ClipboardError {}

/// The base64 (RFC 4648 standard alphabet, padded) encoding of `input`.
///
/// In-house because the sequence is six lines of table math and the
/// clipboard contract should not grow a dependency.
#[must_use]
pub fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let byte0 = usize::from(chunk[0]);
        let byte1 = chunk.get(1).map_or(0, |b| usize::from(*b));
        let byte2 = chunk.get(2).map_or(0, |b| usize::from(*b));
        let triple = (byte0 << 16) | (byte1 << 8) | byte2;
        out.push(ALPHABET[(triple >> 18) & 0x3F] as char);
        out.push(ALPHABET[(triple >> 12) & 0x3F] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) & 0x3F] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple & 0x3F] as char
        } else {
            '='
        });
    }
    out
}

///
/// # Errors
/// Returns [`ClipboardError::TooLarge`] when the base64 payload
/// would exceed [`OSC52_MAX_PAYLOAD`].
/// Builds the OSC 52 *set clipboard* sequence for `contents`.
///
/// Empty contents build the clear-the-clipboard sequence. Fails with
/// [`ClipboardError::TooLarge`] when the base64 payload would exceed
/// [`OSC52_MAX_PAYLOAD`].
pub fn encode_osc52(
    selection: ClipboardSelection,
    contents: &str,
) -> Result<String, ClipboardError> {
    let payload = base64_encode(contents.as_bytes());
    if payload.len() > OSC52_MAX_PAYLOAD {
        return Err(ClipboardError::TooLarge {
            size: payload.len(),
            max: OSC52_MAX_PAYLOAD,
        });
    }
    Ok(format!(
        "\u{1b}]52;{};{}\u{1b}\\",
        selection.selector(),
        payload
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn every_byte_value_round_trips_through_the_alphabet() {
        let all: Vec<u8> = (0..=255_u8).collect();
        let encoded = base64_encode(&all);
        // the standard alphabet is escape-free by construction: no ESC,
        // no BEL, no ST byte can appear
        assert!(!encoded.contains('\u{1b}'));
        assert!(!encoded.contains('\u{7}'));
        assert_eq!(encoded.len(), 4 * all.len().div_ceil(3));
        assert!(encoded.ends_with('='));
    }

    #[test]
    fn the_sequence_is_pinned_byte_for_byte() {
        let sequence = encode_osc52(ClipboardSelection::System, "copied").unwrap();
        assert_eq!(sequence, "\u{1b}]52;c;Y29waWVk\u{1b}\\");
        let primary = encode_osc52(ClipboardSelection::Primary, "x").unwrap();
        assert_eq!(primary, "\u{1b}]52;p;eA==\u{1b}\\");
    }

    #[test]
    fn utf8_survives_the_encoding() {
        let sequence = encode_osc52(ClipboardSelection::System, "汉字!").unwrap();
        // 7 bytes of UTF-8 → ceil(7/3)*4 = 12 chars of base64
        let payload = sequence
            .split(';')
            .nth(2)
            .expect("selection;payload framing")
            .trim_end_matches("\u{1b}\\");
        assert_eq!(payload, "5rGJ5a2XIQ==");
    }

    #[test]
    fn empty_contents_build_the_clear_sequence() {
        let sequence = encode_osc52(ClipboardSelection::System, "").unwrap();
        assert_eq!(sequence, "\u{1b}]52;c;\u{1b}\\");
    }

    #[test]
    fn oversized_payloads_are_rejected_not_truncated() {
        let contents = "x".repeat(OSC52_MAX_PAYLOAD); // 4/3 of the cap once encoded
        let error = encode_osc52(ClipboardSelection::System, &contents).unwrap_err();
        assert_eq!(
            error,
            ClipboardError::TooLarge {
                size: 4 * contents.len().div_ceil(3),
                max: OSC52_MAX_PAYLOAD,
            }
        );
        let rendered = error.to_string();
        assert!(rendered.contains("exceeds"), "{rendered}");
    }

    #[test]
    fn the_largest_allowed_payload_still_encodes() {
        // 3 bytes per 4 base64 chars: exactly at the cap. The framing is
        // ESC ]52 ; c ; <payload> ESC \ → 7 + payload + 2.
        let contents = "y".repeat(OSC52_MAX_PAYLOAD / 4 * 3);
        let sequence = encode_osc52(ClipboardSelection::System, &contents).unwrap();
        assert_eq!(sequence.len(), 9 + OSC52_MAX_PAYLOAD);
    }
}
