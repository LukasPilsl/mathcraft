//! Work budget: a count of elementary rational operations per top-level call.
//!
//! Every public entry point that can do heavy work opens a [`Scope`]. The outermost scope
//! resets the counter; nested scopes (and every internal helper) share it. Hot loops call
//! [`spend`], which fails with `TooLarge("work budget")` once [`WORK_LIMIT`] is passed. The
//! limit is per call, so a long-running session is never starved by earlier calls.

use std::cell::Cell;

use super::SymError;

/// Largest number of charged operations in one top-level call. Measured in a debug build
/// (see `tests/budget.rs`): a dense numeric n x n system costs about 4.7 n^3 units (20x20:
/// 38k units, 0.8 s; 60x60: 992k units, 26 s without the cap). With 100 000, numeric systems
/// up to about 25 x 25 succeed and larger ones are refused after a few seconds; a symbolic
/// 6 x 6 system is refused in about 1.5 s. Symbolic units cost more than numeric ones.
pub(crate) const WORK_LIMIT: u64 = 100_000;

thread_local! {
    static WORK: Cell<u64> = const { Cell::new(0) };
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Marks one top-level call. Dropping it leaves the call.
pub(crate) struct Scope;

impl Scope {
    /// Enters a call; the outermost one resets the counter.
    pub(crate) fn enter() -> Scope {
        DEPTH.with(|d| {
            if d.get() == 0 {
                WORK.with(|w| w.set(0));
            }
            d.set(d.get() + 1);
        });
        Scope
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Charges `units` operations; `TooLarge` once the call's total passes [`WORK_LIMIT`].
pub(crate) fn spend(units: u64) -> Result<(), SymError> {
    WORK.with(|w| {
        let total = w.get().saturating_add(units);
        w.set(total);
        if total > WORK_LIMIT {
            Err(SymError::TooLarge("work budget"))
        } else {
            Ok(())
        }
    })
}
