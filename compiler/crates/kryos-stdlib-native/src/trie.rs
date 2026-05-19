//! Compact ASCII prefix tree (trie). Backed by a Rust-side `Box`-ed
//! tree node; FFI exposes opaque pointers.
//!
//! Useful for autocomplete, dictionary checks, and longest-prefix-match
//! routing tables. ASCII-only (lowercase preferred); for full Unicode
//! support build atop `std::utf8`.

use std::collections::HashMap;

pub struct TrieNode {
    is_word: bool,
    children: HashMap<u8, Box<TrieNode>>,
}

impl TrieNode {
    fn new() -> Self {
        Self { is_word: false, children: HashMap::new() }
    }
}

/// Create a new empty trie. Returns an opaque handle.
#[no_mangle]
pub extern "C" fn kryos_trie_new() -> *mut u8 {
    Box::into_raw(Box::new(TrieNode::new())) as *mut u8
}

/// Insert a word (UTF-8 bytes interpreted as ASCII octets).
#[no_mangle]
pub extern "C" fn kryos_trie_insert(handle: *mut u8, word: *const u8, len: usize) {
    if handle.is_null() || word.is_null() {
        return;
    }
    let root = unsafe { &mut *(handle as *mut TrieNode) };
    let bytes = unsafe { std::slice::from_raw_parts(word, len) };
    let mut node = root;
    for &b in bytes {
        node = node.children.entry(b).or_insert_with(|| Box::new(TrieNode::new()));
    }
    node.is_word = true;
}

/// Test exact-word membership. Returns 1 if present, 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_trie_contains(handle: *const u8, word: *const u8, len: usize) -> i32 {
    if handle.is_null() || word.is_null() {
        return 0;
    }
    let root = unsafe { &*(handle as *const TrieNode) };
    let bytes = unsafe { std::slice::from_raw_parts(word, len) };
    let mut node = root;
    for &b in bytes {
        node = match node.children.get(&b) {
            Some(n) => n,
            None => return 0,
    };
    }
    if node.is_word {
        1
    } else {
        0
    }
}

/// Test whether any word starts with `prefix`. Returns 1 if yes.
#[no_mangle]
pub extern "C" fn kryos_trie_has_prefix(handle: *const u8, prefix: *const u8, len: usize) -> i32 {
    if handle.is_null() || prefix.is_null() {
        return 0;
    }
    let root = unsafe { &*(handle as *const TrieNode) };
    let bytes = unsafe { std::slice::from_raw_parts(prefix, len) };
    let mut node = root;
    for &b in bytes {
        node = match node.children.get(&b) {
            Some(n) => n,
            None => return 0,
        };
    }
    1
}

/// Free a trie.
#[no_mangle]
pub extern "C" fn kryos_trie_drop(handle: *mut u8) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle as *mut TrieNode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_contain() {
        let t = kryos_trie_new();
        kryos_trie_insert(t, b"hello".as_ptr(), 5);
        kryos_trie_insert(t, b"world".as_ptr(), 5);
        kryos_trie_insert(t, b"help".as_ptr(), 4);

        assert_eq!(kryos_trie_contains(t, b"hello".as_ptr(), 5), 1);
        assert_eq!(kryos_trie_contains(t, b"world".as_ptr(), 5), 1);
        assert_eq!(kryos_trie_contains(t, b"hel".as_ptr(), 3), 0);
        assert_eq!(kryos_trie_contains(t, b"helping".as_ptr(), 7), 0);

        // Prefix check
        assert_eq!(kryos_trie_has_prefix(t, b"hel".as_ptr(), 3), 1);
        assert_eq!(kryos_trie_has_prefix(t, b"xyz".as_ptr(), 3), 0);

        kryos_trie_drop(t);
    }

    #[test]
    fn empty_string_is_word_when_inserted() {
        let t = kryos_trie_new();
        kryos_trie_insert(t, b"".as_ptr(), 0);
        assert_eq!(kryos_trie_contains(t, b"".as_ptr(), 0), 1);
        kryos_trie_drop(t);
    }
}
