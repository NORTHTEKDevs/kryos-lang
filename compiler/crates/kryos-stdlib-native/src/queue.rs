//! Ring-buffer queue (FIFO) over caller-owned i64 storage.
//!
//! The queue state is two i64 head/tail indices the caller keeps next
//! to the backing array. Operations are O(1), allocation-free.
//!
//! Layout:
//!   state[0] = head     (consumer pointer)
//!   state[1] = tail     (producer pointer)
//!   state[2] = capacity (immutable; must equal `buf` length)
//!   state[3] = count    (current logical size)

/// Push `value`. Returns 1 on success, 0 if full.
#[no_mangle]
pub extern "C" fn kryos_queue_push(
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

/// Pop the front value. Writes to `*out`. Returns 1 on success, 0 if empty.
#[no_mangle]
pub extern "C" fn kryos_queue_pop(
    buf: *mut i64,
    state: *mut i64,
    out: *mut i64,
) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    let cap = s[2] as usize;
    let count = s[3] as usize;
    if count == 0 {
        return 0;
    }
    let head = s[0] as usize;
    let b = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
    unsafe {
        *out = b[head];
    }
    s[0] = ((head + 1) % cap) as i64;
    s[3] -= 1;
    1
}

/// Peek the front value without removing. Returns 1 if there was a value, 0 if empty.
#[no_mangle]
pub extern "C" fn kryos_queue_peek(buf: *const i64, state: *const i64, out: *mut i64) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(state, 4) };
    if s[3] == 0 {
        return 0;
    }
    let head = s[0] as usize;
    let cap = s[2] as usize;
    let b = unsafe { std::slice::from_raw_parts(buf, cap) };
    unsafe {
        *out = b[head];
    }
    1
}

/// Initialize a state block: head=0, tail=0, capacity=cap, count=0.
#[no_mangle]
pub extern "C" fn kryos_queue_init(state: *mut i64, cap: i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    s[0] = 0;
    s[1] = 0;
    s[2] = cap;
    s[3] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_q(cap: usize) -> (Vec<i64>, Vec<i64>) {
        let buf = vec![0i64; cap];
        let mut state = vec![0i64; 4];
        kryos_queue_init(state.as_mut_ptr(), cap as i64);
        (buf, state)
    }

    #[test]
    fn fifo_order() {
        let (mut buf, mut state) = new_q(5);
        for v in [1i64, 2, 3] {
            kryos_queue_push(buf.as_mut_ptr(), state.as_mut_ptr(), v);
        }
        let mut out = 0i64;
        for expected in [1i64, 2, 3] {
            assert_eq!(
                kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out),
                1
            );
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn full_pop_empty() {
        let (mut buf, mut state) = new_q(2);
        kryos_queue_push(buf.as_mut_ptr(), state.as_mut_ptr(), 10);
        kryos_queue_push(buf.as_mut_ptr(), state.as_mut_ptr(), 20);
        // Full now
        assert_eq!(kryos_queue_push(buf.as_mut_ptr(), state.as_mut_ptr(), 30), 0);
        // Drain
        let mut out = 0i64;
        kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
        kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
        // Empty
        assert_eq!(
            kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out),
            0
        );
    }

    #[test]
    fn wrap_around() {
        let (mut buf, mut state) = new_q(3);
        for v in [1i64, 2, 3] {
            kryos_queue_push(buf.as_mut_ptr(), state.as_mut_ptr(), v);
        }
        let mut out = 0i64;
        kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
        assert_eq!(out, 1);
        kryos_queue_push(buf.as_mut_ptr(), state.as_mut_ptr(), 4);
        // Order should be 2, 3, 4 now
        kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
        assert_eq!(out, 2);
        kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
        assert_eq!(out, 3);
        kryos_queue_pop(buf.as_mut_ptr(), state.as_mut_ptr(), &mut out);
        assert_eq!(out, 4);
    }
}
