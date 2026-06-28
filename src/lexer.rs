//! SpliceQL lexer.
//!
//! Converts a source string into a stream of [`Token`]s.  The lexer is
//! designed for single-pass, zero-copy scanning over the source bytes.
//!
//! Design decisions relevant to Phase 2 (parser):
//!
//! * Keywords are matched **case-insensitively**; identifiers preserve their
//!   original case.  The parser can match on [`TokenKind`] variants directly
//!   without any case normalisation.
//!
//! * `true` / `false` are lexed as [`TokenKind::BoolLit`], not as keyword
//!   tokens.  The parser treats them as literal atoms.
//!
//! * Negative numbers are **not** handled by the lexer: `-5` produces
//!   `Minus, IntLit(5)`.  The parser applies unary minus.
//!
//! * A leading dot followed by a digit is lexed as a float (`FloatLit(0.5)`)
//!   rather than `Dot, IntLit(5)`.
//!
//! * `--` comments are stripped completely; no comment tokens are emitted.
//!
//! * [`TokenKind::Newline`] exists in the enum but is **not emitted** during
//!   lexing.  Newlines are whitespace.  Byte-offset spans are sufficient for
//!   computing line/column when needed.

use crate::error::LexError;
use crate::token::{Span, Token, TokenKind};

/// A streaming lexer for SpliceQL source.
pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer over `source`.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
        }
    }

    /// Produce the next token, or a [`LexError`].
    ///
    /// Returns [`TokenKind::Eof`] when the source is exhausted.  Subsequent
    /// calls after `Eof` keep returning `Eof`.
    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments();

        if self.pos >= self.bytes.len() {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span::new(self.pos, self.pos),
            });
        }

        let start = self.pos;
        let b = self.bytes[self.pos];

        match b {
            b'"' => self.lex_string(start),

            b'0'..=b'9' => self.lex_number(start),

            // Leading-dot float: `.5` → FloatLit(0.5)
            b'.' if matches!(self.peek_at(1), Some(b'0'..=b'9')) => self.lex_number(start),

            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_word(start),

            // `$name` template variable.
            b'$' => self.lex_var(start),

            // ── Operators ──
            b'=' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Eq, start))
            }
            b'!' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    Ok(self.tok(TokenKind::NotEq, start))
                } else {
                    Err(LexError::new(
                        Span::new(start, self.pos),
                        Some('!'),
                        "expected '=' after '!'",
                    ))
                }
            }
            b'<' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    Ok(self.tok(TokenKind::LtEq, start))
                } else {
                    Ok(self.tok(TokenKind::Lt, start))
                }
            }
            b'>' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    Ok(self.tok(TokenKind::GtEq, start))
                } else {
                    Ok(self.tok(TokenKind::Gt, start))
                }
            }
            b'+' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Plus, start))
            }
            b'-' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Minus, start))
            }
            b'*' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Star, start))
            }
            b'/' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Slash, start))
            }

            // ── Punctuation ──
            b'.' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Dot, start))
            }
            b',' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Comma, start))
            }
            b';' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Semicolon, start))
            }
            b':' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::Colon, start))
            }
            b'(' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::LParen, start))
            }
            b')' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::RParen, start))
            }
            b'[' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::LBracket, start))
            }
            b']' => {
                self.pos += 1;
                Ok(self.tok(TokenKind::RBracket, start))
            }

            _ => {
                let ch = self.decode_char();
                Err(LexError::new(
                    Span::new(start, self.pos),
                    Some(ch),
                    format!("unexpected character '{ch}'"),
                ))
            }
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    #[inline]
    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    /// Build a [`Token`] spanning `start..self.pos`.
    #[inline]
    fn tok(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            span: Span::new(start, self.pos),
        }
    }

    /// Decode the UTF-8 character at `self.pos` and advance past it.
    fn decode_char(&mut self) -> char {
        let ch = self.source[self.pos..].chars().next().unwrap();
        self.pos += ch.len_utf8();
        ch
    }

    // ── Whitespace / comments ───────────────────────────────────────────────

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace.
            while let Some(b) = self.peek() {
                if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            // Skip `-- …` single-line comments.
            if self.peek() == Some(b'-') && self.peek_at(1) == Some(b'-') {
                while let Some(b) = self.peek() {
                    self.pos += 1;
                    if b == b'\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    // ── String literals ─────────────────────────────────────────────────────

    fn lex_string(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1; // opening "
        let mut value = String::new();

        loop {
            if self.pos >= self.bytes.len() {
                return Err(LexError::new(
                    Span::new(start, self.pos),
                    None,
                    "unterminated string literal",
                ));
            }

            let b = self.bytes[self.pos];

            if b == b'"' {
                self.pos += 1;
                return Ok(Token {
                    kind: TokenKind::StringLit(value),
                    span: Span::new(start, self.pos),
                });
            }

            if b == b'\\' {
                self.pos += 1;
                if self.pos >= self.bytes.len() {
                    return Err(LexError::new(
                        Span::new(start, self.pos),
                        None,
                        "unterminated string literal",
                    ));
                }
                match self.bytes[self.pos] {
                    b'n' => {
                        self.pos += 1;
                        value.push('\n');
                    }
                    b't' => {
                        self.pos += 1;
                        value.push('\t');
                    }
                    b'r' => {
                        self.pos += 1;
                        value.push('\r');
                    }
                    b'\\' => {
                        self.pos += 1;
                        value.push('\\');
                    }
                    b'"' => {
                        self.pos += 1;
                        value.push('"');
                    }
                    b'0' => {
                        self.pos += 1;
                        value.push('\0');
                    }
                    _ => {
                        let esc_start = self.pos - 1;
                        let ch = self.decode_char();
                        return Err(LexError::new(
                            Span::new(esc_start, self.pos),
                            Some(ch),
                            format!("invalid escape sequence '\\{ch}'"),
                        ));
                    }
                }
                continue;
            }

            // Regular character (handle multi-byte UTF-8).
            if b < 0x80 {
                self.pos += 1;
                value.push(b as char);
            } else {
                let ch = self.source[self.pos..].chars().next().unwrap();
                self.pos += ch.len_utf8();
                value.push(ch);
            }
        }
    }

    // ── Template variables ──────────────────────────────────────────────────

    /// Lex `$[a-zA-Z_][a-zA-Z0-9_]*` into a [`TokenKind::Var`] whose payload is
    /// the name **without** the leading `$`.
    fn lex_var(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1; // consume `$`
        let name_start = self.pos;
        // First character must be a letter or underscore.
        match self.peek() {
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'_') => {}
            _ => {
                return Err(LexError::new(
                    Span::new(start, self.pos),
                    Some('$'),
                    "expected a variable name after '$'",
                ));
            }
        }
        while matches!(
            self.peek(),
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')
        ) {
            self.pos += 1;
        }
        let name = self.source[name_start..self.pos].to_string();
        Ok(Token {
            kind: TokenKind::Var(name),
            span: Span::new(start, self.pos),
        })
    }

    // ── Number literals ─────────────────────────────────────────────────────

    fn lex_number(&mut self, start: usize) -> Result<Token, LexError> {
        let mut is_float = false;

        if self.peek() == Some(b'.') {
            // Leading-dot float: `.5`
            is_float = true;
            self.pos += 1;
            self.eat_digits();
        } else {
            // Integer part.
            self.eat_digits();
            // Fractional part — only if `.` is followed by a digit.
            if self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(b'0'..=b'9')) {
                is_float = true;
                self.pos += 1;
                self.eat_digits();
            }
        }

        // Exponent part: `e5`, `E-3`, `e+10`.
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            let after_e = self.peek_at(1);
            let has_exp = matches!(after_e, Some(b'0'..=b'9'))
                || (matches!(after_e, Some(b'+') | Some(b'-'))
                    && matches!(self.peek_at(2), Some(b'0'..=b'9')));
            if has_exp {
                is_float = true;
                self.pos += 1; // e / E
                if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                    self.pos += 1;
                }
                self.eat_digits();
            }
        }

        let text = &self.source[start..self.pos];

        if is_float {
            let v: f64 = text.parse().map_err(|_| {
                LexError::new(Span::new(start, self.pos), None, format!("invalid float literal '{text}'"))
            })?;
            Ok(Token {
                kind: TokenKind::FloatLit(v),
                span: Span::new(start, self.pos),
            })
        } else {
            let v: i64 = text.parse().map_err(|_| {
                LexError::new(Span::new(start, self.pos), None, format!("invalid integer literal '{text}'"))
            })?;
            Ok(Token {
                kind: TokenKind::IntLit(v),
                span: Span::new(start, self.pos),
            })
        }
    }

    fn eat_digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    // ── Identifiers / keywords ──────────────────────────────────────────────

    fn lex_word(&mut self, start: usize) -> Result<Token, LexError> {
        while matches!(
            self.peek(),
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_')
        ) {
            self.pos += 1;
        }

        let text = &self.source[start..self.pos];
        let span = Span::new(start, self.pos);

        // Case-insensitive keyword match.
        let kind = match text.len() {
            2 => match_kw_2(text),
            3 => match_kw_3(text),
            4 => match_kw_4(text),
            5 => match_kw_5(text),
            6 => match_kw_6(text),
            8 => match_kw_8(text),
            _ => None,
        };

        Ok(Token {
            kind: kind.unwrap_or_else(|| TokenKind::Ident(text.to_string())),
            span,
        })
    }
}

