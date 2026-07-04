//! Diagnostic allocation accounting (gated by env KRYOS_MEMSTATS=1).
//!
//! Tracks created/freed counts per category and live/peak bytes so we can
//! see whether the self-host compile is leaking (created >> freed) and which
//! category dominates. Near-zero cost when KRYOS_MEMSTATS is unset (single
//! relaxed load + branch on each call).

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};

pub static ARR_NEW: AtomicU64 = AtomicU64::new(0);
pub static ARR_FREE: AtomicU64 = AtomicU64::new(0);
pub static ARR_FREE_CALL: AtomicU64 = AtomicU64::new(0);
pub static ARR_GROW: AtomicU64 = AtomicU64::new(0);
pub static STR_NEW: AtomicU64 = AtomicU64::new(0);
pub static STR_FREE: AtomicU64 = AtomicU64::new(0);
pub static STR_FREE_CALL: AtomicU64 = AtomicU64::new(0);
pub static STRUCT_NEW: AtomicU64 = AtomicU64::new(0);
pub static STRUCT_FREE: AtomicU64 = AtomicU64::new(0);
pub static LIVE: AtomicI64 = AtomicI64::new(0);
pub static PEAK: AtomicI64 = AtomicI64::new(0);
// Cumulative bytes allocated per category (never decremented) -- shows
// where the allocation volume goes regardless of frees.
pub static ARR_B: AtomicU64 = AtomicU64::new(0);
pub static STR_B: AtomicU64 = AtomicU64::new(0);
pub static GROW_B: AtomicU64 = AtomicU64::new(0);

static PROBED: AtomicU8 = AtomicU8::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);

#[inline]
pub fn on() -> bool {
    if PROBED.load(Ordering::Relaxed) == 0 {
        let v = std::env::var_os("KRYOS_MEMSTATS").is_some();
        ENABLED.store(v, Ordering::Relaxed);
        PROBED.store(1, Ordering::Relaxed);
    }
    ENABLED.load(Ordering::Relaxed)
}

#[inline]
pub fn add_bytes(d: i64) {
    let n = LIVE.fetch_add(d, Ordering::Relaxed) + d;
    let mut pk = PEAK.load(Ordering::Relaxed);
    while n > pk {
        match PEAK.compare_exchange_weak(pk, n, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(x) => pk = x,
        }
    }
}

/// Called from the hot allocation paths. Cheap when disabled.
#[inline]
pub fn note_arr_new(bytes: i64) {
    if !on() {
        return;
    }
    let c = ARR_NEW.fetch_add(1, Ordering::Relaxed) + 1;
    ARR_B.fetch_add(bytes as u64, Ordering::Relaxed);
    add_bytes(bytes);
    if c % 500_000 == 0 {
        dump();
    }
}

#[inline]
pub fn note_arr_grow(net_bytes: i64) {
    if !on() {
        return;
    }
    ARR_GROW.fetch_add(1, Ordering::Relaxed);
    GROW_B.fetch_add(net_bytes as u64, Ordering::Relaxed);
    add_bytes(net_bytes);
}

#[inline]
pub fn note_arr_free(bytes: i64) {
    if !on() {
        return;
    }
    ARR_FREE.fetch_add(1, Ordering::Relaxed);
    add_bytes(-bytes);
}

#[inline]
pub fn note_arr_free_call() {
    if !on() {
        return;
    }
    ARR_FREE_CALL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn note_str_free_call() {
    if !on() {
        return;
    }
    STR_FREE_CALL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn note_str_new(bytes: i64) {
    if !on() {
        return;
    }
    let c = STR_NEW.fetch_add(1, Ordering::Relaxed) + 1;
    STR_B.fetch_add(bytes as u64, Ordering::Relaxed);
    add_bytes(bytes);
    // Periodic dump on string-heavy workloads (arrays already had one).
    if c % 500_000 == 0 {
        dump();
    }
}

#[inline]
pub fn note_str_free(bytes: i64) {
    if !on() {
        return;
    }
    STR_FREE.fetch_add(1, Ordering::Relaxed);
    add_bytes(-bytes);
}

#[inline]
pub fn note_struct_new(bytes: i64) {
    if !on() {
        return;
    }
    STRUCT_NEW.fetch_add(1, Ordering::Relaxed);
    add_bytes(bytes);
}

#[inline]
pub fn note_struct_free(bytes: i64) {
    if !on() {
        return;
    }
    STRUCT_FREE.fetch_add(1, Ordering::Relaxed);
    add_bytes(-bytes);
}

pub static BIG_CNT: AtomicU64 = AtomicU64::new(0);
pub static BIG_MAX: AtomicU64 = AtomicU64::new(0);

/// Record an array created with a suspiciously large capacity. Logs the
/// first ~30 with their raw cap so a garbage-capacity miscompile is visible.
#[inline]
pub fn note_big_array(cap: i64) {
    if !on() {
        return;
    }
    let c = BIG_CNT.fetch_add(1, Ordering::Relaxed) + 1;
    let cu = cap.max(0) as u64;
    let mut m = BIG_MAX.load(Ordering::Relaxed);
    while cu > m {
        match BIG_MAX.compare_exchange_weak(m, cu, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(x) => m = x,
        }
    }
    if c <= 30 {
        eprintln!("[memstats] BIG array_new #{c}: cap={cap} (~{}MB)", (cu * 8) / 1_048_576);
    }
}

pub fn dump() {
    eprintln!(
        "[memstats] arr_new={} arr_freeCALL={} arr_dealloc={} arr_grow={} | str_new={} str_freeCALL={} str_dealloc={} | struct_new={} | live={}MB peak={}MB",
        ARR_NEW.load(Ordering::Relaxed),
        ARR_FREE_CALL.load(Ordering::Relaxed),
        ARR_FREE.load(Ordering::Relaxed),
        ARR_GROW.load(Ordering::Relaxed),
        STR_NEW.load(Ordering::Relaxed),
        STR_FREE_CALL.load(Ordering::Relaxed),
        STR_FREE.load(Ordering::Relaxed),
        STRUCT_NEW.load(Ordering::Relaxed),
        LIVE.load(Ordering::Relaxed) / 1_048_576,
        PEAK.load(Ordering::Relaxed) / 1_048_576,
    );
    eprintln!(
        "[memstats]   bytes: arr_new={}MB grow={}MB str_new={}MB | big_arrays={} max_cap={}",
        ARR_B.load(Ordering::Relaxed) / 1_048_576,
        GROW_B.load(Ordering::Relaxed) / 1_048_576,
        STR_B.load(Ordering::Relaxed) / 1_048_576,
        BIG_CNT.load(Ordering::Relaxed),
        BIG_MAX.load(Ordering::Relaxed),
    );
}
