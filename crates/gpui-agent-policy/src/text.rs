//! `TextResolver`: TYPE / `SET_VALUE` payload generation is *not* Instinct.
//!
//! `DeterministicTextResolver` with `TextContext`, `TextResolution` and
//! `TextError`. Instinct picks *which field*; this supplies *what to
//! type* — the quoted literal from the clause, exact and unaltered,
//! abstaining when none or more than one candidate applies. There is no
//! select grounding: the action space has no select kind.

use std::fmt;

use crate::goal::UiGoal;

/// Context for resolving typed text. Fingerprint so stale resolves are
/// dropped before dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextContext {
    /// The goal clause being resolved.
    pub goal: UiGoal,
    /// Accessibility label of the target field (used to disambiguate
    /// between multiple quoted literals).
    pub field_label: String,
    /// Role of the target field (e.g. `textbox`).
    pub field_role: String,
    /// Payloads successfully typed earlier in this run; not
    /// fingerprinted.
    pub typed: Vec<String>,
    /// Observation / action-space fingerprint bits the resolve was
    /// based on.
    pub context_fingerprint: u64,
}

impl TextContext {
    /// FNV-1a over the goal text, field label and the caller-supplied
    /// observation fingerprint.
    pub fn fingerprint(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for b in self.goal.as_str().as_bytes() {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        for b in self.field_label.as_bytes() {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= self.context_fingerprint;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        hash
    }
}

/// The resolved payload plus the fingerprint of the context it was
/// resolved against (drop it when the fingerprint no longer matches).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextResolution {
    /// Exact text to type, unaltered from the goal clause.
    pub text: String,
    /// [`TextContext::fingerprint`] at resolve time.
    pub context_fingerprint: u64,
}

/// Why a resolve failed; `Abstain` is a deliberate non-answer, not a
/// malfunction.
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "adding #[non_exhaustive] would break 0.3.0 callers' exhaustive matches"
)]
pub enum TextError {
    /// More than one quoted literal applies and the field label does
    /// not disambiguate — never guess.
    Ambiguous,
    /// The goal carries no usable text value for this field.
    Missing,
    /// A value was found but rejected as invalid.
    Invalid(String),
    /// The resolver declined to produce a value (e.g. every literal was
    /// already typed). The caller abstains; nothing is typed.
    Abstain(String),
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ambiguous => f.write_str("text resolution ambiguous"),
            Self::Missing => f.write_str("no text value found in goal/context"),
            Self::Invalid(msg) => write!(f, "invalid text: {msg}"),
            Self::Abstain(msg) => write!(f, "text resolver abstained: {msg}"),
        }
    }
}

impl std::error::Error for TextError {}

/// Produces the text to type into a field (TYPE / `SET_VALUE`
/// payloads). Not Instinct — Instinct picks *which* field.
pub trait TextResolver {
    /// Resolve `context` into a [`TextResolution`].
    ///
    /// # Errors
    /// [`TextError::Ambiguous`] when several literals apply,
    /// [`TextError::Missing`] when none do, [`TextError::Invalid`] for
    /// a rejected value, and [`TextError::Abstain`] when the resolver
    /// deliberately declines (e.g. every literal already typed).
    fn resolve(&mut self, context: &TextContext) -> Result<TextResolution, TextError>;
}

/// Deterministically extracts a field's text payload from the goal.
/// No model.
#[derive(Clone, Debug, Default)]
pub struct DeterministicTextResolver;

