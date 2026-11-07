//! The Strategist (doc 13 §5; D-078; A2, A4).
//!
//! It runs in phase 3 while its commander is online, every 30, 15 or 10 s by difficulty and
//! staggered by commander (D-079, D-203, D-229). A2 gives it goals and plans; until then the
//! simulation calls it on schedule and it issues nothing, so the call order is in place
//! (A1 plan §8 task 1).

use crate::Output;
use crate::view::View;

/// One evaluation of the Strategist. Issues nothing until A2.
pub fn evaluate(view: &impl View, out: &mut Output) {
    let _ = (view, out);
}
