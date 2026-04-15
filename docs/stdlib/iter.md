# std::iter

Functional-style iteration over arrays: generators, transformations, reductions, sorting, searching, and grouping. All functions operate on plain Kryos arrays and return new arrays or scalar values.

```kryos
use std::iter
```

---

## Generators

### range

`range(start: i64, end: i64) -> [i64]`

Return an array of integers from `start` up to but not including `end`.

**Example:**
```kryos
use std::iter

println(range(0, 5))     // [0, 1, 2, 3, 4]
println(range(3, 7))     // [3, 4, 5, 6]
```

---

### range_step

`range_step(start: i64, end: i64, step: i64) -> [i64]`

Return an array of integers from `start` to `end` (exclusive) advancing by `step`.

**Example:**
```kryos
use std::iter

println(range_step(0, 10, 2))   // [0, 2, 4, 6, 8]
println(range_step(10, 0, -2))  // [10, 8, 6, 4, 2]
```

---

## Indexing

### enumerate

`enumerate(arr: [any]) -> [any]`

Return an array of two-element arrays `[index, value]` for each element.

**Example:**
```kryos
use std::iter

let pairs = enumerate(["a", "b", "c"])
// [[0, "a"], [1, "b"], [2, "c"]]
```

---

### zip

`zip(a: [any], b: [any]) -> [any]`

Pair corresponding elements from `a` and `b` into two-element arrays. Stops at the shorter array.

**Example:**
```kryos
use std::iter

let zipped = zip([1, 2, 3], ["a", "b", "c"])
// [[1, "a"], [2, "b"], [3, "c"]]
```

---

### unzip

`unzip(pairs: [any]) -> [any]`

Split an array of two-element arrays into two separate arrays. Returns `[left_array, right_array]`.

**Example:**
```kryos
use std::iter

let result = unzip([[1, "a"], [2, "b"], [3, "c"]])
// [[1, 2, 3], ["a", "b", "c"]]
```

---

## Transforming

### map

`map(arr: [any], f: fn(any) -> any) -> [any]`

Apply `f` to each element and return a new array of the results.

**Example:**
```kryos
use std::iter

let doubled = map([1, 2, 3, 4], fn(x: i64) -> i64 { return x * 2 })
println(doubled)   // [2, 4, 6, 8]
```

---

### map_indexed

`map_indexed(arr: [any], f: fn(i64, any) -> any) -> [any]`

Apply `f(index, element)` to each element and return a new array of the results.

**Example:**
```kryos
use std::iter

let labelled = map_indexed(["a", "b", "c"], fn(i: i64, v: str) -> str {
    return i + ":" + v
})
// ["0:a", "1:b", "2:c"]
```

---

### filter

`filter(arr: [any], pred: fn(any) -> bool) -> [any]`

Return a new array containing only the elements for which `pred` returns `true`.

**Example:**
```kryos
use std::iter

let evens = filter([1, 2, 3, 4, 5, 6], fn(x: i64) -> bool { return x % 2 == 0 })
println(evens)   // [2, 4, 6]
```

---

### flat_map

`flat_map(arr: [any], f: fn(any) -> [any]) -> [any]`

Apply `f` to each element (which must return an array) and flatten the results into a single array.

**Example:**
```kryos
use std::iter

let expanded = flat_map([1, 2, 3], fn(x: i64) -> [i64] { return [x, x * 10] })
println(expanded)   // [1, 10, 2, 20, 3, 30]
```

---

## Reducing

### reduce

`reduce(arr: [any], init: any, f: fn(any, any) -> any) -> any`

Reduce `arr` to a single value by applying `f(accumulator, element)` left to right, starting from `init`.

**Example:**
```kryos
use std::iter

let total = reduce([1, 2, 3, 4, 5], 0, fn(acc: i64, x: i64) -> i64 { return acc + x })
println(total)   // 15
```

---

### fold

`fold(arr: [any], init: any, f: fn(any, any) -> any) -> any`

Alias for `reduce`.

---

### fold_right

`fold_right(arr: [any], init: any, f: fn(any, any) -> any) -> any`

Reduce from right to left.

---

### for_each

`for_each(arr: [any], f: fn(any))`

Call `f` for each element. Does not return a value.

**Example:**
```kryos
use std::iter

for_each([1, 2, 3], fn(x: i64) {
    println(x)
})
```

---

## Searching

### any

`any(arr: [any], pred: fn(any) -> bool) -> bool`

Return `true` if at least one element satisfies `pred`. Short-circuits on the first match.

---

### all

`all(arr: [any], pred: fn(any) -> bool) -> bool`

Return `true` if every element satisfies `pred`. Short-circuits on the first failure.

---

### find

`find(arr: [any], pred: fn(any) -> bool) -> any`

Return the first element satisfying `pred` as `{"tag": "Some", "value": v}`, or `{"tag": "None"}` if not found.

**Example:**
```kryos
use std::iter

let result = find([1, 2, 3, 4], fn(x: i64) -> bool { return x > 2 })
// {"tag": "Some", "value": 3}
```

---

### position

`position(arr: [any], pred: fn(any) -> bool) -> i64`

Return the index of the first element satisfying `pred`, or `-1` if not found.

---

### count

`count(arr: [any]) -> i64`

Return the number of elements in `arr`.

---

### count_if

`count_if(arr: [any], pred: fn(any) -> bool) -> i64`

Return the number of elements satisfying `pred`.

---

## Slicing

### flatten

`flatten(arr: [any]) -> [any]`

