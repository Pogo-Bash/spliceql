//! **spliceql** — the SpliceQL query language frontend for CodonSplice.
//!
//! This crate is the language layer: lexer, parser, AST, and bytecode
//! compiler.  It contains **no** WASM-specific code; the wasm-bindgen shim
//! lives in the `codonsplice` engine crate.
//!
//! # Phase 1 — Lexer
//!
//! ```
//! use spliceql::{tokenize, TokenKind};
//!
//! let tokens = tokenize("FROM bam \"sample.bam\" WHERE depth > 30").unwrap();
//! assert_eq!(tokens[0].kind, TokenKind::From);
//! assert_eq!(tokens[1].kind, TokenKind::Bam);
//! ```

pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod token;

pub use error::{LexError, ParseError};
pub use lexer::{tokenize, Lexer};
pub use parser::Parser;
pub use token::{Span, Token, TokenKind};

/// Lex and parse `source` into an [`ast::Query`] in one call.
///
/// This is the primary entry point for downstream crates (e.g. `codonsplice`):
/// it tokenizes the source and runs the parser, mapping any lexer failure into
/// a [`ParseError`] so the whole pipeline surfaces a single error type.
///
/// # Examples
///
/// ```
/// let query = spliceql::parse(r#"FROM bam "sample.bam" WHERE depth > 30 CALL variants"#)
///     .unwrap();
/// assert_eq!(query.from.format, spliceql::ast::Format::Bam);
/// ```
pub fn parse(source: &str) -> Result<ast::Query, error::ParseError> {
    tokenize(source)
        .map_err(Into::into)
        .and_then(|tokens| Parser::new(tokens, source).parse())
}
