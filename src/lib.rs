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

pub mod error;
pub mod lexer;
pub mod token;

pub use error::LexError;
pub use lexer::{tokenize, Lexer};
pub use token::{Span, Token, TokenKind};
