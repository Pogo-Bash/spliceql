//! Error types for the SpliceQL lexer.

use std::fmt;

use crate::token::Span;

/// An error produced during lexing.
///
/// Carries the source span where the error occurred, the offending character
/// (if any), and a human-readable message.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub span: Span,
    pub ch: Option<char>,
    pub message: String,
}

impl LexError {
    pub fn new(span: Span, ch: Option<char>, message: impl Into<String>) -> Self {
        Self {
            span,
            ch,
            message: message.into(),
        }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lex error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for LexError {}
