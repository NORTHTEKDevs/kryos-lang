//! Binary min-heap (priority queue) over caller-owned i64 storage.
//!
//! State: `(len, cap)`. Op cost O(log n). Use for Dijkstra, A*,
//! scheduling, top-k selection, anywhere you need "smallest first"
//! without a full sort.
//!
//! For a max-heap, push the negated value and negate again on pop.

/// Initialize `state` with len=0, cap=`cap`.
#[no_mangle]
pub extern "C" fn kryos_heap_init(state: *mut i64, cap: i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 2) };
    s[0] = 0;
    s[1] = cap;
}

/// Push `v` onto the heap. Returns 1 on success, 0 if full.
#[no_mangle]
pub extern "C" fn kryos_heap_push(buf: *mut i64, state: *mut i64, v: i64) -> i32 {
    if buf.is_null() || state.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 2) };
    let mut n = s[0] as usize;
    let cap = s[1] as usize;
    if n >= cap {
        return 0;
    }
    let b = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
    b[n] = v;
    n += 1;
    // sift up
    let mut i = n - 1;
    while i > 0 {
        let parent = (i - 1) / 2;
        if b[parent] <= b[i] {
            break;
        }
        b.swap(parent, i);
        i = parent;
    }
    s[0] = n as i64;
    1
}

/// Pop the minimum. Writes to `*out`. Returns 1 on success, 0 if empty.
#[no_mangle]
pub extern "C" fn kryos_heap_pop_min(
    buf: *mut i64,
    state: *mut i64,
    out: *mut i64,
) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 2) };
    let n = s[0] as usize;
    if n == 0 {
        return 0;
    }
    let cap = s[1] as usize;
    let b = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
    let min = b[0];
    let last = n - 1;
    b[0] = b[last];
    let n = last;
    // sift down
    let mut i = 0;
    loop {
        let l = 2 * i + 1;
        let r = 2 * i + 2;
        let mut smallest = i;
        if l < n && b[l] < b[smallest] {
            smallest = l;
        }
        if r < n && b[r] < b[smallest] {
            smallest = r;
        }
        if smallest == i {
            break;
        }
        b.swap(i, smallest);
        i = smallest;
    }
    unsafe {
        *out = min;
    }
    s[0] = n as i64;
    1
}

/// Peek the minimum without removing. Returns 1 if a value was written, 0 if empty.
#[no_mangle]
pub extern "C" fn kryos_heap_peek_min(
    buf: *const i64,
    state: *const i64,
    out: *mut i64,
) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(state, 2) };
    if s[0] == 0 {
        return 0;
    }
    unsafe {
        *out = *buf;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_in_ascending_order() {
        let mut buf = [0i64; 16];
        let mut state = [0i64; 2];
        kryos_heap_init(state.as_mut_ptr(), 16);
        for v in [5i64, 3, 8, 1, 9, 4, 7, 2] {
            kryos_heap_push(buf.as_mut_ptr(), state.as_mut_ptr(), v);
        }
        let mut out = 0i64;
        let mut sorted: Vec<i64> = Vec::new();
        while kryos_heap_pop_min(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out) == 1 {
            sorted.push(out);
        }
        assert_eq!(sorted, vec![1, 2, 3, 4, 5, 7, 8, 9]);
    }

    #[test]
    fn peek_does_not_remove() {
        let mut buf = [0i64; 4];
        let mut state = [0i64; 2];
        kryos_heap_init(state.as_mut_ptr(), 4);
        kryos_heap_push(buf.as_mut_ptr(), state.as_mut_ptr(), 7);
        kryos_heap_push(buf.as_mut_ptr(), state.as_mut_ptr(), 3);
        let mut out = 0i64;
        kryos_heap_peek_min(buf.as_ptr(), state.as_ptr(), &mut out);
        assert_eq!(out, 3);
        assert_eq!(state[0], 2);
    }

    #[test]
    fn full_returns_zero() {
        let mut buf = [0i64; 2];
        let mut state = [0i64; 2];
        kryos_heap_init(state.as_mut_ptr(), 2);
        kryos_heap_push(buf.as_mut_ptr(), state.as_mut_ptr(), 1);
        kryos_heap_push(buf.as_mut_ptr(), state.as_mut_ptr(), 2);
        assert_eq!(
            kryos_heap_push(buf.as_mut_ptr(), state.as_mut_ptr(), 3),
            0
        );
    }
}
