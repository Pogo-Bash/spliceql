#![no_main]

//! Fuzz target for the SpliceQL parser.
//!
//! Invariant: `spliceql::parse` must be **total** over all inputs — it may
//! return `Ok` or `Err`, but must never panic, abort, or run away.  Mirrors the
//! `fuzz_lexer` harness; since `parse` lexes internally, this also re-exercises
//! the lexer on the same corpus.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The parser operates on `&str`; only feed it valid UTF-8.  Invalid byte
    // sequences are the lexer's concern and aren't interesting here.
    if let Ok(source) = std::str::from_utf8(data) {
        // Result is intentionally discarded: we only assert the absence of a
        // panic.  Both `Ok(Query)` and `Err(ParseError)` are acceptable.
        let _ = spliceql::parse(source);
    }
});
