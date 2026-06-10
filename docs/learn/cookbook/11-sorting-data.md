# Cookbook 11 · Sorting + binary search

`std::sort` provides in-place Timsort, descending reverse, binary search, and an is-sorted check.

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

- `sort_i64` is **in-place** — pass `let mut` arrays.
- For descending: call `sort_i64` then `sort_i64_reverse`.
- `bsearch_i64` returns `-1` on miss — *always* check.
- The sort is unstable (Rust's `sort_unstable`); equal keys may reorder.
