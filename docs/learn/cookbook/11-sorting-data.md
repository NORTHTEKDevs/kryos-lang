# Cookbook 11 · Sorting + binary search

The global builtins `sort(arr)` and `reverse(arr)` sort/reverse an array in place. There's no built-in binary search or is-sorted check — this recipe writes both by hand (a few lines each).

## The program

```kryos
// sort(arr) and reverse(arr) are builtins — no import needed.

// Binary search: returns index of target, or -1 if not found.
// Array must be sorted in ascending order.
fn bsearch(a: [i64], target: i64) -> i64 {
    let mut lo: i64 = 0
    let mut hi: i64 = len(a) - 1
    while lo <= hi {
        let mid = lo + (hi - lo) / 2
        if a[mid] == target { return mid }
        if a[mid] < target {
            lo = mid + 1
        } else {
            hi = mid - 1
        }
    }
    return -1
}

// Check if an array is sorted in ascending order.
fn is_sorted(a: [i64]) -> bool {
    let n = len(a)
    let mut i: i64 = 1
    while i < n {
        if a[i] < a[i - 1] { return false }
        i = i + 1
    }
    return true
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

fn main() {
    let mut nums: [i64] = [5, 3, 8, 1, 4, 1, 9, 2]

    sort(nums)
    print("ascending:  ")
    print_arr(nums)

    reverse(nums)
    print("descending: ")
    print_arr(nums)

    // Re-sort then search.
    sort(nums)
    let pos = bsearch(nums, 4)
    println("4 is at index " + to_string(pos))
    let missing = bsearch(nums, 99)
    println("99 is at index " + to_string(missing))  // -1

    println("sorted? " + to_string(is_sorted(nums)))
}
```

## Things to know

- `sort(arr)` is **in-place and void** — pass `let mut` arrays and call it as a statement (`sort(nums)`), not `nums = sort(nums)`.
- For descending: call `sort(arr)` then `reverse(arr)` (also in-place and void).
- The hand-rolled `bsearch` above returns `-1` on miss — *always* check.
- `std::iter::sort<T>(arr) -> [T]` is a separate, non-mutating generic sort that returns a new array — importing it (`use std::iter::{sort}`) shadows the bare builtin's in-place behavior, so don't mix the two in the same file.