// ── Case-insensitive keyword tables, grouped by length ──────────────────────
//
// Using length-bucketed matches avoids a heap-allocating `to_ascii_uppercase()`
// on every identifier.  Each function folds the input to uppercase byte-by-byte
// and compares against the known keywords of that length.

fn upper(b: u8) -> u8 {
    if b.is_ascii_lowercase() {
        b - 32
    } else {
        b
    }
}

fn eq_ci(text: &str, keyword: &[u8]) -> bool {
    let t = text.as_bytes();
    if t.len() != keyword.len() {
        return false;
    }
    for i in 0..t.len() {
        if upper(t[i]) != keyword[i] {
            return false;
        }
    }
    true
}

fn match_kw_2(t: &str) -> Option<TokenKind> {
    if eq_ci(t, b"OR") {
        Some(TokenKind::Or)
    } else if eq_ci(t, b"AS") {
        Some(TokenKind::As)
    } else if eq_ci(t, b"BY") {
        Some(TokenKind::By)
    } else {
        None
    }
}

fn match_kw_3(t: &str) -> Option<TokenKind> {
    if eq_ci(t, b"AND") {
        Some(TokenKind::And)
    } else if eq_ci(t, b"NOT") {
        Some(TokenKind::Not)
    } else if eq_ci(t, b"ASC") {
        Some(TokenKind::Asc)
    } else if eq_ci(t, b"BAM") {
        Some(TokenKind::Bam)
    } else if eq_ci(t, b"VCF") {
        Some(TokenKind::Vcf)
    } else if eq_ci(t, b"BED") {
        Some(TokenKind::Bed)
    } else if eq_ci(t, b"CNV") {
        Some(TokenKind::Cnv)
    } else if eq_ci(t, b"TSV") {
        Some(TokenKind::Tsv)
    } else {
        None
    }
}

