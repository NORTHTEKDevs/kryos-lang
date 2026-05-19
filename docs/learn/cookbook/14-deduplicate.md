# Cookbook 14 · Deduplicate, reverse, aggregate

`std::collections` ships in-place ops over `[i64]`: dedup of sorted slices,
reverse, sum/min/max. Combined with `std::sort` you can build the common
data-pipeline patterns without a hash set.

## The program

```kryos
use std::sort::sort_i64
use std::collections::{dedup_sorted_i64, reverse_i64, sum_i64, min_i64, max_i64}

@capabilities(io)
fn main() {
    let mut nums: [i64] = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9]

    // Sort, then drop consecutive duplicates.
    sort_i64(nums)
    let new_len = dedup_sorted_i64(nums)
    println("unique count: " + to_string(new_len))
    // Slice off the trailing garbage:
    let unique = substr_arr(nums, 0, new_len)

    println("min: " + to_string(min_i64(unique)))
    println("max: " + to_string(max_i64(unique)))
    println("sum: " + to_string(sum_i64(unique)))

    // Reverse for descending order.
    reverse_i64(unique)
    print("descending: ")
    print_arr(unique)
}

fn substr_arr(src: [i64], start: i64, end: i64) -> [i64] {
    let mut out: [i64] = []
    let mut i = start
    while i < end {
        out = push(out, src[i])
        i = i + 1
    }
    return out
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

- `dedup_sorted_i64` requires the slice to be **already sorted**. Otherwise
  it only removes consecutive equals.
- `sum_i64` saturates on overflow (caps at i64::MAX, not wrap).
- `min_i64` / `max_i64` return sentinels (`i64::MAX` / `i64::MIN`) on empty
  input — always check `len > 0` first if that's a problem.
