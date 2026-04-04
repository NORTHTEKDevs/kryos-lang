# std::collections

Higher-order collection functions for arrays: map, filter, reduce, sort, and more.

All functions in this module are available after `use std::collections`. Callback arguments can be named functions, anonymous functions, or closures. All functions return new arrays -- they do not mutate the original.

---

## Transformation

### map

```
map(arr: [T], fn: (T) -> U) -> [U]
```

Apply a function to every element of an array and return a new array of the results.

**Example:**

```kryos
let nums = [1, 2, 3, 4]
let doubled = map(nums, fn(x) { return x * 2 })
println(to_string(doubled))  // [2, 4, 6, 8]
```

```kryos
let names = ["alice", "bob"]
let upper_names = map(names, fn(s) { return upper(s) })
println(to_string(upper_names))  // ["ALICE", "BOB"]
```

**Edge cases:**

- Throws a runtime error if the first argument is not an array.
- Returns an empty array if the input array is empty.

**See also:** `filter`, `flat_map`

---

### flat_map

```
flat_map(arr: [T], fn: (T) -> [U] | U) -> [U]
```

Apply a function to every element and flatten the results by one level. If the function returns an array, its elements are concatenated into the result. If it returns a non-array value, that value is appended directly.

**Example:**

```kryos
let words = ["hello world", "foo bar"]
let chars = flat_map(words, fn(s) { return split(s, " ") })
println(to_string(chars))  // ["hello", "world", "foo", "bar"]
```

```kryos
let nums = [1, 2, 3]
let expanded = flat_map(nums, fn(x) { return [x, x * 10] })
println(to_string(expanded))  // [1, 10, 2, 20, 3, 30]
```

**Edge cases:**

- Only flattens one level. Nested arrays inside the returned arrays are not flattened further.
- Throws if the first argument is not an array.

**See also:** `map`

---

### reverse

```
reverse(arr: [T]) -> [T]
```

Return a new array with elements in reverse order.

**Example:**

```kryos
let reversed = reverse([1, 2, 3])
println(to_string(reversed))  // [3, 2, 1]
```

**Edge cases:**

- Returns an empty array for an empty input.
- Does not mutate the original array.
- Throws if the argument is not an array.

**See also:** `sort`

---

## Filtering

### filter

```
filter(arr: [T], fn: (T) -> bool) -> [T]
```

Return a new array containing only elements for which the callback returns a truthy value.

**Example:**

```kryos
let nums = [1, 2, 3, 4, 5, 6]
let evens = filter(nums, fn(x) { return x % 2 == 0 })
println(to_string(evens))  // [2, 4, 6]
```

```kryos
let words = ["kryos", "", "lang", ""]
let non_empty = filter(words, fn(s) { return len(s) > 0 })
println(to_string(non_empty))  // ["kryos", "lang"]
```

**Edge cases:**

- Throws if the first argument is not an array.
- Returns an empty array if no elements match.

**See also:** `find`, `any`, `all`

---

### find

```
find(arr: [T], fn: (T) -> bool) -> T | none
```

Return the first element for which the callback returns truthy. Returns `none` if no element matches.

**Example:**

```kryos
let users = [{"name": "alice", "age": 30}, {"name": "bob", "age": 25}]
let young = find(users, fn(u) { return json_get(u, "age") < 28 })
println(to_string(young))  // {"name": "bob", "age": 25}
```

```kryos
let missing = find([1, 2, 3], fn(x) { return x > 10 })
println(to_string(missing))  // none
```

**Edge cases:**

- Stops at the first match. Does not scan the entire array.
- Returns `none` (not an error) when no element matches.
- Throws if the first argument is not an array.

**See also:** `filter`, `any`

---

## Aggregation

### reduce

```
reduce(arr: [T], fn: (acc: U, item: T) -> U, initial: U) -> U
```

Reduce an array to a single value by applying a function to an accumulator and each element. The third argument is the initial accumulator value.

**Example:**

```kryos
let nums = [1, 2, 3, 4, 5]
let total = reduce(nums, fn(acc, x) { return acc + x }, 0)
println(to_string(total))  // 15
```

