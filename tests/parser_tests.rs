//! Integration tests for the SpliceQL parser (Phase 2).
//!
//! Organised into five groups mirroring the Phase 2 spec:
//!   1. Happy-path whole queries
//!   2. Expression precedence / associativity
//!   3. Error cases
//!   4. Span accuracy
//!   5. (Fuzz extension lives under `fuzz/`, not here.)

use spliceql::ast::*;
use spliceql::error::ParseError;
use spliceql::{parse, Span};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn ok(src: &str) -> Query {
    parse(src).unwrap_or_else(|e| panic!("expected parse to succeed for {src:?}, got {e}"))
}

fn err(src: &str) -> ParseError {
    parse(src).expect_err(&format!("expected parse error for {src:?}"))
}

/// Build a `Binary` ignoring spans for structural comparison.  We compare AST
/// *shapes* by walking, since literal spans make `==` brittle.
fn binop(e: &Expr) -> Option<&BinOp> {
    match e {
        Expr::Binary { op, .. } => Some(op),
        _ => None,
    }
}

fn sides(e: &Expr) -> (&Expr, &Expr) {
    match e {
        Expr::Binary { left, right, .. } => (left, right),
        _ => panic!("not a binary expr: {e:?}"),
    }
}

fn ident(e: &Expr) -> &str {
    match e {
        Expr::Ident(s, _) => s,
        _ => panic!("not an ident: {e:?}"),
    }
}

// ── Group 1 — Happy path queries ─────────────────────────────────────────────

