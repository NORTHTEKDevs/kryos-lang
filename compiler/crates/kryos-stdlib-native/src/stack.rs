//! Stack (LIFO) over caller-owned i64 storage. State: `(top, cap)`.

#[no_mangle]
pub extern "C" fn kryos_stack_init(state: *mut i64, cap: i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 2) };
    s[0] = 0;
    s[1] = cap;
}

#[no_mangle]
pub extern "C" fn kryos_stack_push(buf: *mut i64, state: *mut i64, value: i64) -> i32 {
    if buf.is_null() || state.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 2) };
    let top = s[0] as usize;
    let cap = s[1] as usize;
    if top >= cap {
        return 0;
    }
    let b = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
    b[top] = value;
    s[0] += 1;
    1
}

#[no_mangle]
pub extern "C" fn kryos_stack_pop(buf: *const i64, state: *mut i64, out: *mut i64) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 2) };
    let top = s[0] as usize;
    if top == 0 {
        return 0;
    }
    let cap = s[1] as usize;
    let b = unsafe { std::slice::from_raw_parts(buf, cap) };
    unsafe {
        *out = b[top - 1];
    }
    s[0] -= 1;
    1
}

#[no_mangle]
pub extern "C" fn kryos_stack_peek(buf: *const i64, state: *const i64, out: *mut i64) -> i32 {
    if buf.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(state, 2) };
    let top = s[0] as usize;
    if top == 0 {
        return 0;
    }
    let cap = s[1] as usize;
    let b = unsafe { std::slice::from_raw_parts(buf, cap) };
    unsafe {
        *out = b[top - 1];
    }
    1
}

#[no_mangle]
pub extern "C" fn kryos_stack_len(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    unsafe { *state }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifo_order() {
        let mut buf = [0i64; 4];
        let mut state = [0i64; 2];
        kryos_stack_init(state.as_mut_ptr(), 4);
        kryos_stack_push(buf.as_mut_ptr(), state.as_mut_ptr(), 10);
        kryos_stack_push(buf.as_mut_ptr(), state.as_mut_ptr(), 20);
        kryos_stack_push(buf.as_mut_ptr(), state.as_mut_ptr(), 30);
        assert_eq!(kryos_stack_len(state.as_ptr()), 3);
        let mut out = 0i64;
        kryos_stack_pop(buf.as_ptr(), state.as_mut_ptr(), &mut out);
        assert_eq!(out, 30);
        kryos_stack_pop(buf.as_ptr(), state.as_mut_ptr(), &mut out);
        assert_eq!(out, 20);
        kryos_stack_pop(buf.as_ptr(), state.as_mut_ptr(), &mut out);
        assert_eq!(out, 10);
        assert_eq!(kryos_stack_pop(buf.as_ptr(), state.as_mut_ptr(), &mut out), 0);
    }
}
