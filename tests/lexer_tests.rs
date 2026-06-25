use spliceql::{tokenize, LexError, Lexer, Span, TokenKind};

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Collect token kinds from `source`, excluding the trailing Eof.
fn kinds(source: &str) -> Vec<TokenKind> {
    tokenize(source)
        .expect("unexpected lex error")
        .into_iter()
        .map(|t| t.kind)
        .filter(|k| *k != TokenKind::Eof)
        .collect()
}

/// Expect a lex error from `source`.
fn lex_err(source: &str) -> LexError {
    tokenize(source).expect_err("expected lex error")
}

// ── Empty / whitespace ──────────────────────────────────────────────────────

#[test]
fn empty_input() {
    let tokens = tokenize("").unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Eof);
}

#[test]
fn whitespace_only() {
    let tokens = tokenize("   \t  \n  \r\n  ").unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Eof);
}

// ── Language keywords (case-insensitive) ────────────────────────────────────

#[test]
fn language_keywords_uppercase() {
    assert_eq!(
        kinds("FROM SELECT WHERE CALL WITH INTO FILTER AND OR NOT AS LIMIT ORDER BY ASC DESC"),
        vec![
            TokenKind::From,
            TokenKind::Select,
            TokenKind::Where,
            TokenKind::Call,
            TokenKind::With,
            TokenKind::Into,
            TokenKind::Filter,
            TokenKind::And,
            TokenKind::Or,
            TokenKind::Not,
            TokenKind::As,
            TokenKind::Limit,
            TokenKind::Order,
            TokenKind::By,
            TokenKind::Asc,
            TokenKind::Desc,
        ]
    );
}

#[test]
fn language_keywords_lowercase() {
    assert_eq!(
        kinds("from select where call with into filter and or not as limit order by asc desc"),
        vec![
            TokenKind::From,
            TokenKind::Select,
            TokenKind::Where,
            TokenKind::Call,
            TokenKind::With,
            TokenKind::Into,
            TokenKind::Filter,
            TokenKind::And,
            TokenKind::Or,
            TokenKind::Not,
            TokenKind::As,
            TokenKind::Limit,
            TokenKind::Order,
            TokenKind::By,
            TokenKind::Asc,
            TokenKind::Desc,
        ]
    );
}

#[test]
fn language_keywords_mixed_case() {
    assert_eq!(
        kinds("From sElEcT wHeRe CaLl"),
        vec![TokenKind::From, TokenKind::Select, TokenKind::Where, TokenKind::Call]
    );
}

// ── Genomic keywords ────────────────────────────────────────────────────────

#[test]
fn genomic_keywords() {
    assert_eq!(
        kinds("BAM VCF FASTA BED CRAM VARIANTS CNV COVERAGE READS HEADER"),
        vec![
            TokenKind::Bam,
            TokenKind::Vcf,
            TokenKind::Fasta,
            TokenKind::Bed,
            TokenKind::Cram,
            TokenKind::Variants,
            TokenKind::Cnv,
            TokenKind::Coverage,
            TokenKind::Reads,
            TokenKind::Header,
        ]
    );
}

#[test]
fn genomic_keywords_lowercase() {
    assert_eq!(
        kinds("bam vcf fasta bed cram variants cnv coverage reads header"),
        vec![
            TokenKind::Bam,
            TokenKind::Vcf,
            TokenKind::Fasta,
            TokenKind::Bed,
            TokenKind::Cram,
            TokenKind::Variants,
            TokenKind::Cnv,
            TokenKind::Coverage,
            TokenKind::Reads,
            TokenKind::Header,
        ]
    );
}

// ── Boolean literals ────────────────────────────────────────────────────────

#[test]
fn bool_literals() {
    assert_eq!(
        kinds("true false TRUE FALSE True False"),
        vec![
            TokenKind::BoolLit(true),
            TokenKind::BoolLit(false),
            TokenKind::BoolLit(true),
            TokenKind::BoolLit(false),
            TokenKind::BoolLit(true),
            TokenKind::BoolLit(false),
        ]
    );
}

// ── Identifiers ─────────────────────────────────────────────────────────────

#[test]
fn identifiers() {
    assert_eq!(
        kinds("depth qual chr min_af"),
        vec![
            TokenKind::Ident("depth".into()),
            TokenKind::Ident("qual".into()),
            TokenKind::Ident("chr".into()),
            TokenKind::Ident("min_af".into()),
        ]
    );
}

