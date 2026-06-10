//! Thread-local budget frames for the `@budget` function attribute.
//!
//! A function annotated `@budget(tokens = N, calls = M)` pushes a frame on
//! entry and pops back to its depth on every return. `std::llm` charges the
//! active frames around each model call: one call pre-charged before the
//! request, actual token usage after. Charging decrements EVERY active
//! frame, so an outer budget constrains everything nested under it.
//!
//! Frames are popped by depth (`kryos_budget_pop_to`), which makes unwinding
//! self-healing: if an exception skips an inner pop, the next outer pop
//! truncates past the leaked frames.

use std::cell::RefCell;

#[derive(Clone, Copy)]
struct Frame {
    tokens_left: i64,
    calls_left: i64,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

/// Push a budget frame. Negative limits mean "unlimited" for that axis.
/// Returns the depth BEFORE the push, for `kryos_budget_pop_to`.
#[no_mangle]
pub extern "C" fn kryos_budget_push(tokens: i64, calls: i64) -> i64 {
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let depth = f.len() as i64;
        f.push(Frame {
            tokens_left: if tokens < 0 { i64::MAX } else { tokens },
            calls_left: if calls < 0 { i64::MAX } else { calls },
        });
        depth
    })
}

/// Truncate the frame stack back to `depth` (idempotent; tolerates depths
/// above the current length, which arise after exception unwinds).
#[no_mangle]
pub extern "C" fn kryos_budget_pop_to(depth: i64) {
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let d = depth.max(0) as usize;
        if d < f.len() {
            f.truncate(d);
        }
    })
}

/// 1 when at least one budget frame is active on this thread.
#[no_mangle]
pub extern "C" fn kryos_budget_active() -> i64 {
    FRAMES.with(|f| if f.borrow().is_empty() { 0 } else { 1 })
}

/// Reserve one model call. Returns 0 and decrements every frame when all
/// frames have a call left; returns 1 (and charges nothing) when any frame
/// is out of calls.
#[no_mangle]
pub extern "C" fn kryos_budget_try_call() -> i64 {
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        if f.iter().any(|fr| fr.calls_left <= 0) {
            return 1;
        }
        for fr in f.iter_mut() {
            fr.calls_left -= 1;
        }
        0
    })
}

/// Charge actual token usage against every active frame. Returns 1 when any
/// frame went negative (the budget is now exceeded — callers should throw so
/// agent loops stop), else 0. Usage is recorded either way: post-hoc charges
/// reflect tokens that were genuinely consumed.
#[no_mangle]
pub extern "C" fn kryos_budget_charge_tokens(tokens: i64) -> i64 {
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let mut exceeded = 0;
        for fr in f.iter_mut() {
            fr.tokens_left -= tokens.max(0);
            if fr.tokens_left < 0 {
                exceeded = 1;
            }
        }
        exceeded
    })
}

/// Remaining (tokens, calls) of the tightest active frame, for
/// introspection from Kryos. Returns -1 axes when no frame is active.
#[no_mangle]
pub extern "C" fn kryos_budget_remaining_tokens() -> i64 {
    FRAMES.with(|f| f.borrow().iter().map(|fr| fr.tokens_left).min().unwrap_or(-1))
}

#[no_mangle]
pub extern "C" fn kryos_budget_remaining_calls() -> i64 {
    FRAMES.with(|f| f.borrow().iter().map(|fr| fr.calls_left).min().unwrap_or(-1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_charge_pop() {
        let d = kryos_budget_push(100, 2);
        assert_eq!(d, 0);
        assert_eq!(kryos_budget_active(), 1);
        assert_eq!(kryos_budget_try_call(), 0);
        assert_eq!(kryos_budget_charge_tokens(60), 0);
        assert_eq!(kryos_budget_try_call(), 0);
        assert_eq!(kryos_budget_charge_tokens(60), 1); // 120 > 100
        assert_eq!(kryos_budget_try_call(), 1); // calls exhausted
        kryos_budget_pop_to(d);
        assert_eq!(kryos_budget_active(), 0);
    }

    #[test]
    fn nested_outer_constrains_inner() {
        let d0 = kryos_budget_push(50, 10);
        let d1 = kryos_budget_push(1000, 10);
        assert_eq!(kryos_budget_charge_tokens(60), 1); // outer frame exceeded
        kryos_budget_pop_to(d1);
        kryos_budget_pop_to(d0);
        assert_eq!(kryos_budget_active(), 0);
    }

    #[test]
    fn pop_to_self_heals_after_unwind() {
        let d0 = kryos_budget_push(100, 5);
        let _leaked = kryos_budget_push(10, 1); // imagine a throw skipped this pop
        kryos_budget_pop_to(d0); // outer pop truncates past the leak
        assert_eq!(kryos_budget_active(), 0);
        kryos_budget_pop_to(d0); // idempotent
        assert_eq!(kryos_budget_active(), 0);
    }

    #[test]
    fn unlimited_axes() {
        let d = kryos_budget_push(-1, -1);
        assert_eq!(kryos_budget_try_call(), 0);
        assert_eq!(kryos_budget_charge_tokens(1_000_000_000), 0);
        kryos_budget_pop_to(d);
    }
}
