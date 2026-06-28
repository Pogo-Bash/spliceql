//! Token types and source spans for SpliceQL.
//!
//! Every token the lexer produces carries a [`Span`] recording its byte-offset
//! range in the source string.  The parser (Phase 2) uses these spans for error
//! reporting without needing to re-scan the source.

use std::fmt;

// ── Span ────────────────────────────────────────────────────────────────────

/// Byte-offset range `[start, end)` into the source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[inline]
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

// ── Token ───────────────────────────────────────────────────────────────────

/// A single lexed token.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

// ── TokenKind ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Language keywords ──
    From,
    Select,
    Where,
    Call,
    With,
    Annotate,
    Into,
    Filter,
    And,
    Or,
    Not,
    As,
    Limit,
    Order,
    By,
    Asc,
    Desc,
    /// `SPLIT` — decompose multi-allelic VCF records into biallelic rows on load.
    Split,

    // ── Genomic keywords ──
    Bam,
    Vcf,
    Fasta,
    Bed,
    Cram,
    // Output-only sink formats.
    Json,
    Tsv,
    Variants,
    Cnv,
    Coverage,
    Reads,
    Header,
    /// VCF set operation join: `FROM vcf "a" ISEC vcf "b"`.
    Isec,
    /// Tumor/normal somatic join: `FROM vcf "tumor" PAIRED WITH vcf "normal"`.
    Paired,
    /// Selects the set-operation partition: `MODE shared`.
    Mode,

    // ── Literals ──
    StringLit(String),
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),

    // ── Identifiers ──
    Ident(String),

    // ── Variables ──
    /// A `$name` template variable (the name excludes the leading `$`).
    Var(String),

    // ── Comparison operators ──
    Eq,    // =
    NotEq, // !=
    Lt,    // <
    Gt,    // >
    LtEq,  // <=
    GtEq,  // >=

    // ── Arithmetic operators ──
    Plus,  // +
    Minus, // -
    Star,  // *
    Slash, // /

    // ── Punctuation ──
    Dot,       // .
    Comma,     // ,
    Semicolon, // ;
    Colon,     // :
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]

    // ── Special ──
    Eof,
    Newline,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::From => f.write_str("FROM"),
            Self::Select => f.write_str("SELECT"),
            Self::Where => f.write_str("WHERE"),
            Self::Call => f.write_str("CALL"),
            Self::With => f.write_str("WITH"),
            Self::Annotate => f.write_str("ANNOTATE"),
            Self::Into => f.write_str("INTO"),
            Self::Filter => f.write_str("FILTER"),
            Self::And => f.write_str("AND"),
            Self::Or => f.write_str("OR"),
            Self::Not => f.write_str("NOT"),
            Self::As => f.write_str("AS"),
            Self::Limit => f.write_str("LIMIT"),
            Self::Order => f.write_str("ORDER"),
            Self::By => f.write_str("BY"),
            Self::Asc => f.write_str("ASC"),
            Self::Desc => f.write_str("DESC"),
            Self::Split => f.write_str("SPLIT"),

            Self::Bam => f.write_str("BAM"),
            Self::Vcf => f.write_str("VCF"),
            Self::Fasta => f.write_str("FASTA"),
            Self::Bed => f.write_str("BED"),
            Self::Cram => f.write_str("CRAM"),
            Self::Json => f.write_str("JSON"),
            Self::Tsv => f.write_str("TSV"),
            Self::Variants => f.write_str("VARIANTS"),
            Self::Cnv => f.write_str("CNV"),
            Self::Coverage => f.write_str("COVERAGE"),
            Self::Reads => f.write_str("READS"),
            Self::Header => f.write_str("HEADER"),
            Self::Isec => f.write_str("ISEC"),
            Self::Paired => f.write_str("PAIRED"),
            Self::Mode => f.write_str("MODE"),

            Self::StringLit(s) => write!(f, "\"{s}\""),
            Self::IntLit(n) => write!(f, "{n}"),
            Self::FloatLit(v) => write!(f, "{v}"),
            Self::BoolLit(b) => write!(f, "{b}"),
            Self::Ident(name) => f.write_str(name),
            Self::Var(name) => write!(f, "${name}"),

            Self::Eq => f.write_str("="),
            Self::NotEq => f.write_str("!="),
            Self::Lt => f.write_str("<"),
            Self::Gt => f.write_str(">"),
            Self::LtEq => f.write_str("<="),
            Self::GtEq => f.write_str(">="),
            Self::Plus => f.write_str("+"),
            Self::Minus => f.write_str("-"),
            Self::Star => f.write_str("*"),
            Self::Slash => f.write_str("/"),

            Self::Dot => f.write_str("."),
            Self::Comma => f.write_str(","),
            Self::Semicolon => f.write_str(";"),
            Self::Colon => f.write_str(":"),
            Self::LParen => f.write_str("("),
            Self::RParen => f.write_str(")"),
            Self::LBracket => f.write_str("["),
            Self::RBracket => f.write_str("]"),

            Self::Eof => f.write_str("EOF"),
            Self::Newline => f.write_str("\\n"),
        }
    }
}