fn match_kw_4(t: &str) -> Option<TokenKind> {
    if eq_ci(t, b"FROM") {
        Some(TokenKind::From)
    } else if eq_ci(t, b"CALL") {
        Some(TokenKind::Call)
    } else if eq_ci(t, b"WITH") {
        Some(TokenKind::With)
    } else if eq_ci(t, b"INTO") {
        Some(TokenKind::Into)
    } else if eq_ci(t, b"DESC") {
        Some(TokenKind::Desc)
    } else if eq_ci(t, b"CRAM") {
        Some(TokenKind::Cram)
    } else if eq_ci(t, b"JSON") {
        Some(TokenKind::Json)
    } else if eq_ci(t, b"ISEC") {
        Some(TokenKind::Isec)
    } else if eq_ci(t, b"MODE") {
        Some(TokenKind::Mode)
    } else if eq_ci(t, b"TRUE") {
        Some(TokenKind::BoolLit(true))
    } else {
        None
    }
}

fn match_kw_5(t: &str) -> Option<TokenKind> {
    if eq_ci(t, b"WHERE") {
        Some(TokenKind::Where)
    } else if eq_ci(t, b"LIMIT") {
        Some(TokenKind::Limit)
    } else if eq_ci(t, b"ORDER") {
        Some(TokenKind::Order)
    } else if eq_ci(t, b"FASTA") {
        Some(TokenKind::Fasta)
    } else if eq_ci(t, b"READS") {
        Some(TokenKind::Reads)
    } else if eq_ci(t, b"FALSE") {
        Some(TokenKind::BoolLit(false))
    } else {
        None
    }
}

fn match_kw_6(t: &str) -> Option<TokenKind> {
    if eq_ci(t, b"SELECT") {
        Some(TokenKind::Select)
    } else if eq_ci(t, b"FILTER") {
        Some(TokenKind::Filter)
    } else if eq_ci(t, b"HEADER") {
        Some(TokenKind::Header)
    } else if eq_ci(t, b"PAIRED") {
        Some(TokenKind::Paired)
    } else {
        None
    }
}

fn match_kw_8(t: &str) -> Option<TokenKind> {
    if eq_ci(t, b"VARIANTS") {
        Some(TokenKind::Variants)
    } else if eq_ci(t, b"COVERAGE") {
        Some(TokenKind::Coverage)
    } else if eq_ci(t, b"ANNOTATE") {
        Some(TokenKind::Annotate)
    } else {
        None
    }
}

// ── Convenience function ────────────────────────────────────────────────────

/// Tokenize an entire source string, returning all tokens including the
/// trailing [`TokenKind::Eof`].
pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token()?;
        let done = tok.kind == TokenKind::Eof;
        tokens.push(tok);
        if done {
            break;
        }
    }
    Ok(tokens)
}
