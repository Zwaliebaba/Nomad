//! Universe identities and creation parameters (N2 plan §5; D-079, D-106, D-107).

use std::fmt;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::str::FromStr;

use nomad_types::UniverseId;

/// Characters of the text form of a [`UniverseId`]: Crockford's base-32 alphabet in lower
/// case. It has no I, L, O or U, so an ID reads back without confusion, and lower case
/// keeps directory names identical on case-insensitive file systems.
///
/// *Proposed:* D-107 fixes "8-character base-32" but not the alphabet.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Characters in the text form (D-107).
pub const UNIVERSE_ID_LEN: usize = 8;

/// Bits carried by the text form: 8 base-32 characters.
const UNIVERSE_ID_BITS: u32 = 40;

/// The 8-character base-32 text form of `id` (D-107), used as its directory name.
///
/// Only the low 40 bits are shown. Every identity this server creates or parses fits.
pub fn universe_id_text(id: UniverseId) -> String {
    (0..UNIVERSE_ID_LEN)
        .rev()
        .map(|digit| {
            let shift = 5 * u32::try_from(digit).unwrap_or(0);
            let index = usize::try_from((id.0 >> shift) & 0x1f).unwrap_or(0);
            char::from(ALPHABET[index])
        })
        .collect()
}

/// Why a text is not a Universe ID.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{text:?} is not a Universe ID (8 characters of 0-9 and a-z without i, l, o, u)")]
pub struct ParseUniverseIdError {
    text: String,
}

/// Parses the text form written by [`universe_id_text`]. Upper case is accepted.
pub fn parse_universe_id(text: &str) -> Result<UniverseId, ParseUniverseIdError> {
    let error = || ParseUniverseIdError {
        text: text.to_owned(),
    };
    if text.len() != UNIVERSE_ID_LEN {
        return Err(error());
    }
    let mut value = 0_u64;
    for byte in text.bytes() {
        let lower = byte.to_ascii_lowercase();
        let digit = ALPHABET
            .iter()
            .position(|&c| c == lower)
            .ok_or_else(error)?;
        value = (value << 5) | u64::try_from(digit).map_err(|_| error())?;
    }
    Ok(UniverseId(value))
}

/// A fresh random Universe identity that fits the 8-character text form.
pub fn random_universe_id() -> UniverseId {
    UniverseId(random_u64() & ((1 << UNIVERSE_ID_BITS) - 1))
}

/// A random 64-bit number from the operating system's hash seed, for identities and
/// default seeds. Not for anything that needs to be unpredictable to an attacker.
pub(crate) fn random_u64() -> u64 {
    // `RandomState` keys come from the operating system's random source.
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.finish()
}

/// AI difficulty (D-079, doc 13 §7). Changes competence only, never resources or
/// information. Normal is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Difficulty {
    /// Slow reactions, few commands.
    Easy,
    /// The MVP default.
    #[default]
    Normal,
    /// Close to the human command cap.
    Hard,
}

impl Difficulty {
    /// The stored and displayed name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }
}

impl fmt::Display for Difficulty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a text is not a difficulty.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a difficulty (easy, normal or hard)")]
pub struct ParseDifficultyError(String);

impl FromStr for Difficulty {
    type Err = ParseDifficultyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.to_ascii_lowercase().as_str() {
            "easy" => Ok(Self::Easy),
            "normal" => Ok(Self::Normal),
            "hard" => Ok(Self::Hard),
            _ => Err(ParseDifficultyError(text.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_form_round_trips() {
        for raw in [0, 1, 31, 32, 0x12_3456_789a, (1 << 40) - 1] {
            let id = UniverseId(raw);
            let text = universe_id_text(id);
            assert_eq!(text.len(), UNIVERSE_ID_LEN);
            assert_eq!(parse_universe_id(&text), Ok(id));
            assert_eq!(parse_universe_id(&text.to_ascii_uppercase()), Ok(id));
        }
        assert_eq!(universe_id_text(UniverseId(0)), "00000000");
        assert_eq!(universe_id_text(UniverseId((1 << 40) - 1)), "zzzzzzzz");
    }

    #[test]
    fn bad_text_is_rejected() {
        for text in ["", "0000000", "000000000", "0000000i", "0000000-", "ééééé"] {
            assert!(parse_universe_id(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn random_ids_fit_and_differ() {
        let a = random_universe_id();
        let b = random_universe_id();
        assert!(a.0 < 1 << 40 && b.0 < 1 << 40);
        assert_ne!(a, b);
    }

    #[test]
    fn difficulty_parses_and_defaults_to_normal() {
        assert_eq!(Difficulty::default(), Difficulty::Normal);
        for d in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            assert_eq!(d.as_str().parse(), Ok(d));
        }
        assert_eq!("HARD".parse(), Ok(Difficulty::Hard));
        assert!("brutal".parse::<Difficulty>().is_err());
    }
}
