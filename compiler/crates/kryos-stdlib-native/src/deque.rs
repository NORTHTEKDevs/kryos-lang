//! Double-ended queue (deque) over a caller-owned ring buffer.
//!
//! State: `[head, tail, cap, count]` (4 i64). Push/pop at either end in O(1).
//! Reuses the queue's wraparound math; adds front-side push/pop.

#[no_mangle]
pub extern "C" fn kryos_deque_init(state: *mut i64, cap: i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    s[0] = 0;
    s[1] = 0;
    s[2] = cap;
    s[3] = 0;
}

/// Push at the back. Returns 1 success, 0 if full.
#[no_mangle]
pub extern "C" fn kryos_deque_push_back(
    buf: *mut i64,
    state: *mut i64,
    value: i64,
) -> i32 {
    if buf.is_null() || state.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    let cap = s[2] as usize;
    let count = s[3] as usize;
    if cap == 0 || count >= cap {
        return 0;
    }
    let tail = s[1] as usize;
    let b = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
    b[tail] = value;
    s[1] = ((tail + 1) % cap) as i64;
    s[3] += 1;
    1
}

/// Push at the front. Returns 1 success, 0 if full.
#[no_mangle]
pub extern "C" fn kryos_deque_push_front(
    buf: *mut i64,
    state: *mut i64,
    value: i64,
) -> i32 {
    if buf.is_null() || state.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    let cap = s[2] as usize;
    let count = s[3] as usize;
    if cap == 0 || count >= cap {
        return 0;
    }
    let head = s[0] as usize;
    let new_head = (head + cap - 1) % cap;
    let b = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
    b[new_head] = value;
    s[0] = new_head as i64;
    s[3] += 1;
    1
}

#[no_mangle]
pub extern "C" fn kryos_deque_pop_front(
    buf: *mut i64,
    state: *mut i64,
    out: *mut i64,
) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    let count = s[3] as usize;
    if count == 0 {
        return 0;
    }
    let cap = s[2] as usize;
    let head = s[0] as usize;
    let b = unsafe { std::slice::from_raw_parts(buf, cap) };
    unsafe {
        *out = b[head];
    }
    s[0] = ((head + 1) % cap) as i64;
    s[3] -= 1;
    1
}

#[no_mangle]
pub extern "C" fn kryos_deque_pop_back(
    buf: *mut i64,
    state: *mut i64,
    out: *mut i64,
) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    let count = s[3] as usize;
    if count == 0 {
        return 0;
    }
    let cap = s[2] as usize;
    let tail = s[1] as usize;
    let new_tail = (tail + cap - 1) % cap;
    let b = unsafe { std::slice::from_raw_parts(buf, cap) };
    unsafe {
        *out = b[new_tail];
    }
    s[1] = new_tail as i64;
    s[3] -= 1;
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_via_back_push_front_pop() {
        let mut buf = [0i64; 5];
        let mut state = [0i64; 4];
        kryos_deque_init(state.as_mut_ptr(), 5);
        for v in [1i64, 2, 3] {
            kryos_deque_push_back(buf.as_mut_ptr(), state.as_mut_ptr(), v);
        }
        let mut out = 0i64;
        for expected in [1i64, 2, 3] {
            kryos_deque_pop_front(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn lifo_via_back_push_back_pop() {
        let mut buf = [0i64; 5];
        let mut state = [0i64; 4];
        kryos_deque_init(state.as_mut_ptr(), 5);
        for v in [1i64, 2, 3] {
            kryos_deque_push_back(buf.as_mut_ptr(), state.as_mut_ptr(), v);
        }
        let mut out = 0i64;
        for expected in [3i64, 2, 1] {
            kryos_deque_pop_back(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn front_push_reverses_view() {
        let mut buf = [0i64; 5];
        let mut state = [0i64; 4];
        kryos_deque_init(state.as_mut_ptr(), 5);
        for v in [1i64, 2, 3] {
            kryos_deque_push_front(buf.as_mut_ptr(), state.as_mut_ptr(), v);
        }
        let mut out = 0i64;
        // After push_front 1,2,3 the deque is [3, 2, 1].
        // pop_front gives 3, 2, 1.
        for expected in [3i64, 2, 1] {
            kryos_deque_pop_front(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
            assert_eq!(out, expected);
        }
    }
}
