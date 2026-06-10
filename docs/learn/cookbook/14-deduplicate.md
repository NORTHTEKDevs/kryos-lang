# Cookbook 14 · Deduplicate, reverse, aggregate

`std::collections` ships in-place ops over `[i64]`: dedup of sorted slices,
reverse, sum/min/max. Combined with `std::sort` you can build the common
data-pipeline patterns without a hash set.

## The program

```kryos
// sort, reverse, min, max are builtins — no import needed.

// Remove consecutive duplicates from a sorted array.
fn dedup_sorted(nums: [i64]) -> [i64] {
    let n = len(nums)
    if n == 0 { return nums }
    let mut out: [i64] = [nums[0]]
    let mut i: i64 = 1
    while i < n {
        if nums[i] != nums[i - 1] {
            out = push(out, nums[i])
        }
        i = i + 1
    }
    return out
}

// Sum all elements.
fn sum(nums: [i64]) -> i64 {
    let mut acc: i64 = 0
    for x in nums { acc = acc + x }
    return acc
}

@capabilities(io)
fn main() {
    let mut nums: [i64] = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9]

    // Sort, then drop consecutive duplicates.
    sort(nums)
    let unique = dedup_sorted(nums)

    let mut lo = unique[0]
    let mut hi = unique[0]
    for x in unique {
        if x < lo { lo = x }
        if x > hi { hi = x }
    }

    println("unique count: " + to_string(len(unique)))
    println("min: " + to_string(lo))
    println("max: " + to_string(hi))
    println("sum: " + to_string(sum(unique)))

    // Reverse for descending order.
    let mut desc = unique
    reverse(desc)
    print_arr(desc)
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

- `dedup_sorted` above requires the slice to be **already sorted**. Otherwise
  it only removes consecutive equals.
- `sort(arr)` and `reverse(arr)` are in-place builtins — pass `let mut` arrays.
- For aggregates (sum, min, max) the stdlib has no array-level helpers; inline
  a `for x in arr` loop as shown above. Check `len > 0` before accessing `arr[0]`.