impl TextResolver for DeterministicTextResolver {
    fn resolve(&mut self, context: &TextContext) -> Result<TextResolution, TextError> {
        let goal = context.goal.as_str();
        let literals = quoted_literals(goal);
        if !literals.is_empty() {
            let untyped: Vec<_> = literals
                .into_iter()
                .filter(|(_, literal)| !context.typed.contains(literal))
                .collect();
            if untyped.is_empty() {
                return Err(TextError::Abstain(
                    "no quoted literal left for this field".into(),
                ));
            }

            // PRD M4.3: more than one candidate quote is `Ambiguous`
            // unless the field label disambiguates — never guess the
            // first literal.
            if untyped.len() == 1 {
                return Ok(TextResolution {
                    text: untyped[0].1.clone(),
                    context_fingerprint: context.fingerprint(),
                });
            }
            let label_words = ascii_words(&context.field_label);
            let selected = untyped
                .iter()
                .find(|(opening_quote, _)| {
                    let before = &goal[..*opening_quote];
                    let start = before
                        .char_indices()
                        .rev()
                        .nth(31)
                        .map_or(0, |(index, _)| index);
                    ascii_words(&before[start..])
                        .iter()
                        .rev()
                        .take(3)
                        .any(|word| label_words.contains(word))
                })
                .ok_or(TextError::Ambiguous)?;
            return Ok(TextResolution {
                text: selected.1.clone(),
                context_fingerprint: context.fingerprint(),
            });
        }

        if let Some(quoted) = first_quoted(goal) {
            if quoted.is_empty() {
                return Err(TextError::Missing);
            }
            return Ok(TextResolution {
                text: quoted,
                context_fingerprint: context.fingerprint(),
            });
        }
        // Pattern: type <value> into <field>
        if let Some(v) = type_into_pattern(goal, &context.field_label) {
            return Ok(TextResolution {
                text: v,
                context_fingerprint: context.fingerprint(),
            });
        }
        // Pattern: fill <field> with <value>
        if let Some(v) = fill_with_pattern(goal, &context.field_label) {
            return Ok(TextResolution {
                text: v,
                context_fingerprint: context.fingerprint(),
            });
        }
        Err(TextError::Missing)
    }
}

fn quoted_literals(goal: &str) -> Vec<(usize, String)> {
    let mut literals = Vec::new();
    let mut cursor = 0;
    while cursor < goal.len() {
        let Some((offset, opening)) = goal[cursor..]
            .char_indices()
            .find(|(_, character)| matches!(*character, '"' | '“'))
        else {
            break;
        };
        let opening_quote = cursor + offset;
        let (closing, content_start) = match opening {
            '"' => ('"', opening_quote + 1),
            '“' => ('”', opening_quote + opening.len_utf8()),
            _ => unreachable!(),
        };
        let Some((offset, _)) = goal[content_start..]
            .char_indices()
            .find(|(_, character)| *character == closing)
        else {
            cursor = content_start;
            continue;
        };
        let content_end = content_start + offset;
        if content_end == content_start {
            cursor = content_end + closing.len_utf8();
            continue;
        }
        literals.push((opening_quote, goal[content_start..content_end].to_owned()));
        cursor = content_end + closing.len_utf8();
    }
    literals
}

fn ascii_words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn first_quoted(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() {
                return Some(s[start..i].to_owned());
            }
            return None;
        }
        i += 1;
    }
    None
}

fn type_into_pattern(goal: &str, field: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    let field_l = field.to_ascii_lowercase();
    // "type VALUE into FIELD"
    let type_idx = lower.find("type ")?;
    let into_idx = lower.find(" into ")?;
    // "type into X" has no value between the verb and " into ".
    if into_idx < type_idx + 5 {
        return None;
    }
    let value = goal[type_idx + 5..into_idx].trim();
    let after = &lower[into_idx + 6..];
    if !after.contains(&field_l) && !field_l.is_empty() {
        // Still allow if field label empty / generic.
        if !field.is_empty() {
            return None;
        }
    }
    if value.is_empty() {
        return None;
    }
    Some(value.trim_matches(|c| c == '"' || c == '\'').to_owned())
}

fn fill_with_pattern(goal: &str, field: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    let field_l = field.to_ascii_lowercase();
    if !field_l.is_empty() && !lower.contains(&field_l) {
        return None;
    }
    let with_idx = lower.find(" with ")?;
    let value = goal[with_idx + 6..].trim();
    if value.is_empty() {
        return None;
    }
    Some(value.trim_matches(|c| c == '"' || c == '\'').to_owned())
}
