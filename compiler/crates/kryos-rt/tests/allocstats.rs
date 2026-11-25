//! Exact counting tests for the KRYOS_ALLOC_STATS counters.
//!
//! This is its own test binary and holds a single #[test] so nothing else in
//! the process touches the counters: every assertion below is an EXACT delta.
//! (The lib test binary runs many tests in parallel that allocate arcs and
//! boxes, so exact deltas are impossible there.)

use kryos_rt::allocstats::{force_on_for_tests, note_arc_alloc, snapshot};

#[test]
fn counters_are_exact() {
    force_on_for_tests();
    let s0 = snapshot();

    // arc: alloc +1; retain and a non-final release change nothing; the
    // release that reaches zero is +1 free.
    let a = kryos_rt::arc::kryos_arc_alloc(16, 8);
    let s1 = snapshot();
    assert_eq!(s1.0, s0.0 + 1, "kryos_arc_alloc counts exactly one alloc");
    assert_eq!(s1.1, s0.1);
    kryos_rt::arc::kryos_arc_retain(a);
    kryos_rt::arc::kryos_arc_release(a); // 2 -> 1: not a free
    assert_eq!(snapshot().1, s0.1, "a non-final arc release is not a free");
    kryos_rt::arc::kryos_arc_release(a); // 1 -> 0: the free
    let s2 = snapshot();
    assert_eq!(s2.1, s0.1 + 1, "the final arc release is exactly one free");
    assert_eq!(s2.0, s0.0 + 1);

    // box: calloc +1; a kryos_free that only consumes an extra owner is NOT
    // a free; the last kryos_free is exactly +1.
    let b = kryos_rt::alloc::kryos_calloc(1, 16);
    let s3 = snapshot();
    assert_eq!(s3.2, s2.2 + 1, "kryos_calloc counts exactly one box alloc");
    assert_eq!(s3.3, s2.3);
    kryos_rt::alloc::kryos_struct_retain(b); // adds an extra owner
    kryos_rt::alloc::kryos_free(b); // consumes the extra owner
    let s4 = snapshot();
    assert_eq!(s4.3, s3.3, "consuming an extra owner must NOT count a box free");
    kryos_rt::alloc::kryos_free(b); // last owner: the real free
    let s5 = snapshot();
    assert_eq!(s5.3, s3.3 + 1, "the last kryos_free counts exactly one box free");
    assert_eq!(s5.2, s3.2);

    // Threads: 8 x 1000 relaxed increments are exact.
    let before = snapshot().0;
    let hs: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..1000 {
                    note_arc_alloc();
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(snapshot().0, before + 8000);
}
