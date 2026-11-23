//! Code-completion helpers built on top of the symbol index.
//!
//! Dev-C++ shows a popup list after typing `.` or `->` and after `Ctrl+Space`.
//! This module computes the text fragment before the cursor and turns the
//! symbol index into ranked candidates.

use crate::symbols::{self, CompletionCandidate, SymbolIndex};

/// The context in which completion was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    /// Identifier fragment immediately before the cursor.
    pub prefix: String,
    /// Text before the cursor on the current line (used for member access).
    pub line_prefix: String,
    /// Whether the trigger was an explicit request (`Ctrl+Space`).
    pub explicit: bool,
}

/// Extracts the completion context from a buffer and a cursor offset.
///
/// `offset` is a byte offset into `text`.
#[must_use]
pub fn context_at(text: &str, offset: usize, explicit: bool) -> CompletionContext {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line_prefix = &before[line_start..];

    let prefix: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    CompletionContext {
        prefix,
        line_prefix: line_prefix.to_string(),
        explicit,
    }
}

/// Computes completion candidates for the given context.
#[must_use]
pub fn candidates(index: &SymbolIndex, context: &CompletionContext) -> Vec<CompletionCandidate> {
    symbols::complete(index, &context.prefix, &context.line_prefix)
}

/// Whether the popup should be shown automatically at this position.
///
/// Dev-C++ pops up completion after `.` and `->`.
#[must_use]
pub fn should_auto_complete(context: &CompletionContext) -> bool {
    let trimmed = context.line_prefix.trim_end();
    trimmed.ends_with('.') || trimmed.ends_with("->")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extracts_prefix() {
        let text = "int value = myVa";
        let ctx = context_at(text, text.len(), false);
        assert_eq!(ctx.prefix, "myVa");
    }

    #[test]
    fn extracts_line_prefix() {
        let text = "obj->meth\nnext";
        let ctx = context_at(text, 9, false);
        assert_eq!(ctx.line_prefix, "obj->meth");
        assert_eq!(ctx.prefix, "meth");
    }

    #[test]
    fn detects_member_access() {
        let text = "point.";
        let ctx = context_at(text, text.len(), false);
        assert!(should_auto_complete(&ctx));
        assert_eq!(ctx.prefix, "");
    }

    #[test]
    fn no_autocomplete_in_plain_text() {
        let text = "somevar";
        let ctx = context_at(text, text.len(), false);
        assert!(!should_auto_complete(&ctx));
    }
}