```kryos
// Build a string from an array
let words = ["kryos", "is", "fast"]
let sentence = reduce(words, fn(acc, w) { return acc + " " + w }, "")
println(trim(sentence))  // "kryos is fast"
```

**Edge cases:**

- If the array is empty, returns the initial value unchanged.
- Throws if the first argument is not an array.

**See also:** `sum`, `count`

---

### sum

```
sum(arr: [number]) -> number
```

Return the sum of all elements in a numeric array.

**Example:**

```kryos
let total = sum([10, 20, 30])
println(to_string(total))  // 60
```

**Edge cases:**

- Returns `0` for an empty array.
- Throws if the argument is not an array.
- Behavior is undefined for non-numeric elements.

**See also:** `reduce`, `count`

---

### count

```
count(arr: [T]) -> i32
```

Return the number of elements in an array. Equivalent to `len()` but specific to arrays.

**Example:**

```kryos
let n = count([1, 2, 3])
println(to_string(n))  // 3
```

**Edge cases:**

- Returns `0` for an empty array.
- Throws if the argument is not an array.

**See also:** `len`, `sum`

---

## Predicates

### any

```
any(arr: [T], fn: (T) -> bool) -> bool
```

Return `true` if at least one element satisfies the callback.

**Example:**

```kryos
let has_negative = any([1, -2, 3], fn(x) { return x < 0 })
println(to_string(has_negative))  // true
```

**Edge cases:**

- Short-circuits: stops scanning as soon as a truthy result is found.
- Returns `false` for an empty array.
- Throws if the first argument is not an array.

**See also:** `all`, `find`

---

### all

```
all(arr: [T], fn: (T) -> bool) -> bool
```

Return `true` if every element satisfies the callback.

**Example:**

```kryos
let all_positive = all([1, 2, 3], fn(x) { return x > 0 })
println(to_string(all_positive))  // true

let all_even = all([2, 4, 5], fn(x) { return x % 2 == 0 })
println(to_string(all_even))  // false
```

**Edge cases:**

- Short-circuits: stops scanning as soon as a falsy result is found.
- Returns `true` for an empty array (vacuous truth).
- Throws if the first argument is not an array.

**See also:** `any`, `filter`

---

## Ordering

### sort

```
sort(arr: [T]) -> [T]
```

Return a new array with elements sorted in ascending order using the default comparison.

**Example:**

```kryos
let sorted_nums = sort([3, 1, 4, 1, 5])
println(to_string(sorted_nums))  // [1, 1, 3, 4, 5]

let sorted_words = sort(["banana", "apple", "cherry"])
println(to_string(sorted_words))  // ["apple", "banana", "cherry"]
```

**Edge cases:**

- Does not mutate the original array.
- Strings are sorted lexicographically.
- Throws if the argument is not an array.
- Comparing mixed types (e.g., strings and numbers) may produce unexpected results.

**See also:** `reverse`

---

## Combining

### zip

```
zip(a: [T], b: [U]) -> [[T, U]]
```

Combine two arrays element-wise into an array of two-element arrays.

**Example:**

```kryos
let names = ["alice", "bob", "carol"]
let scores = [95, 87, 92]
let pairs = zip(names, scores)
println(to_string(pairs))  // [["alice", 95], ["bob", 87], ["carol", 92]]
```

```kryos
for pair in zip(names, scores) {
    println(pair[0] + ": " + to_string(pair[1]))
}
```

**Edge cases:**

- If the arrays have different lengths, the result has the length of the shorter array. Extra elements in the longer array are discarded.
- Throws if either argument is not an array.

**See also:** `enumerate`

---

### enumerate

```
enumerate(arr: [T]) -> [[i32, T]]
```

Return an array of `[index, value]` pairs, where `index` is zero-based.

**Example:**

```kryos
let items = ["a", "b", "c"]
for pair in enumerate(items) {
    println(to_string(pair[0]) + ": " + pair[1])
}
// 0: a
// 1: b
// 2: c
```

**Edge cases:**

- Indices start at 0.
- Returns an empty array for an empty input.
- Throws if the argument is not an array.

**See also:** `zip`