#[test]
fn identifiers_with_digits_and_underscores() {
    assert_eq!(
        kinds("chr7 _private name_123 __x"),
        vec![
            TokenKind::Ident("chr7".into()),
            TokenKind::Ident("_private".into()),
            TokenKind::Ident("name_123".into()),
            TokenKind::Ident("__x".into()),
        ]
    );
}

#[test]
fn identifier_preserves_case() {
    // Keywords are case-folded; identifiers are not.
    assert_eq!(
        kinds("myVar MyVar MYVAR"),
        vec![
            TokenKind::Ident("myVar".into()),
            TokenKind::Ident("MyVar".into()),
            TokenKind::Ident("MYVAR".into()),
        ]
    );
}

// ── String literals ─────────────────────────────────────────────────────────

#[test]
fn string_literal_simple() {
    assert_eq!(
        kinds(r#""hello" "sample.bam" "chr7""#),
        vec![
            TokenKind::StringLit("hello".into()),
            TokenKind::StringLit("sample.bam".into()),
            TokenKind::StringLit("chr7".into()),
        ]
    );
}

#[test]
fn string_literal_empty() {
    assert_eq!(kinds(r#""""#), vec![TokenKind::StringLit("".into())]);
}

#[test]
fn string_literal_escapes() {
    assert_eq!(
        kinds(r#""\n\t\r\\\"\0""#),
        vec![TokenKind::StringLit("\n\t\r\\\"\0".into())]
    );
}

#[test]
fn string_literal_with_spaces() {
    assert_eq!(
        kinds(r#""hello world""#),
        vec![TokenKind::StringLit("hello world".into())]
    );
}

#[test]
fn string_unterminated() {
    let err = lex_err(r#""hello"#);
    assert_eq!(err.message, "unterminated string literal");
    assert_eq!(err.ch, None);
}

#[test]
fn string_unterminated_with_backslash() {
    let err = lex_err(r#""hello\"#);
    assert_eq!(err.message, "unterminated string literal");
}

#[test]
fn string_invalid_escape() {
    let err = lex_err(r#""\x""#);
    assert!(err.message.contains("invalid escape sequence"));
    assert_eq!(err.ch, Some('x'));
}

// ── Integer literals ────────────────────────────────────────────────────────

#[test]
fn integer_literals() {
    assert_eq!(
        kinds("0 42 999999 10"),
        vec![
            TokenKind::IntLit(0),
            TokenKind::IntLit(42),
            TokenKind::IntLit(999999),
            TokenKind::IntLit(10),
        ]
    );
}

// ── Float literals ──────────────────────────────────────────────────────────

#[test]
fn float_literals() {
    assert_eq!(
        kinds("1.5 0.05 3.14159"),
        vec![
            TokenKind::FloatLit(1.5),
            TokenKind::FloatLit(0.05),
            TokenKind::FloatLit(3.14159),
        ]
    );
}

#[test]
fn float_leading_dot() {
    assert_eq!(
        kinds(".5 .123"),
        vec![TokenKind::FloatLit(0.5), TokenKind::FloatLit(0.123)]
    );
}

#[test]
fn float_scientific_notation() {
    assert_eq!(
        kinds("1e5 1.5e-3 2E10 1.0e+2"),
        vec![
            TokenKind::FloatLit(1e5),
            TokenKind::FloatLit(1.5e-3),
            TokenKind::FloatLit(2e10),
            TokenKind::FloatLit(1.0e+2),
        ]
    );
}

#[test]
fn negative_number_is_unary_minus() {
    // Negative numbers are Minus + literal; the parser applies unary minus.
    assert_eq!(
        kinds("-5"),
        vec![TokenKind::Minus, TokenKind::IntLit(5)]
    );
    assert_eq!(
        kinds("-3.14"),
        vec![TokenKind::Minus, TokenKind::FloatLit(3.14)]
    );
    assert_eq!(
        kinds("-.5"),
        vec![TokenKind::Minus, TokenKind::FloatLit(0.5)]
    );
}

#[test]
fn integer_dot_not_followed_by_digit() {
    // `1.` where `.` is not followed by a digit → IntLit, Dot
    assert_eq!(
        kinds("1."),
        vec![TokenKind::IntLit(1), TokenKind::Dot]
    );
}

#[test]
fn number_followed_by_ident() {
    // `123abc` → IntLit, Ident (no fused alphanumeric tokens)
    assert_eq!(
        kinds("123abc"),
        vec![TokenKind::IntLit(123), TokenKind::Ident("abc".into())]
    );
}

#[test]
fn exponent_without_digits_is_not_float() {
    // `1e` → IntLit(1), Ident("e") — not a float
    assert_eq!(
        kinds("1e"),
        vec![TokenKind::IntLit(1), TokenKind::Ident("e".into())]
    );
    // `1e+` → IntLit(1), Ident("e"), Plus
    assert_eq!(
        kinds("1e+"),
        vec![TokenKind::IntLit(1), TokenKind::Ident("e".into()), TokenKind::Plus]
    );
}

// ── Operators ───────────────────────────────────────────────────────────────

#[test]
fn all_operators() {
    assert_eq!(
        kinds("= != < > <= >= + - * /"),
        vec![
            TokenKind::Eq,
            TokenKind::NotEq,
            TokenKind::Lt,
            TokenKind::Gt,
            TokenKind::LtEq,
            TokenKind::GtEq,
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
        ]
    );
}

#[test]
fn operators_no_whitespace() {
    assert_eq!(
        kinds("<=>=!="),
        vec![TokenKind::LtEq, TokenKind::GtEq, TokenKind::NotEq]
    );
}

// ── Punctuation ─────────────────────────────────────────────────────────────

#[test]
fn all_punctuation() {
    assert_eq!(
        kinds(". , ; : ( ) [ ]"),
        vec![
            TokenKind::Dot,
            TokenKind::Comma,
            TokenKind::Semicolon,
            TokenKind::Colon,
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBracket,
            TokenKind::RBracket,
        ]
    );
}

// ── Comments ────────────────────────────────────────────────────────────────

#[test]
fn single_line_comment_stripped() {
    assert_eq!(kinds("-- this is a comment"), vec![]);
}

#[test]
fn comment_at_end_of_line() {
    assert_eq!(
        kinds("FROM -- comment\nbam"),
        vec![TokenKind::From, TokenKind::Bam]
    );
}

#[test]
fn multiple_comments() {
    let src = "-- first comment\nFROM -- second\n-- third\nbam";
    assert_eq!(kinds(src), vec![TokenKind::From, TokenKind::Bam]);
}

#[test]
fn comment_does_not_eat_next_line() {
    let src = "-- comment\n42";
    assert_eq!(kinds(src), vec![TokenKind::IntLit(42)]);
}

#[test]
fn single_dash_is_minus_not_comment() {
    assert_eq!(kinds("- 5"), vec![TokenKind::Minus, TokenKind::IntLit(5)]);
}

// ── Error cases ─────────────────────────────────────────────────────────────

#[test]
fn invalid_character() {
    let err = lex_err("@");
    assert_eq!(err.ch, Some('@'));
    assert!(err.message.contains("unexpected character"));
}

#[test]
fn invalid_character_after_valid_tokens() {
    let err = lex_err("FROM @");
    assert_eq!(err.ch, Some('@'));
    assert_eq!(err.span.start, 5);
}

#[test]
fn bare_bang() {
    let err = lex_err("!");
    assert_eq!(err.ch, Some('!'));
    assert!(err.message.contains("expected '=' after '!'"));
}

#[test]
fn bare_bang_before_non_eq() {
    let err = lex_err("!>");
    assert_eq!(err.ch, Some('!'));
}

// ── Span accuracy ───────────────────────────────────────────────────────────

#[test]
fn spans_are_correct() {
    let tokens = tokenize("FROM bam").unwrap();
    // FROM → bytes 0..4
    assert_eq!(tokens[0].span, Span::new(0, 4));
    assert_eq!(tokens[0].kind, TokenKind::From);
    // bam → bytes 5..8
    assert_eq!(tokens[1].span, Span::new(5, 8));
    assert_eq!(tokens[1].kind, TokenKind::Bam);
    // Eof → 8..8
    assert_eq!(tokens[2].span, Span::new(8, 8));
    assert_eq!(tokens[2].kind, TokenKind::Eof);
}

#[test]
fn span_includes_string_quotes() {
    // "abc" occupies bytes 0..5 (including both quotes)
    let tokens = tokenize(r#""abc""#).unwrap();
    assert_eq!(tokens[0].span, Span::new(0, 5));
    assert_eq!(tokens[0].kind, TokenKind::StringLit("abc".into()));
}

#[test]
fn span_multichar_operator() {
    let tokens = tokenize("<=").unwrap();
    assert_eq!(tokens[0].span, Span::new(0, 2));
}

// ── Full SpliceQL query ─────────────────────────────────────────────────────

#[test]
fn full_query() {
    let src = r#"
FROM bam "sample.bam"
WHERE chr = "chr7"
  AND depth > 30
  AND qual > 20
CALL variants
  WITH min_af = 0.05
INTO vcf "output.vcf"
"#;
    assert_eq!(
        kinds(src),
        vec![
            // FROM bam "sample.bam"
            TokenKind::From,
            TokenKind::Bam,
            TokenKind::StringLit("sample.bam".into()),
            // WHERE chr = "chr7"
            TokenKind::Where,
            TokenKind::Ident("chr".into()),
            TokenKind::Eq,
            TokenKind::StringLit("chr7".into()),
            // AND depth > 30
            TokenKind::And,
            TokenKind::Ident("depth".into()),
            TokenKind::Gt,
            TokenKind::IntLit(30),
            // AND qual > 20
            TokenKind::And,
            TokenKind::Ident("qual".into()),
            TokenKind::Gt,
            TokenKind::IntLit(20),
            // CALL variants
            TokenKind::Call,
            TokenKind::Variants,
            // WITH min_af = 0.05
            TokenKind::With,
            TokenKind::Ident("min_af".into()),
            TokenKind::Eq,
            TokenKind::FloatLit(0.05),
            // INTO vcf "output.vcf"
            TokenKind::Into,
            TokenKind::Vcf,
            TokenKind::StringLit("output.vcf".into()),
        ]
    );
}

#[test]
fn complex_query_with_comments() {
    let src = r#"
-- CNV analysis on chromosome 7
FROM bam "tumor.bam"
WHERE chr = "chr7"
CALL cnv
  WITH window_size = 10000,
       amp_threshold = 1.5  -- amplification cutoff
INTO vcf "cnvs.vcf"
LIMIT 100
"#;
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::From,
            TokenKind::Bam,
            TokenKind::StringLit("tumor.bam".into()),
            TokenKind::Where,
            TokenKind::Ident("chr".into()),
            TokenKind::Eq,
            TokenKind::StringLit("chr7".into()),
            TokenKind::Call,
            TokenKind::Cnv,
            TokenKind::With,
            TokenKind::Ident("window_size".into()),
            TokenKind::Eq,
            TokenKind::IntLit(10000),
            TokenKind::Comma,
            TokenKind::Ident("amp_threshold".into()),
            TokenKind::Eq,
            TokenKind::FloatLit(1.5),
            TokenKind::Into,
            TokenKind::Vcf,
            TokenKind::StringLit("cnvs.vcf".into()),
            TokenKind::Limit,
            TokenKind::IntLit(100),
        ]
    );
}

#[test]
fn select_with_expressions() {
    let src = r#"SELECT chr, pos, qual * 2 + 1 FROM bam "reads.bam" ORDER BY qual DESC"#;
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::Select,
            TokenKind::Ident("chr".into()),
            TokenKind::Comma,
            TokenKind::Ident("pos".into()),
            TokenKind::Comma,
            TokenKind::Ident("qual".into()),
            TokenKind::Star,
            TokenKind::IntLit(2),
            TokenKind::Plus,
            TokenKind::IntLit(1),
            TokenKind::From,
            TokenKind::Bam,
            TokenKind::StringLit("reads.bam".into()),
            TokenKind::Order,
            TokenKind::By,
            TokenKind::Ident("qual".into()),
            TokenKind::Desc,
        ]
    );
}

// ── Lexer::next_token streaming usage ───────────────────────────────────────

#[test]
fn streaming_lexer() {
    let mut lexer = Lexer::new("42 + 8");
    let t1 = lexer.next_token().unwrap();
    assert_eq!(t1.kind, TokenKind::IntLit(42));
    let t2 = lexer.next_token().unwrap();
    assert_eq!(t2.kind, TokenKind::Plus);
    let t3 = lexer.next_token().unwrap();
    assert_eq!(t3.kind, TokenKind::IntLit(8));
    let t4 = lexer.next_token().unwrap();
    assert_eq!(t4.kind, TokenKind::Eof);
    // Subsequent calls keep returning Eof.
    let t5 = lexer.next_token().unwrap();
    assert_eq!(t5.kind, TokenKind::Eof);
}
