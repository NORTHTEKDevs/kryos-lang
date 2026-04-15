# std::stream

Lazy integer streams with a fluent pipeline API. Build processing chains from range or list sources, transform and filter elements, then collect or reduce at the end.

```kryos
use std::stream
```

---

## Types

### Stream

A lazy sequence of `i64` values.

| Field   | Type    | Description                      |
|---------|---------|----------------------------------|
| `data`  | `[i64]` | Buffered elements                |
| `count` | `i64`   | Number of elements in the stream |

---

## Creating Streams

### stream_from_list

`stream_from_list(items: [i64]) -> Stream`

Create a `Stream` from an existing array of integers.

**Example:**
```kryos
use std::stream

let s = stream_from_list([1, 2, 3, 4, 5])
```

---

### stream_from_range

`stream_from_range(start: i64, end: i64) -> Stream`

Create a `Stream` of integers from `start` up to but not including `end`.

**Example:**
```kryos
use std::stream

let s = stream_from_range(0, 10)   // 0, 1, 2, ..., 9
```

---

### stream_empty

`stream_empty() -> Stream`

Create an empty `Stream` with zero elements.

**Example:**
```kryos
use std::stream

let s = stream_empty()
println(s.count())   // 0
```

---

### stream_concat

`stream_concat(a: Stream, b: Stream) -> Stream`

Concatenate two streams into a new stream containing all elements of `a` followed by all elements of `b`.

**Example:**
```kryos
use std::stream

let a = stream_from_range(0, 3)
let b = stream_from_range(10, 13)
let c = stream_concat(a, b)
println(c.collect())   // [0, 1, 2, 10, 11, 12]
```

---

## Transformations

### map

`map(transform: fn(i64) -> i64) -> Stream`

Apply `transform` to each element and return a new stream with the results.

**Example:**
```kryos
use std::stream

let doubled = stream_from_range(1, 6).map(fn(x: i64) -> i64 {
    return x * 2
})
println(doubled.collect())   // [2, 4, 6, 8, 10]
```

---

### filter

`filter(predicate: fn(i64) -> bool) -> Stream`

Return a new stream containing only the elements for which `predicate` returns `true`.

**Example:**
```kryos
use std::stream

let evens = stream_from_range(0, 10).filter(fn(x: i64) -> bool {
    return x % 2 == 0
})
println(evens.collect())   // [0, 2, 4, 6, 8]
```

---

### take

`take(n: i64) -> Stream`

Return a new stream with at most the first `n` elements.

**Example:**
```kryos
use std::stream

let first3 = stream_from_range(0, 100).take(3)
println(first3.collect())   // [0, 1, 2]
```

---

### skip

`skip(n: i64) -> Stream`

Return a new stream with the first `n` elements removed.

**Example:**
```kryos
use std::stream

let s = stream_from_list([10, 20, 30, 40, 50]).skip(2)
println(s.collect())   // [30, 40, 50]
```

---

## Consumers

### collect

`collect() -> [i64]`

Evaluate the stream and return all elements as an array.

---

### reduce

`reduce(reducer: fn(i64, i64) -> i64, initial: i64) -> i64`

Reduce the stream to a single value by applying `reducer` to each element with an accumulator, starting from `initial`.

**Example:**
```kryos
use std::stream

let product = stream_from_range(1, 6).reduce(fn(acc: i64, x: i64) -> i64 {
    return acc * x
}, 1)
println(product)   // 120
```

---

### for_each

`for_each(action: fn(i64))`

Execute `action` for each element. Does not return a value.

**Example:**
```kryos
use std::stream

stream_from_range(1, 4).for_each(fn(x: i64) {
    println(x)
})
// 1
// 2
// 3
```

---

### count

`count() -> i64`

Return the number of elements in the stream.

---

### first

`first() -> i64`

Return the first element. Throws if the stream is empty.

---

### last

`last() -> i64`

Return the last element. Throws if the stream is empty.

---

### sum

`sum() -> i64`

Return the sum of all elements. Returns `0` for an empty stream.

---

### min

`min() -> i64`

Return the minimum element. Throws if the stream is empty.

---

### max

`max() -> i64`

Return the maximum element. Throws if the stream is empty.

---

## Predicates

### any

`any(predicate: fn(i64) -> bool) -> bool`

Return `true` if any element satisfies `predicate`. Short-circuits on the first match.

**Example:**
```kryos
use std::stream

let has_neg = stream_from_list([1, -2, 3]).any(fn(x: i64) -> bool {
    return x < 0
})
println(has_neg)   // true
```

---

### all

`all(predicate: fn(i64) -> bool) -> bool`

Return `true` if every element satisfies `predicate`. Short-circuits on the first failure.

**Example:**
```kryos
use std::stream

let all_pos = stream_from_range(1, 5).all(fn(x: i64) -> bool {
    return x > 0
})
println(all_pos)   // true
```

---

## Complete Example

```kryos
use std::stream

// Sum of squares of even numbers in 1..20
let result = stream_from_range(1, 21)
    .filter(fn(x: i64) -> bool { return x % 2 == 0 })
    .map(fn(x: i64) -> i64 { return x * x })
    .sum()
println(result)   // 1540

// First 5 multiples of 3
let multiples = stream_from_range(1, 100)
    .filter(fn(x: i64) -> bool { return x % 3 == 0 })
    .take(5)
    .collect()
println(multiples)   // [3, 6, 9, 12, 15]

// Combine two ranges and find max
let combined = stream_concat(
    stream_from_range(10, 15),
    stream_from_range(100, 105)
)
println(combined.max())   // 104
```
