//! Abstract syntax tree for SpliceQL.
//!
//! A parsed query is a [`Query`] — one required `FROM` clause plus a set of
//! optional, order-independent clauses.  Every node carries a [`Span`] into the
//! original source so later phases (the bytecode compiler) and error reporting
//! can point back at exact byte ranges without re-scanning.
//!
//! All node types derive `Debug`, `Clone`, and `PartialEq`.  `PartialEq` makes
//! the parser tests literal-comparison friendly; note that [`Expr::FloatLit`]
//! therefore inherits `f64`'s `PartialEq` (so `NaN != NaN`) — this is
//! intentional and matches the lexer's `TokenKind::FloatLit` behaviour.

use crate::token::Span;

// ── Top-level query ──────────────────────────────────────────────────────────

/// A complete SpliceQL query.
///
/// `FROM` is the only required clause.  Every other field is `None` when its
/// clause is absent.  Clause order in the source is free (with one soft rule:
/// `SELECT` should precede `WHERE`), so the presence of a field says nothing
/// about where it appeared.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub from: FromClause,
    pub select: Option<Vec<SelectItem>>,
    pub filter: Option<Expr>, // WHERE
    pub call: Option<CallClause>,
    pub with: Option<Vec<(String, Expr)>>,
    pub into: Option<IntoClause>,
    pub order: Option<Vec<OrderItem>>,
    pub limit: Option<Expr>,
    pub span: Span,
}

// ── Clauses ──────────────────────────────────────────────────────────────────

/// `FROM <format> "<path>" [AS <alias>] [ISEC <format> "<path>" [MODE <mode>]]`.
///
/// The optional `isec` field turns the `FROM` into a two-input VCF set
/// operation (see [`IsecClause`]); when `None` this is an ordinary single
/// source.
#[derive(Debug, Clone, PartialEq)]
pub struct FromClause {
    pub format: Format,
    pub path: String,
    pub alias: Option<String>,
    pub isec: Option<IsecClause>,
    pub span: Span,
}

/// `ISEC <format> "<path>" [MODE <mode>]` — the second input and partition of a
/// VCF set operation. Records of the two inputs are matched on the exact
/// `(chrom, pos, ref, alt)` key (bcftools-isec semantics).
#[derive(Debug, Clone, PartialEq)]
pub struct IsecClause {
    pub format: Format,
    pub path: String,
    pub mode: IsecMode,
    pub span: Span,
}

/// Which partition a VCF `ISEC` emits. Mirrors the files `bcftools isec -p`
/// produces. `Shared` is the default when no `MODE` is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsecMode {
    /// Records private to the first (`A`) input — bcftools `0000.vcf`.
    PrivateA,
    /// Records private to the second (`B`) input — bcftools `0001.vcf`.
    PrivateB,
    /// Shared records, taken from `A` — bcftools `0002.vcf` (the default).
    Shared,
    /// Shared records, taken from `B` — bcftools `0003.vcf`.
    SharedB,
    /// All records: `A` plus the `B`-private records.
    Union,
}

/// `CALL <operation>` — `operation` is one of the validated operation names
/// (`variants`, `cnv`, `coverage`, `reads`, `header`).
#[derive(Debug, Clone, PartialEq)]
pub struct CallClause {
    pub operation: String,
    pub span: Span,
}

/// `INTO <format> "<path>"`.
#[derive(Debug, Clone, PartialEq)]
pub struct IntoClause {
    pub format: Format,
    pub path: String,
    pub span: Span,
}

/// A single projected item: `<expr> [AS <alias>]`.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<String>,
    pub span: Span,
}

/// A single `ORDER BY` term: `<expr> [ASC | DESC]`.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    pub expr: Expr,
    pub direction: Direction,
    pub span: Span,
}

/// Sort direction; `ASC` is the default when neither keyword is present.
#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Asc,
    Desc,
}

// ── Formats ──────────────────────────────────────────────────────────────────

/// A genomic file format usable in `FROM` and `INTO`.
#[derive(Debug, Clone, PartialEq)]
pub enum Format {
    Bam,
    Vcf,
    Fasta,
    Bed,
    Cram,
    /// Output-only: newline-delimited JSON (one object per record).
    Json,
    /// Output-only: tab-separated values with a header row.
    Tsv,
}

// ── Operators ────────────────────────────────────────────────────────────────

/// Prefix unary operators.
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg, // -x
    Not, // NOT x
}

/// Infix binary operators.
#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    And,
    Or,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Add,
    Sub,
    Mul,
    Div,
}

// ── Expressions ──────────────────────────────────────────────────────────────

/// An expression node.  Spans cover the full sub-expression: a `Binary`'s span
/// runs from its left operand's start to its right operand's end.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    IntLit(i64, Span),
    FloatLit(f64, Span),
    StringLit(String, Span),
    BoolLit(bool, Span),
    Ident(String, Span),
    /// A `$name` template variable, resolved at runtime from the VarMap.
    Var(String, Span),
    Wildcard(Span),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    FieldAccess {
        object: Box<Expr>,
        field: String,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
}

impl Expr {
    /// The source span this expression covers.
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit(_, s)
            | Expr::FloatLit(_, s)
            | Expr::StringLit(_, s)
            | Expr::BoolLit(_, s)
            | Expr::Ident(_, s)
            | Expr::Var(_, s)
            | Expr::Wildcard(s)
            | Expr::Unary { span: s, .. }
            | Expr::Binary { span: s, .. }
            | Expr::FieldAccess { span: s, .. }
            | Expr::Call { span: s, .. } => *s,
        }
    }
}