#[test]
fn basic_bam_query_with_where_and_call() {
    let q = ok(r#"FROM bam "sample.bam" WHERE depth > 30 CALL variants"#);
    assert_eq!(q.from.format, Format::Bam);
    assert_eq!(q.from.path, "sample.bam");
    assert!(q.from.alias.is_none());
    assert!(q.filter.is_some());
    assert_eq!(q.call.as_ref().unwrap().operation, "variants");
    assert!(q.into.is_none());

    let filter = q.filter.unwrap();
    assert_eq!(binop(&filter), Some(&BinOp::Gt));
    let (l, r) = sides(&filter);
    assert_eq!(ident(l), "depth");
    assert_eq!(*r, Expr::IntLit(30, r.span()));
}

#[test]
fn vcf_query_with_into() {
    let q = ok(r#"FROM vcf "in.vcf" CALL variants INTO vcf "out.vcf""#);
    assert_eq!(q.from.format, Format::Vcf);
    let into = q.into.unwrap();
    assert_eq!(into.format, Format::Vcf);
    assert_eq!(into.path, "out.vcf");
}

#[test]
fn select_star_where_order_limit() {
    let q = ok(r#"FROM bam "r.bam" SELECT * WHERE qual >= 20 ORDER BY qual DESC LIMIT 100"#);

    let select = q.select.unwrap();
    assert_eq!(select.len(), 1);
    assert!(matches!(select[0].expr, Expr::Wildcard(_)));

    assert!(q.filter.is_some());

    let order = q.order.unwrap();
    assert_eq!(order.len(), 1);
    assert_eq!(order[0].direction, Direction::Desc);
    assert_eq!(ident(&order[0].expr), "qual");

    let limit = q.limit.unwrap();
    assert_eq!(limit, Expr::IntLit(100, limit.span()));
}

#[test]
fn multi_condition_where_and_or_precedence() {
    // a OR b AND c  →  a OR (b AND c)
    let q = ok(r#"FROM bam "x.bam" WHERE a OR b AND c"#);
    let filter = q.filter.unwrap();
    assert_eq!(binop(&filter), Some(&BinOp::Or));
    let (l, r) = sides(&filter);
    assert_eq!(ident(l), "a");
    assert_eq!(binop(r), Some(&BinOp::And));
    let (rl, rr) = sides(r);
    assert_eq!(ident(rl), "b");
    assert_eq!(ident(rr), "c");
}

#[test]
fn query_with_all_clauses() {
    let src = r#"
        FROM bam "tumor.bam" AS t
        SELECT chr, pos AS position
        WHERE chr = "chr7" AND depth > 30
        CALL cnv
        WITH window_size = 10000, amp_threshold = 1.5
        INTO vcf "cnvs.vcf"
        ORDER BY pos ASC, qual DESC
        LIMIT 50
    "#;
    let q = ok(src);
    assert_eq!(q.from.alias.as_deref(), Some("t"));
    assert_eq!(q.select.as_ref().unwrap().len(), 2);
    assert_eq!(q.select.as_ref().unwrap()[1].alias.as_deref(), Some("position"));
    assert!(q.filter.is_some());
    assert_eq!(q.call.as_ref().unwrap().operation, "cnv");

    let with = q.with.unwrap();
    assert_eq!(with.len(), 2);
    assert_eq!(with[0].0, "window_size");
    assert_eq!(with[0].1, Expr::IntLit(10000, with[0].1.span()));
    assert_eq!(with[1].0, "amp_threshold");
    assert_eq!(with[1].1, Expr::FloatLit(1.5, with[1].1.span()));

    assert_eq!(q.into.as_ref().unwrap().format, Format::Vcf);
    let order = q.order.unwrap();
    assert_eq!(order.len(), 2);
    assert_eq!(order[0].direction, Direction::Asc);
    assert_eq!(order[1].direction, Direction::Desc);
    let limit = q.limit.unwrap();
    assert_eq!(limit, Expr::IntLit(50, limit.span()));
}

#[test]
fn all_call_operations() {
    for (src_op, want) in [
        ("variants", "variants"),
        ("cnv", "cnv"),
        ("coverage", "coverage"),
        ("reads", "reads"),
        ("header", "header"),
    ] {
        let q = ok(&format!(r#"FROM bam "x.bam" CALL {src_op}"#));
        assert_eq!(q.call.unwrap().operation, want);
    }
}

#[test]
fn clauses_are_order_independent() {
    // CALL before WHERE, LIMIT before INTO — all optional clauses commute.
    let q = ok(r#"FROM bam "x.bam" CALL reads LIMIT 5 WHERE depth > 1 INTO bam "o.bam""#);
    assert_eq!(q.call.unwrap().operation, "reads");
    assert!(q.filter.is_some());
    assert!(q.into.is_some());
    assert!(q.limit.is_some());
}

#[test]
fn filter_keyword_is_an_alias_for_where() {
    let q = ok(r#"FROM bam "x.bam" FILTER depth > 10"#);
    assert!(q.filter.is_some());
}

// ── Group 2 — Expression precedence ──────────────────────────────────────────

#[test]
fn or_binds_looser_than_and() {
    let q = ok(r#"FROM bam "x.bam" WHERE a AND b OR c"#);
    // a AND b OR c  →  (a AND b) OR c
    let f = q.filter.unwrap();
    assert_eq!(binop(&f), Some(&BinOp::Or));
    let (l, r) = sides(&f);
    assert_eq!(binop(l), Some(&BinOp::And));
    assert_eq!(ident(r), "c");
}

#[test]
fn unary_minus_binds_tighter_than_mul() {
    // -a * b  →  (-a) * b
    let q = ok(r#"FROM bam "x.bam" WHERE -a * b > 0"#);
    let f = q.filter.unwrap();
    // top is the comparison
    assert_eq!(binop(&f), Some(&BinOp::Gt));
    let (l, _) = sides(&f);
    assert_eq!(binop(l), Some(&BinOp::Mul));
    let (ll, lr) = sides(l);
    assert!(matches!(ll, Expr::Unary { op: UnaryOp::Neg, .. }));
    assert_eq!(ident(lr), "b");
}

#[test]
fn not_binds_looser_than_comparison_tighter_than_or() {
    // NOT a OR b  →  (NOT a) OR b
    let q = ok(r#"FROM bam "x.bam" WHERE NOT a OR b"#);
    let f = q.filter.unwrap();
    assert_eq!(binop(&f), Some(&BinOp::Or));
    let (l, r) = sides(&f);
    assert!(matches!(l, Expr::Unary { op: UnaryOp::Not, .. }));
    assert_eq!(ident(r), "b");
}

#[test]
fn not_applies_to_full_comparison() {
    // NOT depth > 30  →  NOT (depth > 30), because NOT (rbp 5) < comparison (lbp 7)
    let q = ok(r#"FROM bam "x.bam" WHERE NOT depth > 30"#);
    let f = q.filter.unwrap();
    match f {
        Expr::Unary { op: UnaryOp::Not, operand, .. } => {
            assert_eq!(binop(&operand), Some(&BinOp::Gt));
        }
        other => panic!("expected NOT at the top, got {other:?}"),
    }
}

#[test]
fn field_access() {
    let q = ok(r#"FROM bam "x.bam" WHERE reads.depth > 30"#);
    let f = q.filter.unwrap();
    assert_eq!(binop(&f), Some(&BinOp::Gt));
    let (l, _) = sides(&f);
    match l {
        Expr::FieldAccess { object, field, .. } => {
            assert_eq!(ident(object), "reads");
            assert_eq!(field, "depth");
        }
        other => panic!("expected field access, got {other:?}"),
    }
}

#[test]
fn chained_field_access_is_left_associative() {
    let q = ok(r#"FROM bam "x.bam" WHERE a.b.c = 1"#);
    let f = q.filter.unwrap();
    let (l, _) = sides(&f);
    // ((a.b).c)
    match l {
        Expr::FieldAccess { object, field, .. } => {
            assert_eq!(field, "c");
            match object.as_ref() {
                Expr::FieldAccess { object: inner, field: f2, .. } => {
                    assert_eq!(ident(inner), "a");
                    assert_eq!(f2, "b");
                }
                other => panic!("expected nested field access, got {other:?}"),
            }
        }
        other => panic!("expected field access, got {other:?}"),
    }
}

#[test]
fn parentheses_override_precedence() {
    // (a OR b) AND c
    let q = ok(r#"FROM bam "x.bam" WHERE (a OR b) AND c"#);
    let f = q.filter.unwrap();
    assert_eq!(binop(&f), Some(&BinOp::And));
    let (l, r) = sides(&f);
    assert_eq!(binop(l), Some(&BinOp::Or));
    assert_eq!(ident(r), "c");
}

#[test]
fn mixed_arithmetic_and_comparison() {
    // depth > 30 + 5  →  depth > (30 + 5)
    let q = ok(r#"FROM bam "x.bam" WHERE depth > 30 + 5"#);
    let f = q.filter.unwrap();
    assert_eq!(binop(&f), Some(&BinOp::Gt));
    let (_, r) = sides(&f);
    assert_eq!(binop(r), Some(&BinOp::Add));
}

#[test]
fn star_is_multiplication_in_infix_position() {
    let q = ok(r#"FROM bam "x.bam" SELECT qual * 2"#);
    let item = &q.select.unwrap()[0];
    assert_eq!(binop(&item.expr), Some(&BinOp::Mul));
}

#[test]
fn function_call_expression() {
    let q = ok(r#"FROM bam "x.bam" SELECT mean(depth, qual)"#);
    let item = &q.select.unwrap()[0];
    match &item.expr {
        Expr::Call { callee, args, .. } => {
            assert_eq!(ident(callee), "mean");
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected call, got {other:?}"),
    }
}

// ── Group 3 — Error cases ────────────────────────────────────────────────────

#[test]
fn missing_from_is_an_error() {
    let e = err(r#"SELECT * WHERE depth > 30"#);
    assert!(
        matches!(e, ParseError::UnexpectedToken { .. } | ParseError::UnexpectedEof { .. }),
        "got {e:?}"
    );
}

#[test]
fn empty_input_is_an_error() {
    let e = err("");
    assert!(matches!(e, ParseError::UnexpectedEof { .. }), "got {e:?}");
}

#[test]
fn chained_comparison_is_rejected() {
    let e = err(r#"FROM bam "x.bam" WHERE a > b > c"#);
    assert!(matches!(e, ParseError::ChainedComparison { .. }), "got {e:?}");
}

#[test]
fn chained_comparison_eq() {
    let e = err(r#"FROM bam "x.bam" WHERE a = b = c"#);
    assert!(matches!(e, ParseError::ChainedComparison { .. }), "got {e:?}");
}

#[test]
fn invalid_format_is_rejected() {
    let e = err(r#"FROM csv "x.csv""#);
    match e {
        ParseError::InvalidFormat { got, .. } => assert_eq!(got, "csv"),
        other => panic!("expected InvalidFormat, got {other:?}"),
    }
}

#[test]
fn invalid_call_op_is_rejected() {
    let e = err(r#"FROM bam "x.bam" CALL snp"#);
    match e {
        ParseError::InvalidCallOp { got, .. } => assert_eq!(got, "snp"),
        other => panic!("expected InvalidCallOp, got {other:?}"),
    }
}

#[test]
fn unterminated_expression_is_an_error() {
    let e = err(r#"FROM bam "x.bam" WHERE depth >"#);
    assert!(matches!(e, ParseError::UnexpectedEof { .. }), "got {e:?}");
}

#[test]
fn unclosed_paren_is_an_error() {
    let e = err(r#"FROM bam "x.bam" WHERE (a OR b"#);
    assert!(matches!(e, ParseError::UnexpectedEof { .. }), "got {e:?}");
}

#[test]
fn lex_error_propagates_through_parse() {
    // `@` is not a valid character; the lexer fails and parse() wraps it.
    let e = err(r#"FROM bam "x.bam" WHERE @"#);
    match e {
        ParseError::LexError(le) => assert_eq!(le.ch, Some('@')),
        other => panic!("expected LexError, got {other:?}"),
    }
}

#[test]
fn missing_path_after_format_is_an_error() {
    let e = err(r#"FROM bam CALL variants"#);
    assert!(matches!(e, ParseError::UnexpectedToken { .. }), "got {e:?}");
}

// ── Group 4 — Span accuracy ──────────────────────────────────────────────────

#[test]
fn literal_spans_point_at_source_bytes() {
    //                0123456789...                       (byte offsets)
    let src = r#"FROM bam "x.bam" WHERE depth > 30"#;
    let depth_at = src.find("depth").unwrap();
    let thirty_at = src.find("30").unwrap();

    let q = ok(src);
    let f = q.filter.unwrap();
    let (l, r) = sides(&f);

    assert_eq!(l.span(), Span::new(depth_at, depth_at + "depth".len()));
    assert_eq!(r.span(), Span::new(thirty_at, thirty_at + "30".len()));
    // The binary's span covers depth..30.
    assert_eq!(f.span(), Span::new(depth_at, thirty_at + "30".len()));
}

#[test]
fn binary_span_spans_both_operands() {
    let src = r#"FROM bam "x.bam" WHERE a + b * c"#;
    let a_at = src.find("a +").unwrap();
    let c_at = src.rfind('c').unwrap();
    let q = ok(src);
    let f = q.filter.unwrap();
    assert_eq!(f.span(), Span::new(a_at, c_at + 1));
}

#[test]
fn unary_span_includes_operator() {
    let src = r#"FROM bam "x.bam" WHERE -a > 0"#;
    let minus_at = src.find("-a").unwrap();
    let q = ok(src);
    let f = q.filter.unwrap();
    let (l, _) = sides(&f);
    match l {
        Expr::Unary { span, operand, .. } => {
            assert_eq!(span.start, minus_at);
            assert_eq!(span.end, operand.span().end);
        }
        other => panic!("expected unary, got {other:?}"),
    }
}

#[test]
fn from_clause_span_is_accurate() {
    let src = r#"FROM bam "sample.bam" CALL variants"#;
    let q = ok(src);
    let end = src.find("\" CALL").map(|i| i + 1).unwrap(); // closing quote inclusive
    assert_eq!(q.from.span, Span::new(0, end));
}

#[test]
fn error_span_points_at_offending_token() {
    let src = r#"FROM bam "x.bam" WHERE a > b > c"#;
    // span of the second `>`
    let second_gt = src.rfind('>').unwrap();
    let e = err(src);
    assert_eq!(e.span(), Span::new(second_gt, second_gt + 1));
}

#[test]
fn invalid_format_span_points_at_format_token() {
    let src = r#"FROM csv "x.csv""#;
    let csv_at = src.find("csv").unwrap();
    let e = err(src);
    assert_eq!(e.span(), Span::new(csv_at, csv_at + 3));
}

// ── line/col helper ──────────────────────────────────────────────────────────

#[test]
fn byte_offset_to_line_col_basic() {
    use spliceql::error::byte_offset_to_line_col;
    let src = "FROM bam\nWHERE x";
    assert_eq!(byte_offset_to_line_col(src, 0), (1, 1));
    assert_eq!(byte_offset_to_line_col(src, 5), (1, 6)); // 'b' in bam
    assert_eq!(byte_offset_to_line_col(src, 9), (2, 1)); // 'W' on line 2
}

// ── ANNOTATE clause (Track 1) ─────────────────────────────────────────────────

#[test]
fn annotate_clause_parses_database_paths() {
    let q = ok(r#"FROM vcf "in.vcf" ANNOTATE WITH genes="g.gff3", clinvar="c.vcf.gz""#);
    let ann = q.annotate.expect("ANNOTATE clause present");
    assert_eq!(ann.params.len(), 2);
    assert_eq!(ann.params[0].0, "genes");
    assert!(matches!(&ann.params[0].1, Expr::StringLit(s, _) if s == "g.gff3"));
    assert_eq!(ann.params[1].0, "clinvar");
    assert!(matches!(&ann.params[1].1, Expr::StringLit(s, _) if s == "c.vcf.gz"));
}

#[test]
fn annotate_clause_is_order_independent() {
    let q = ok(r#"FROM vcf "in.vcf" ANNOTATE WITH genes="g.gff3" WHERE gene = "EGFR""#);
    assert!(q.annotate.is_some());
    assert!(q.filter.is_some());
}

#[test]
fn annotate_clause_accepts_var_path() {
    let q = ok(r#"FROM vcf "in.vcf" ANNOTATE WITH clinvar=$db"#);
    let ann = q.annotate.expect("ANNOTATE present");
    assert!(matches!(&ann.params[0].1, Expr::Var(name, _) if name == "db"));
}
