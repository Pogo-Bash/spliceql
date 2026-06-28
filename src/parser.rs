//! SpliceQL parser.
//!
//! A hand-written recursive-descent parser for clauses, with a [Pratt][pratt]
//! (precedence-climbing) sub-parser for expressions.  It consumes the
//! [`Token`] stream produced by the lexer and yields an [`ast::Query`].
//!
//! [pratt]: https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html
//!
//! # Grammar overview
//!
//! ```text
//! query   := FROM_clause clause*
//! clause  := SELECT_clause | WHERE_clause | CALL_clause | WITH_clause
//!          | INTO_clause | ORDER_clause | LIMIT_clause
//! ```
//!
//! `FROM` is required and must be first.  Every other clause is optional and
//! order-independent (see the module-level notes on the soft `SELECT`-before-
//! `WHERE` rule).
//!
//! # Binding powers
//!
//! | operator                | left | right | notes              |
//! |-------------------------|------|-------|--------------------|
//! | `OR`                    | 1    | 2     |                    |
//! | `AND`                   | 3    | 4     |                    |
//! | `NOT` (prefix)          | —    | 5     |                    |
//! | `= != < > <= >=`        | 7    | 8     | non-associative    |
//! | `+ -` (infix)           | 9    | 10    |                    |
//! | `* /`                   | 11   | 12    |                    |
//! | `-` (unary)             | —    | 13    |                    |
//! | `.` (field access)      | 15   | 16    |                    |
//! | `(` `[` (postfix)       | 17   | —     | call / subscript   |

use crate::ast::*;
use crate::error::ParseError;
use crate::token::{Span, Token, TokenKind};

/// A recursive-descent + Pratt parser over a token stream.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Original source, retained for error rendering and for recovering the
    /// spelling of genomic keywords used as identifiers (see `keyword_text`).
    source: String,
}

impl Parser {
    /// Build a parser over `tokens`.
    ///
    /// The token vector is normalised to always end in [`TokenKind::Eof`], so
    /// `peek`/`advance` are total even if the caller hands over an empty or
    /// `Eof`-less slice.  `tokenize` already guarantees a trailing `Eof`; this
    /// just makes the public constructor robust to hand-built input (and fuzz
    /// harnesses).
    pub fn new(tokens: Vec<Token>, source: impl Into<String>) -> Self {
        let mut tokens = tokens;
        let needs_eof = tokens.last().map(|t| t.kind != TokenKind::Eof).unwrap_or(true);
        if needs_eof {
            let at = tokens.last().map(|t| t.span.end).unwrap_or(0);
            tokens.push(Token {
                kind: TokenKind::Eof,
                span: Span::new(at, at),
            });
        }
        Self {
            tokens,
            pos: 0,
            source: source.into(),
        }
    }

    /// Parse the token stream into a [`Query`].
    pub fn parse(&mut self) -> Result<Query, ParseError> {
        self.parse_query()
    }

    // ── Cursor primitives ────────────────────────────────────────────────────

    /// The current token without consuming it.
    fn peek(&self) -> &Token {
        // `self.pos` is always in-bounds: `new` guarantees a trailing `Eof` and
        // `advance` never steps past it.
        &self.tokens[self.pos]
    }

    /// Consume and return the current token.  Stops at `Eof` (repeated calls
    /// keep returning `Eof`).
    fn advance(&mut self) -> &Token {
        let i = self.pos;
        if self.tokens[self.pos].kind != TokenKind::Eof {
            self.pos += 1;
        }
        &self.tokens[i]
    }