Flatten one level of nesting (arrays of arrays into a single array).

**Example:**
```kryos
use std::iter

println(flatten([[1, 2], [3, 4], [5]]))   // [1, 2, 3, 4, 5]
```

---

### take

`take(arr: [any], n: i64) -> [any]`

Return the first `n` elements.

---

### take_while

`take_while(arr: [any], pred: fn(any) -> bool) -> [any]`

Return elements from the front as long as `pred` returns `true`.

---

### skip

`skip(arr: [any], n: i64) -> [any]`

Return all elements after the first `n`.

---

### skip_while

`skip_while(arr: [any], pred: fn(any) -> bool) -> [any]`

Drop elements from the front while `pred` returns `true`, then return the rest.

---

### chain

`chain(a: [any], b: [any]) -> [any]`

Concatenate `a` and `b` into a single array.

---

## Ordering

### rev

`rev(arr: [any]) -> [any]`

Return a new array with elements in reverse order.

---

### sort

`sort(arr: [any]) -> [any]`

Return a new array sorted in ascending order (insertion sort).

---

### sort_by

`sort_by(arr: [any], cmp: fn(any, any) -> i64) -> [any]`

Sort by a custom comparator. `cmp(a, b)` must return a negative number if `a < b`, zero if equal, or a positive number if `a > b`.

---

### sort_by_key

`sort_by_key(arr: [any], key_fn: fn(any) -> any) -> [any]`

Sort by a key derived from each element.

**Example:**
```kryos
use std::iter

let words = ["banana", "apple", "cherry"]
let sorted = sort_by_key(words, fn(s: str) -> i64 { return len(s) })
println(sorted)   // ["apple", "banana", "cherry"]
```

---

## Deduplication

### dedup

`dedup(arr: [any]) -> [any]`

Remove consecutive duplicate elements.

---

### unique

`unique(arr: [any]) -> [any]`

Remove all duplicate elements, preserving first-occurrence order.

**Example:**
```kryos
use std::iter

println(unique([1, 2, 2, 3, 1, 4]))   // [1, 2, 3, 4]
```

---

## Aggregation

### sum

`sum(arr: [any]) -> f64`

Return the sum of all elements.

---

### product

`product(arr: [any]) -> f64`

Return the product of all elements.

---

### min

`min(arr: [any]) -> any`

Return the smallest element.

---

### max

`max(arr: [any]) -> any`

Return the largest element.

---

### min_by_key

`min_by_key(arr: [any], key_fn: fn(any) -> any) -> any`

Return the element with the smallest key.

---

### max_by_key

`max_by_key(arr: [any], key_fn: fn(any) -> any) -> any`

Return the element with the largest key.

---

## Grouping

### group_by

`group_by(arr: [any], key_fn: fn(any) -> any) -> map`

Group elements by the value returned by `key_fn`. Returns a map from key to array of matching elements.

**Example:**
```kryos
use std::iter

let grouped = group_by([1, 2, 3, 4, 5, 6], fn(x: i64) -> str {
    if x % 2 == 0 { return "even" }
    return "odd"
})
// {"even": [2, 4, 6], "odd": [1, 3, 5]}
```

---

### partition

`partition(arr: [any], pred: fn(any) -> bool) -> [any]`

Split `arr` into two arrays: `[matches, non_matches]`.

**Example:**
```kryos
use std::iter

let parts = partition([1, 2, 3, 4, 5], fn(x: i64) -> bool { return x % 2 == 0 })
// [[2, 4], [1, 3, 5]]
```

---

### chunks

`chunks(arr: [any], n: i64) -> [any]`

Split `arr` into consecutive sub-arrays of size `n`. The last chunk may be smaller.

**Example:**
```kryos
use std::iter

println(chunks([1, 2, 3, 4, 5], 2))   // [[1, 2], [3, 4], [5]]
```

---

### windows

`windows(arr: [any], n: i64) -> [any]`

Return all overlapping sub-arrays of size `n`.

**Example:**
```kryos
use std::iter

println(windows([1, 2, 3, 4], 3))   // [[1, 2, 3], [2, 3, 4]]
```

---

### scan

`scan(arr: [any], init: any, f: fn(any, any) -> any) -> [any]`

Like `reduce`, but emit every intermediate accumulator value as an array.

**Example:**
```kryos
use std::iter

let running_sum = scan([1, 2, 3, 4], 0, fn(acc: i64, x: i64) -> i64 { return acc + x })
println(running_sum)   // [1, 3, 6, 10]
```

---

## Complete Example

```kryos
use std::iter

let data = [5, 3, 8, 1, 9, 2, 7, 4, 6]

// Find the top 3 even numbers
let top3_evens = take(
    sort(filter(data, fn(x: i64) -> bool { return x % 2 == 0 })),
    3
)
println(top3_evens)   // [2, 4, 6]

// Running sum
let cumulative = scan(range(1, 6), 0, fn(acc: i64, x: i64) -> i64 { return acc + x })
println(cumulative)   // [1, 3, 6, 10, 15]

// Group by remainder
let by_mod3 = group_by(range(1, 10), fn(x: i64) -> i64 { return x % 3 })
// {0: [3, 6, 9], 1: [1, 4, 7], 2: [2, 5, 8]}

// Labeled enumeration
for_each(enumerate(["alice", "bob", "carol"]), fn(pair: [any]) {
    println(pair[0] + ": " + pair[1])
})
// 0: alice
// 1: bob
// 2: carol
```
