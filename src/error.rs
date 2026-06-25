//! Error types for the SpliceQL lexer and parser.

use std::fmt;

use crate::token::{Span, TokenKind};

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

// ── ParseError ───────────────────────────────────────────────────────────────

/// An error produced during parsing.
///
/// Every variant carries the [`Span`] of the offending token so callers can map
/// it back to a line/column with [`byte_offset_to_line_col`].  Lexer failures
/// surface through [`ParseError::LexError`] via the [`From`] impl, so a single
/// `Result<_, ParseError>` covers the whole lex-then-parse pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// A token was found where a different one was required.
    UnexpectedToken {
        expected: String,
        got: TokenKind,
        span: Span,
    },
    /// The token stream ended while more input was required.
    UnexpectedEof { expected: String, span: Span },
    /// A comparison operator was chained, e.g. `a < b < c`.  The span points at
    /// the *second* comparison operator.
    ChainedComparison { span: Span },
    /// A `FROM`/`INTO` format token was not one of the known formats.
    InvalidFormat { got: String, span: Span },
    /// A `CALL` operation name was not one of the known operations.
    InvalidCallOp { got: String, span: Span },
    /// The lexer failed before the parser ever saw a token.
    LexError(LexError),
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError::LexError(e)
    }
}

impl ParseError {
    /// The source span this error refers to.
    pub fn span(&self) -> Span {
        match self {
            ParseError::UnexpectedToken { span, .. }
            | ParseError::UnexpectedEof { span, .. }
            | ParseError::ChainedComparison { span }
            | ParseError::InvalidFormat { span, .. }
            | ParseError::InvalidCallOp { span, .. } => *span,
            ParseError::LexError(e) => e.span,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedToken {
                expected,
                got,
                span,
            } => write!(
                f,
                "parse error at {}: expected {expected}, found `{got}`",
                fmt_span(*span),
            ),
            ParseError::UnexpectedEof { expected, span } => write!(
                f,
                "parse error at {}: expected {expected}, but reached end of input",
                fmt_span(*span),
            ),
            ParseError::ChainedComparison { span } => write!(
                f,
                "parse error at {}: comparison operators cannot be chained; \
                 use parentheses or AND",
                fmt_span(*span),
            ),
            ParseError::InvalidFormat { got, span } => write!(
                f,
                "parse error at {}: `{got}` is not a valid format \
                 (expected one of BAM, VCF, FASTA, BED, CRAM)",
                fmt_span(*span),
            ),
            ParseError::InvalidCallOp { got, span } => write!(
                f,
                "parse error at {}: `{got}` is not a valid CALL operation \
                 (expected one of variants, cnv, coverage, reads, header)",
                fmt_span(*span),
            ),
            ParseError::LexError(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::LexError(e) => Some(e),
            _ => None,
        }
    }
}

/// Render a span as `start..end` for error messages.
///
/// Spans are byte offsets; callers that have the original source can upgrade
/// these to `line:col` with [`byte_offset_to_line_col`].
fn fmt_span(span: Span) -> String {
    format!("{}..{}", span.start, span.end)
}

/// Convert a byte offset into a 1-based `(line, column)` pair.
///
/// Lines and columns are counted in Unicode scalar values, not bytes, so a
/// multi-byte character advances the column by one.  An `offset` past the end
/// of `source` is clamped to the final position rather than panicking — this
/// keeps error formatting total even for end-of-input spans.
pub fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    let mut seen = 0usize;

    for ch in source.chars() {
        if seen >= offset {
            break;
        }
        seen += ch.len_utf8();
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}