    /// `true` if the current token's kind equals `kind`.
    ///
    /// Intended for data-less kinds (keywords, punctuation); calling it with a
    /// payload-carrying kind compares payloads too, which is rarely what you
    /// want — match manually in those cases.
    fn at(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    /// Consume a token of exactly `kind`, or produce a precise error.
    fn expect(&mut self, kind: TokenKind) -> Result<&Token, ParseError> {
        if self.peek().kind == kind {
            Ok(self.advance())
        } else {
            let span = self.peek().span;
            let expected = format!("`{kind}`");
            if self.peek().kind == TokenKind::Eof {
                Err(ParseError::UnexpectedEof { expected, span })
            } else {
                Err(ParseError::UnexpectedToken {
                    expected,
                    got: self.peek().kind.clone(),
                    span,
                })
            }
        }
    }

    /// End byte-offset of the most recently consumed token (0 before any).
    fn prev_end(&self) -> usize {
        if self.pos == 0 {
            0
        } else {
            self.tokens[self.pos - 1].span.end
        }
    }

    /// Consume an identifier token, returning its name and span.
    ///
    /// Genomic keywords (`reads`, `coverage`, `variants`, format names, …) are
    /// **contextual**: in a name position they are accepted as identifiers, with
    /// their original spelling recovered from the source.  This lets field and
    /// column names like `reads.depth` or `SELECT coverage` work even though the
    /// lexer classifies those words as keywords.
    fn expect_ident(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        let span = self.peek().span;
        match &self.peek().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok((name, span))
            }
            k if Self::is_genomic_keyword(k) => {
                let name = self.keyword_text(span);
                self.advance();
                Ok((name, span))
            }
            TokenKind::Eof => Err(ParseError::UnexpectedEof {
                expected: what.to_string(),
                span,
            }),
            other => {
                let got = other.clone();
                Err(ParseError::UnexpectedToken {
                    expected: what.to_string(),
                    got,
                    span,
                })
            }
        }
    }

    /// The original source text covered by `span` (the as-written spelling of a
    /// contextual keyword).  Spans always lie on UTF-8 boundaries, so slicing is
    /// safe; an out-of-range span (only possible from hand-built tokens) falls
    /// back to the empty string rather than panicking.
    fn keyword_text(&self, span: Span) -> String {
        self.source
            .get(span.start..span.end)
            .unwrap_or("")
            .to_string()
    }

    /// `true` for keywords that double as contextual identifiers (field/column
    /// names) when they appear in an expression position. Genomic keywords
    /// (`reads.depth`, `SELECT coverage`) plus `FILTER` — which is both a `WHERE`
    /// synonym at clause start and the VCF `filter` column name in expressions.
    fn is_genomic_keyword(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Bam
                | TokenKind::Vcf
                | TokenKind::Fasta
                | TokenKind::Bed
                | TokenKind::Cram
                | TokenKind::Json
                | TokenKind::Tsv
                | TokenKind::Variants
                | TokenKind::Cnv
                | TokenKind::Coverage
                | TokenKind::Reads
                | TokenKind::Header
                | TokenKind::Filter
        )
    }

    /// Consume a string-literal token, returning its value and span.
    /// Retained as a parsing primitive (paths now go through `expect_path`).
    #[allow(dead_code)]
    fn expect_string(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        let span = self.peek().span;
        match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok((s, span))
            }
            TokenKind::Eof => Err(ParseError::UnexpectedEof {
                expected: what.to_string(),
                span,
            }),
            other => {
                let got = other.clone();
                Err(ParseError::UnexpectedToken {
                    expected: what.to_string(),
                    got,
                    span,
                })
            }
        }
    }

    /// Consume a path: either a string literal (`"sample.bam"`) or a `$var`
    /// (`$bam`). A variable path is stored verbatim as `"$name"`; the compiler
    /// interns it and the VM resolves the `$`-prefix against the VarMap at
    /// `OPEN_SOURCE` / `WRITE_INTO` time. This lets `.spq` scripts parameterize
    /// their inputs and outputs.
    fn expect_path(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        let span = self.peek().span;
        match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok((s, span))
            }
            TokenKind::Var(name) => {
                let path = format!("${name}");
                self.advance();
                Ok((path, span))
            }
            TokenKind::Eof => Err(ParseError::UnexpectedEof {
                expected: what.to_string(),
                span,
            }),
            other => {
                let got = other.clone();
                Err(ParseError::UnexpectedToken {
                    expected: what.to_string(),
                    got,
                    span,
                })
            }
        }
    }

    // ── Query / clauses ──────────────────────────────────────────────────────

    fn parse_query(&mut self) -> Result<Query, ParseError> {
        let from = self.parse_from()?;
        let start = from.span.start;

        let mut select = None;
        let mut filter = None;
        let mut call = None;
        let mut with = None;
        let mut annotate = None;
        let mut into = None;
        let mut order = None;
        let mut limit = None;

        loop {
            // Clone the kind so the match body can borrow `self` mutably.
            let kind = self.peek().kind.clone();
            match kind {
                TokenKind::Select => select = Some(self.parse_select()?),
                TokenKind::Where | TokenKind::Filter => filter = Some(self.parse_where()?),
                TokenKind::Call => call = Some(self.parse_call()?),
                TokenKind::With => with = Some(self.parse_with()?),
                TokenKind::Annotate => annotate = Some(self.parse_annotate()?),
                TokenKind::Into => into = Some(self.parse_into()?),
                TokenKind::Order => order = Some(self.parse_order()?),
                TokenKind::Limit => limit = Some(self.parse_limit()?),
                // A bare `;` terminates the query; tolerate a trailing one.
                TokenKind::Semicolon => {
                    self.advance();
                    continue;
                }
                TokenKind::Eof => break,
                other => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "a clause keyword (SELECT, WHERE, CALL, WITH, ANNOTATE, INTO, ORDER, LIMIT)"
                            .to_string(),
                        got: other,
                        span: self.peek().span,
                    });
                }
            }
        }

        let end = self.prev_end();
        Ok(Query {
            from,
            select,
            filter,
            call,
            with,
            annotate,
            into,
            order,
            limit,
            span: Span::new(start, end),
        })
    }

    fn parse_from(&mut self) -> Result<FromClause, ParseError> {
        let start = self.peek().span.start;
        self.expect(TokenKind::From)?;

        let fmt_span = self.peek().span;
        let fmt_kind = self.peek().kind.clone();
        let format = match Self::format_from_kind(&fmt_kind) {
            Some(f) => {
                self.advance();
                f
            }
            None if fmt_kind == TokenKind::Eof => {
                return Err(ParseError::UnexpectedEof {
                    expected: "a file format (BAM, VCF, FASTA, BED, CRAM)".to_string(),
                    span: fmt_span,
                });
            }
            None => {
                return Err(ParseError::InvalidFormat {
                    got: format!("{fmt_kind}"),
                    span: fmt_span,
                });
            }
        };

        let (path, _) = self.expect_path("a file path string or $variable")?;

        let alias = if self.at(TokenKind::As) {
            self.advance();
            let (name, _) = self.expect_ident("an alias name")?;
            Some(name)
        } else {
            None
        };

        // Optional VCF set-operation join. Two surfaces lower to the same
        // `IsecClause` (and the same set-op engine):
        //   * `ISEC <format> "<path>" [MODE <mode>]`           (general)
        //   * `PAIRED WITH <format> "<path>" [MODE <mode>]`    (tumor/normal)
        let isec = if self.at(TokenKind::Isec) {
            Some(self.parse_isec()?)
        } else if self.at(TokenKind::Paired) {
            Some(self.parse_paired()?)
        } else {
            None
        };

        let end = self.prev_end();
        Ok(FromClause {
            format,
            path,
            alias,
            isec,
            span: Span::new(start, end),
        })
    }

    /// Parse `ISEC <format> "<path>" [MODE <mode>]` (the current token is `ISEC`).
    fn parse_isec(&mut self) -> Result<IsecClause, ParseError> {
        let start = self.peek().span.start;
        self.expect(TokenKind::Isec)?;

        let fmt_span = self.peek().span;
        let fmt_kind = self.peek().kind.clone();
        let format = match Self::format_from_kind(&fmt_kind) {
            Some(f) => {
                self.advance();
                f
            }
            None if fmt_kind == TokenKind::Eof => {
                return Err(ParseError::UnexpectedEof {
                    expected: "a file format (BAM, VCF, FASTA, BED, CRAM)".to_string(),
                    span: fmt_span,
                });
            }
            None => {
                return Err(ParseError::InvalidFormat {
                    got: format!("{fmt_kind}"),
                    span: fmt_span,
                });
            }
        };

        let (path, _) = self.expect_path("a file path string or $variable")?;

        // Optional `MODE <ident>`; defaults to `shared` (the intersection).
        let mode = if self.at(TokenKind::Mode) {
            self.advance();
            let (name, name_span) = self.expect_ident("a set-operation mode")?;
            match name.to_ascii_lowercase().as_str() {
                "private_a" | "privatea" => IsecMode::PrivateA,
                "private_b" | "privateb" => IsecMode::PrivateB,
                "shared" | "intersect" => IsecMode::Shared,
                "shared_b" | "sharedb" => IsecMode::SharedB,
                "union" => IsecMode::Union,
                other => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "one of: shared, shared_b, private_a, private_b, union"
                            .to_string(),
                        got: TokenKind::Ident(other.to_string()),
                        span: name_span,
                    });
                }
            }
        } else {
            IsecMode::Shared
        };

        let end = self.prev_end();
        Ok(IsecClause {
            format,
            path,
            mode,
            span: Span::new(start, end),
        })
    }

    /// Parse `PAIRED WITH <format> "<path>" [MODE somatic|germline]` (the
    /// current token is `PAIRED`). This is tumor/normal somatic calling
    /// expressed as a VCF set operation: the first source is the TUMOR (`A`),
    /// the paired source is the NORMAL (`B`). It lowers to the *same*
    /// [`IsecClause`] / set-op engine as `ISEC`:
    ///   * `MODE somatic` (the default) → [`IsecMode::PrivateA`] — variants
    ///     present in the tumor but absent from the normal.
    ///   * `MODE germline`              → [`IsecMode::Shared`] — variants the
    ///     tumor shares with the normal.
    fn parse_paired(&mut self) -> Result<IsecClause, ParseError> {
        let start = self.peek().span.start;
        self.expect(TokenKind::Paired)?;
        self.expect(TokenKind::With)?;

        let fmt_span = self.peek().span;
        let fmt_kind = self.peek().kind.clone();
        let format = match Self::format_from_kind(&fmt_kind) {
            Some(f) => {
                self.advance();
                f
            }
            None if fmt_kind == TokenKind::Eof => {
                return Err(ParseError::UnexpectedEof {
                    expected: "a file format (BAM, VCF, FASTA, BED, CRAM)".to_string(),
                    span: fmt_span,
                });
            }
            None => {
                return Err(ParseError::InvalidFormat {
                    got: format!("{fmt_kind}"),
                    span: fmt_span,
                });
            }
        };

        let (path, _) = self.expect_path("a file path string or $variable")?;

        // Optional `MODE somatic|germline`; defaults to `somatic` (tumor-private).
        let mode = if self.at(TokenKind::Mode) {
            self.advance();
            let (name, name_span) = self.expect_ident("a somatic mode")?;
            match name.to_ascii_lowercase().as_str() {
                "somatic" | "tumor_only" | "tumoronly" => IsecMode::PrivateA,
                "germline" | "shared" => IsecMode::Shared,
                other => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "one of: somatic, germline".to_string(),
                        got: TokenKind::Ident(other.to_string()),
                        span: name_span,
                    });
                }
            }
        } else {
            IsecMode::PrivateA
        };

        let end = self.prev_end();
        Ok(IsecClause {
            format,
            path,
            mode,
            span: Span::new(start, end),
        })
    }

    fn parse_into(&mut self) -> Result<IntoClause, ParseError> {
        let start = self.peek().span.start;
        self.expect(TokenKind::Into)?;

        let fmt_span = self.peek().span;
        let fmt_kind = self.peek().kind.clone();
        let format = match Self::format_from_kind(&fmt_kind) {
            Some(f) => {
                self.advance();
                f
            }
            None if fmt_kind == TokenKind::Eof => {
                return Err(ParseError::UnexpectedEof {
                    expected: "a file format (BAM, VCF, FASTA, BED, CRAM)".to_string(),
                    span: fmt_span,
                });
            }
            None => {
                return Err(ParseError::InvalidFormat {
                    got: format!("{fmt_kind}"),
                    span: fmt_span,
                });
            }
        };

        let (path, _) = self.expect_path("a file path string or $variable")?;
        let end = self.prev_end();
        Ok(IntoClause {
            format,
            path,
            span: Span::new(start, end),
        })
    }

    fn parse_call(&mut self) -> Result<CallClause, ParseError> {
        let start = self.peek().span.start;
        self.expect(TokenKind::Call)?;

        let op_span = self.peek().span;
        let op_kind = self.peek().kind.clone();
        let operation = match op_kind {
            TokenKind::Variants => "variants",
            TokenKind::Cnv => "cnv",
            TokenKind::Coverage => "coverage",
            TokenKind::Reads => "reads",
            TokenKind::Header => "header",
            TokenKind::Eof => {
                return Err(ParseError::UnexpectedEof {
                    expected: "a CALL operation (variants, cnv, coverage, reads, header)"
                        .to_string(),
                    span: op_span,
                });
            }
            other => {
                return Err(ParseError::InvalidCallOp {
                    got: format!("{other}"),
                    span: op_span,
                });
            }
        };
        self.advance();

        let end = self.prev_end();
        Ok(CallClause {
            operation: operation.to_string(),
            span: Span::new(start, end),
        })
    }

    fn parse_select(&mut self) -> Result<Vec<SelectItem>, ParseError> {
        self.expect(TokenKind::Select)?;
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr(0)?;
            let item_start = expr.span().start;
            let alias = if self.at(TokenKind::As) {
                self.advance();
                let (name, _) = self.expect_ident("an alias name")?;
                Some(name)
            } else {
                None
            };
            let item_end = self.prev_end();
            items.push(SelectItem {
                expr,
                alias,
                span: Span::new(item_start, item_end),
            });
            if self.at(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(items)
    }

    fn parse_where(&mut self) -> Result<Expr, ParseError> {
        // Caller guarantees the current token is WHERE or FILTER.
        self.advance();
        self.parse_expr(0)
    }

    fn parse_with(&mut self) -> Result<Vec<(String, Expr)>, ParseError> {
        self.expect(TokenKind::With)?;
        let mut pairs = Vec::new();
        loop {
            let (key, _) = self.expect_ident("a parameter name")?;
            self.expect(TokenKind::Eq)?;
            let value = self.parse_expr(0)?;
            pairs.push((key, value));
            if self.at(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(pairs)
    }

    /// Parse `ANNOTATE WITH key = "path", key = "path", ...`.
    ///
    /// Shares the `key = value` pair shape with [`Self::parse_with`], but the
    /// values are annotation-database paths (string literals or `$vars`) rather
    /// than CALL-tuning scalars, and the pairs hang off a dedicated clause.
    fn parse_annotate(&mut self) -> Result<AnnotateClause, ParseError> {
        let start = self.peek().span.start;
        self.expect(TokenKind::Annotate)?;
        self.expect(TokenKind::With)?;
        let mut params = Vec::new();
        loop {
            let (key, _) = self.expect_ident("an annotation database name")?;
            self.expect(TokenKind::Eq)?;
            let value = self.parse_expr(0)?;
            params.push((key, value));
            if self.at(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let end = self.prev_end();
        Ok(AnnotateClause {
            params,
            span: Span::new(start, end),
        })
    }

    fn parse_order(&mut self) -> Result<Vec<OrderItem>, ParseError> {
        self.expect(TokenKind::Order)?;
        self.expect(TokenKind::By)?;
        let mut items = Vec::new();
        loop {
            let expr = self.parse_expr(0)?;
            let item_start = expr.span().start;
            let direction = if self.at(TokenKind::Asc) {
                self.advance();
                Direction::Asc
            } else if self.at(TokenKind::Desc) {
                self.advance();
                Direction::Desc
            } else {
                Direction::Asc
            };
            let item_end = self.prev_end();
            items.push(OrderItem {
                expr,
                direction,
                span: Span::new(item_start, item_end),
            });
            if self.at(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(items)
    }

    fn parse_limit(&mut self) -> Result<Expr, ParseError> {
        self.expect(TokenKind::Limit)?;
        self.parse_expr(0)
    }

    // ── Pratt expression parser ──────────────────────────────────────────────

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_prefix()?;

        loop {
            let kind = self.peek().kind.clone();

            // Postfix operators: call `(` and subscript `[`, both at bp 17.
            match kind {
                TokenKind::LParen => {
                    if Self::POSTFIX_BP < min_bp {
                        break;
                    }
                    lhs = self.parse_call_args(lhs)?;
                    continue;
                }
                TokenKind::LBracket => {
                    if Self::POSTFIX_BP < min_bp {
                        break;
                    }
                    lhs = self.parse_subscript(lhs)?;
                    continue;
                }
                _ => {}
            }

            let (l_bp, r_bp) = match Self::infix_bp(&kind) {
                Some(bp) => bp,
                None => break,
            };
            if l_bp < min_bp {
                break;
            }

            self.advance();

            // Field access: RHS is a bare identifier, not a full expression.
            if kind == TokenKind::Dot {
                let (field, field_span) = self.expect_ident("a field name")?;
                let span = Span::new(lhs.span().start, field_span.end);
                lhs = Expr::FieldAccess {
                    object: Box::new(lhs),
                    field,
                    span,
                };
                continue;
            }

            let rhs = self.parse_expr(r_bp)?;
            let span = Span::new(lhs.span().start, rhs.span().end);
            let op = Self::bin_op(&kind).expect("infix_bp implies a BinOp mapping");

            // Non-associative comparisons: `a < b < c` is rejected.
            let is_cmp = Self::is_comparison(&kind);
            lhs = Expr::Binary {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
                span,
            };
            if is_cmp && Self::is_comparison(&self.peek().kind) {
                return Err(ParseError::ChainedComparison {
                    span: self.peek().span,
                });
            }
        }

        Ok(lhs)
    }

    /// Parse a prefix position (the Pratt "null denotation").
    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        let span = self.peek().span;
        let kind = self.peek().kind.clone();
        match kind {
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Expr::IntLit(n, span))
            }
            TokenKind::FloatLit(v) => {
                self.advance();
                Ok(Expr::FloatLit(v, span))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Expr::StringLit(s, span))
            }
            TokenKind::BoolLit(b) => {
                self.advance();
                Ok(Expr::BoolLit(b, span))
            }
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Expr::Ident(name, span))
            }
            TokenKind::Var(name) => {
                self.advance();
                Ok(Expr::Var(name, span))
            }
            // Genomic keywords act as identifiers in expression position
            // (`reads.depth`, `SELECT coverage`), recovering their spelling.
            ref k if Self::is_genomic_keyword(k) => {
                let name = self.keyword_text(span);
                self.advance();
                Ok(Expr::Ident(name, span))
            }
            // `*` in prefix position is the SELECT wildcard; in infix position
            // (handled above) it is multiplication.
            TokenKind::Star => {
                self.advance();
                Ok(Expr::Wildcard(span))
            }
            TokenKind::Not => {
                let r_bp = Self::prefix_bp(&kind).expect("NOT is a prefix operator");
                self.advance();
                let operand = self.parse_expr(r_bp)?;
                let full = Span::new(span.start, operand.span().end);
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                    span: full,
                })
            }
            TokenKind::Minus => {
                let r_bp = Self::prefix_bp(&kind).expect("unary minus is a prefix operator");
                self.advance();
                let operand = self.parse_expr(r_bp)?;
                let full = Span::new(span.start, operand.span().end);
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                    span: full,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr(0)?;
                self.expect(TokenKind::RParen)?;
                Ok(inner)
            }
            TokenKind::Eof => Err(ParseError::UnexpectedEof {
                expected: "an expression".to_string(),
                span,
            }),
            other => Err(ParseError::UnexpectedToken {
                expected: "an expression".to_string(),
                got: other,
                span,
            }),
        }
    }

    /// Parse `( arg, arg, … )` following `callee`, producing an [`Expr::Call`].
    fn parse_call_args(&mut self, callee: Expr) -> Result<Expr, ParseError> {
        self.expect(TokenKind::LParen)?;
        let mut args = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                args.push(self.parse_expr(0)?);
                if self.at(TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        let span = Span::new(callee.span().start, self.prev_end());
        Ok(Expr::Call {
            callee: Box::new(callee),
            args,
            span,
        })
    }

    /// Parse `[ index ]` following `object`.
    ///
    /// The AST has no dedicated subscript node, so `obj[i]` is desugared to a
    /// single-argument [`Expr::Call`].  See the Phase 3 notes: the compiler
    /// treats a one-arg `Call` whose callee is a field/identifier uniformly,
    /// so subscripts and calls share a lowering path.
    fn parse_subscript(&mut self, object: Expr) -> Result<Expr, ParseError> {
        self.expect(TokenKind::LBracket)?;
        let index = self.parse_expr(0)?;
        self.expect(TokenKind::RBracket)?;
        let span = Span::new(object.span().start, self.prev_end());
        Ok(Expr::Call {
            callee: Box::new(object),
            args: vec![index],
            span,
        })
    }

    // ── Binding-power tables ─────────────────────────────────────────────────

    const POSTFIX_BP: u8 = 17;

    /// Right binding power for prefix operators, or `None` if `kind` is not a
    /// prefix operator.  `NOT` binds at 5, unary `-` at 13.
    fn prefix_bp(kind: &TokenKind) -> Option<u8> {
        match kind {
            TokenKind::Not => Some(5),
            TokenKind::Minus => Some(13),
            _ => None,
        }
    }

    /// `(left_bp, right_bp)` for infix operators, or `None`.
    ///
    /// Postfix `(`/`[` are *not* listed here; they are handled directly in
    /// [`Self::parse_expr`] at [`Self::POSTFIX_BP`].
    fn infix_bp(kind: &TokenKind) -> Option<(u8, u8)> {
        Some(match kind {
            TokenKind::Or => (1, 2),
            TokenKind::And => (3, 4),
            TokenKind::Eq
            | TokenKind::NotEq
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::LtEq
            | TokenKind::GtEq => (7, 8),
            TokenKind::Plus | TokenKind::Minus => (9, 10),
            TokenKind::Star | TokenKind::Slash => (11, 12),
            TokenKind::Dot => (15, 16),
            _ => return None,
        })
    }

    fn is_comparison(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Eq
                | TokenKind::NotEq
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::LtEq
                | TokenKind::GtEq
        )
    }

    fn bin_op(kind: &TokenKind) -> Option<BinOp> {
        Some(match kind {
            TokenKind::And => BinOp::And,
            TokenKind::Or => BinOp::Or,
            TokenKind::Eq => BinOp::Eq,
            TokenKind::NotEq => BinOp::NotEq,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::LtEq => BinOp::LtEq,
            TokenKind::GtEq => BinOp::GtEq,
            TokenKind::Plus => BinOp::Add,
            TokenKind::Minus => BinOp::Sub,
            TokenKind::Star => BinOp::Mul,
            TokenKind::Slash => BinOp::Div,
            _ => return None,
        })
    }

    fn format_from_kind(kind: &TokenKind) -> Option<Format> {
        Some(match kind {
            TokenKind::Bam => Format::Bam,
            TokenKind::Vcf => Format::Vcf,
            TokenKind::Fasta => Format::Fasta,
            TokenKind::Bed => Format::Bed,
            TokenKind::Cram => Format::Cram,
            TokenKind::Json => Format::Json,
            TokenKind::Tsv => Format::Tsv,
            _ => return None,
        })
    }
}
