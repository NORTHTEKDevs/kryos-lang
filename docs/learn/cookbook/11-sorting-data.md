# Cookbook 11 · Sorting + binary search

`std::sort` provides in-place Timsort, descending reverse, binary search, and an is-sorted check.

## The program

```kryos
use std::sort::{sort_i64, sort_i64_reverse, bsearch_i64, is_sorted_i64}

fn main() {
    let mut nums: [i64] = [5, 3, 8, 1, 4, 1, 9, 2]

    sort_i64(nums)
    print("ascending:  ")
    print_arr(nums)

    sort_i64_reverse(nums)
    print("descending: ")
    print_arr(nums)

    // Re-sort then search.
    sort_i64(nums)
    let pos = bsearch_i64(nums, 4)
    println("4 is at index " + to_string(pos))
    let missing = bsearch_i64(nums, 99)
    println("99 is at index " + to_string(missing))  // -1

    println("sorted? " + to_string(is_sorted_i64(nums)))
}

fn print_arr(a: [i64]) {
    let mut s: str = "["
    let n = len(a)
    let mut i: i64 = 0
    while i < n {
        if i > 0 { s = s + ", " }
        s = s + to_string(a[i])
        i = i + 1
    }
    s = s + "]"
    println(s)
}
```

## Things to know

- `sort_i64` is **in-place** — pass `let mut` arrays.
- For descending: call `sort_i64` then `sort_i64_reverse`.
- `bsearch_i64` returns `-1` on miss — *always* check.
- The sort is unstable (Rust's `sort_unstable`); equal keys may reorder.
