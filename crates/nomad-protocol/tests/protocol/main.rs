//! Protocol tests (Design doc 20 §11, D-089): round trips of every message variant, and
//! rejection of malformed input without a panic (doc 20 §9, `CODING_STANDARDS.md` §5).

mod malformed;
mod round_trip;
mod samples;
