# std::collections

Typed collection data structures: `List`, `Map`, `Set`, `Stack`, `Queue`, and `Deque`. Each is an OO struct with methods called using dot notation.

All methods return new values rather than mutating in place. Assign the return value to update the collection.

```kryos
use std::collections
```

---

## List

A dynamic ordered sequence. Backed by an array with a tracked length.

```kryos
struct List {
    data:   [any],
    length: i64
}
```

### List.new

`List.new() -> List`

Create an empty list.

**Example:**
```kryos
use std::collections

let mut xs = List.new()
xs = xs.push(1)
xs = xs.push(2)
println(xs.to_string())   // [1, 2]
```

---

### List.from

`List.from(arr: [any]) -> List`

Create a list from an existing array.

**Example:**
```kryos
use std::collections

let xs = List.from([10, 20, 30])
println(xs.len())   // 3
```

---

### List.push

`push(self: List, val: any) -> List`

Append `val` to the end of the list. Returns the updated list.

**Example:**
```kryos
let xs = List.new()
let xs = xs.push("a").push("b").push("c")
println(xs.to_string())   // [a, b, c]
```

---

### List.pop

`pop(self: List) -> any`

Remove and return the last element. Throws a runtime error if the list is empty.

**Example:**
```kryos
let xs = List.from([1, 2, 3])
let val = xs.pop()
println(val)   // 3
```

---

### List.get

`get(self: List, index: i64) -> any`

Return the element at zero-based `index`. Throws a runtime error if `index` is out of bounds.

**Example:**
```kryos
let xs = List.from(["a", "b", "c"])
println(xs.get(1))   // b
```

---

### List.set

`set(self: List, index: i64, val: any) -> List`

Return a new list with the element at `index` replaced by `val`. Throws a runtime error if `index` is out of bounds.

**Example:**
```kryos
let xs = List.from([1, 2, 3])
let xs = xs.set(1, 99)
println(xs.to_string())   // [1, 99, 3]
```

---

### List.len

`len(self: List) -> i64`

Return the number of elements.

**Example:**
```kryos
let xs = List.from([10, 20, 30])
println(xs.len())   // 3
```

---

### List.is_empty

`is_empty(self: List) -> bool`

Return `true` if the list contains no elements.

**Example:**
```kryos
println(List.new().is_empty())             // true
println(List.from([1]).is_empty())         // false
```

---

### List.contains

`contains(self: List, val: any) -> bool`

Return `true` if `val` is present in the list. Uses equality comparison.

**Example:**
```kryos
let xs = List.from([1, 2, 3])
println(xs.contains(2))   // true
println(xs.contains(9))   // false
```

---

### List.index_of

`index_of(self: List, val: any) -> i64`

Return the zero-based index of the first occurrence of `val`, or `-1` if not found.

**Example:**
```kryos
let xs = List.from(["a", "b", "c", "b"])
println(xs.index_of("b"))   // 1
println(xs.index_of("z"))   // -1
```

---

### List.insert

`insert(self: List, index: i64, val: any) -> List`

Return a new list with `val` inserted at `index`. Elements at and after `index` shift right.

**Example:**
```kryos
let xs = List.from([1, 3])
let xs = xs.insert(1, 2)
println(xs.to_string())   // [1, 2, 3]
```

---

### List.remove

`remove(self: List, index: i64) -> List`

Return a new list with the element at `index` removed.

**Example:**
```kryos
let xs = List.from([1, 2, 3])
let xs = xs.remove(1)
println(xs.to_string())   // [1, 3]
```

---

### List.slice

`slice(self: List, start: i64, end: i64) -> List`

Return a new list containing elements from `start` (inclusive) to `end` (exclusive). Indices are clamped to list bounds.

**Example:**
```kryos
let xs = List.from([10, 20, 30, 40, 50])
println(xs.slice(1, 4).to_string())   // [20, 30, 40]
```

**Edge cases:**
- Indices below `0` are clamped to `0`.
- Indices beyond `len` are clamped to `len`.
- If `start >= end` after clamping, returns an empty list.

---

### List.clear

`clear(self: List) -> List`

Return an empty list.

**Example:**
```kryos
let xs = List.from([1, 2, 3])
println(xs.clear().is_empty())   // true
```

---

### List.sort

`sort(self: List) -> List`

Return a new list sorted in ascending order using insertion sort.

**Example:**
```kryos
let xs = List.from([3, 1, 4, 1, 5, 9])
println(xs.sort().to_string())   // [1, 1, 3, 4, 5, 9]
```

---

### List.reverse

`reverse(self: List) -> List`

Return a new list with elements in reversed order.

**Example:**
```kryos
let xs = List.from([1, 2, 3])
println(xs.reverse().to_string())   // [3, 2, 1]
```

---

### List.map

`map(self: List, f: fn) -> List`

Apply `f` to each element and return a new list of the results.

**Example:**
```kryos
let xs = List.from([1, 2, 3])
let ys = xs.map(fn(x) { x * 2 })
println(ys.to_string())   // [2, 4, 6]
```

---

### List.filter

`filter(self: List, pred: fn) -> List`

Return a new list containing only elements for which `pred` returns `true`.

**Example:**
```kryos
let xs = List.from([1, 2, 3, 4, 5])
let evens = xs.filter(fn(x) { x % 2 == 0 })
println(evens.to_string())   // [2, 4]
```

---

### List.iter

`iter(self: List) -> [any]`

Return the underlying array of elements. Useful for `for` loop iteration.

**Example:**
```kryos
let xs = List.from([10, 20, 30])
for x in xs.iter() {
    println(x)
}
```

---

### List.to_string

`to_string(self: List) -> str`

Return a string representation in the format `[a, b, c]`.

**Example:**
```kryos
println(List.from([1, 2, 3]).to_string())   // [1, 2, 3]
println(List.new().to_string())             // []
```

---

## Map

An ordered key-value store keyed by `str`.

```kryos
struct Map {
    store:  any,
    length: i64
}
```

### Map.new

`Map.new() -> Map`

Create an empty map.

**Example:**
```kryos
use std::collections

let mut m = Map.new()
m = m.set("name", "Alice")
m = m.set("age", 30)
println(m.to_string())   // {name: Alice, age: 30}
```

---

### Map.from

`Map.from(obj: any) -> Map`

Create a map from an existing object value.

---

### Map.set

`set(self: Map, key: str, val: any) -> Map`

Return a new map with `key` mapped to `val`. Overwrites any existing value for `key`.

**Example:**
```kryos
let m = Map.new()
let m = m.set("x", 1).set("y", 2)
println(m.to_string())   // {x: 1, y: 2}
```

---

### Map.get

`get(self: Map, key: str) -> any`

Return the value for `key`. Throws a runtime error if `key` is not present.

**Example:**
```kryos
let m = Map.new().set("host", "localhost")
println(m.get("host"))   // localhost
```

**See also:** `get_or`

---

### Map.get_or

`get_or(self: Map, key: str, default: any) -> any`

Return the value for `key`, or `default` if `key` is not present.

**Example:**
```kryos
let m = Map.new().set("port", 8080)
println(m.get_or("port", 80))      // 8080
println(m.get_or("timeout", 30))   // 30
```

---

### Map.has

`has(self: Map, key: str) -> bool`

Return `true` if `key` exists in the map.

**Example:**
```kryos
let m = Map.new().set("debug", true)
println(m.has("debug"))    // true
println(m.has("verbose"))  // false
```

---

### Map.delete

`delete(self: Map, key: str) -> Map`

Return a new map with `key` removed. No-op if `key` is not present.

**Example:**
```kryos
let m = Map.new().set("a", 1).set("b", 2)
let m = m.delete("a")
println(m.to_string())   // {b: 2}
```

---

### Map.keys

`keys(self: Map) -> [str]`

Return an array of all keys in insertion order.

**Example:**
```kryos
let m = Map.new().set("x", 1).set("y", 2).set("z", 3)
println(m.keys())   // [x, y, z]
```

---

### Map.values

`values(self: Map) -> [any]`

Return an array of all values in insertion order.

**Example:**
```kryos
let m = Map.new().set("x", 10).set("y", 20)
println(m.values())   // [10, 20]
```

---

### Map.entries

`entries(self: Map) -> [any]`

Return an array of `[key, value]` pairs in insertion order.

**Example:**
```kryos
let m = Map.new().set("a", 1).set("b", 2)
for entry in m.entries() {
    println(entry)   // [a, 1] then [b, 2]
}
```

---

### Map.len

`len(self: Map) -> i64`

Return the number of key-value pairs.

---

### Map.is_empty

`is_empty(self: Map) -> bool`

Return `true` if the map contains no entries.

---

### Map.clear

`clear(self: Map) -> Map`

Return an empty map.

---

### Map.merge

`merge(self: Map, other: Map) -> Map`

Return a new map combining both maps. When the same key exists in both, `other`'s value takes precedence.

**Example:**
```kryos
let base = Map.new().set("a", 1).set("b", 2)
let patch = Map.new().set("b", 99).set("c", 3)
let result = base.merge(patch)
println(result.to_string())   // {a: 1, b: 99, c: 3}
```

---

### Map.to_string

`to_string(self: Map) -> str`

Return a string representation in the format `{key: val, ...}`.

**Example:**
```kryos
println(Map.new().set("x", 1).to_string())   // {x: 1}
println(Map.new().to_string())               // {}
```

---

## Set

An unordered collection of unique values.

```kryos
struct Set {
    store:  any,
    length: i64
}
```

### Set.new

`Set.new() -> Set`

Create an empty set.

**Example:**
```kryos
use std::collections

let mut s = Set.new()
s = s.add(1).add(2).add(2).add(3)
println(s.to_string())   // Set{1, 2, 3}
```

---

### Set.from

`Set.from(arr: [any]) -> Set`

Create a set from an array, deduplicating elements.

**Example:**
```kryos
let s = Set.from([1, 2, 2, 3, 3])
println(s.len())   // 3
```

---

### Set.add

`add(self: Set, val: any) -> Set`

Return a new set with `val` added. If `val` is already present, the set is unchanged.

---

### Set.has

`has(self: Set, val: any) -> bool`

Return `true` if `val` is in the set.

**Example:**
```kryos
let s = Set.from([10, 20, 30])
println(s.has(20))   // true
println(s.has(99))   // false
```

---

### Set.delete

`delete(self: Set, val: any) -> Set`

Return a new set with `val` removed. No-op if `val` is not present.

---

### Set.len

`len(self: Set) -> i64`

Return the number of unique elements.

---

### Set.is_empty

`is_empty(self: Set) -> bool`

Return `true` if the set contains no elements.

---

### Set.union

`union(self: Set, other: Set) -> Set`

Return a new set containing all elements from both sets.

**Example:**
```kryos
let a = Set.from([1, 2, 3])
let b = Set.from([3, 4, 5])
println(a.union(b).to_string())   // Set{1, 2, 3, 4, 5}
```

---

### Set.intersection

`intersection(self: Set, other: Set) -> Set`

Return a new set containing only elements present in both sets.

**Example:**
```kryos
let a = Set.from([1, 2, 3])
let b = Set.from([2, 3, 4])
println(a.intersection(b).to_string())   // Set{2, 3}
```

---

### Set.difference

`difference(self: Set, other: Set) -> Set`

Return a new set containing elements in `self` that are not in `other`.

**Example:**
```kryos
let a = Set.from([1, 2, 3])
let b = Set.from([2, 3, 4])
println(a.difference(b).to_string())   // Set{1}
```

---

### Set.symmetric_difference

`symmetric_difference(self: Set, other: Set) -> Set`

Return a new set containing elements in either set but not both.

**Example:**
```kryos
let a = Set.from([1, 2, 3])
let b = Set.from([2, 3, 4])
println(a.symmetric_difference(b).to_string())   // Set{1, 4}
```

---

### Set.is_subset

`is_subset(self: Set, other: Set) -> bool`

Return `true` if every element of `self` is also in `other`.

**Example:**
```kryos
let a = Set.from([1, 2])
let b = Set.from([1, 2, 3])
println(a.is_subset(b))   // true
println(b.is_subset(a))   // false
```

---

### Set.is_superset

`is_superset(self: Set, other: Set) -> bool`

Return `true` if `self` contains every element of `other`.

---

### Set.to_list

`to_list(self: Set) -> [any]`

Return the elements as an array. Order is not guaranteed.

---

### Set.clear

`clear(self: Set) -> Set`

Return an empty set.

---

### Set.to_string

`to_string(self: Set) -> str`

Return a string representation in the format `Set{a, b, c}`.

**Example:**
```kryos
println(Set.from([1, 2, 3]).to_string())   // Set{1, 2, 3}
println(Set.new().to_string())             // Set{}
```

---

## Stack

A last-in, first-out (LIFO) collection.

```kryos
struct Stack {
    data:   [any],
    length: i64
}
```

### Stack.new

`Stack.new() -> Stack`

Create an empty stack.

**Example:**
```kryos
use std::collections

let mut s = Stack.new()
s = s.push(1)
s = s.push(2)
s = s.push(3)
let top = s.peek()
println(top)             // 3
println(s.to_string())   // Stack[1, 2, 3]
```

---

### Stack.push

`push(self: Stack, val: any) -> Stack`

Return a new stack with `val` on top.

---

### Stack.pop

`pop(self: Stack) -> any`

Remove and return the top element. Throws a runtime error if the stack is empty.

**Example:**
```kryos
let s = Stack.new().push("a").push("b").push("c")
println(s.pop())   // c
```

---

### Stack.peek

`peek(self: Stack) -> any`

Return the top element without removing it. Throws a runtime error if the stack is empty.

**Example:**
```kryos
let s = Stack.new().push(10).push(20)
println(s.peek())   // 20
```

---

### Stack.len

`len(self: Stack) -> i64`

Return the number of elements.

---

### Stack.is_empty

`is_empty(self: Stack) -> bool`

Return `true` if the stack contains no elements.

---

### Stack.clear

`clear(self: Stack) -> Stack`

Return an empty stack.

---

### Stack.to_string

`to_string(self: Stack) -> str`

Return a string representation in the format `Stack[...]` with the top element on the right.

**Example:**
```kryos
println(Stack.new().push(1).push(2).push(3).to_string())   // Stack[1, 2, 3]
```

---

## Queue

A first-in, first-out (FIFO) collection.

```kryos
struct Queue {
    data:   [any],
    length: i64
}
```

### Queue.new

`Queue.new() -> Queue`

Create an empty queue.

**Example:**
```kryos
use std::collections

let mut q = Queue.new()
q = q.enqueue("first")
q = q.enqueue("second")
q = q.enqueue("third")
println(q.dequeue())     // first
println(q.to_string())   // Queue[second, third]
```

---

### Queue.enqueue

`enqueue(self: Queue, val: any) -> Queue`

Return a new queue with `val` added to the back.

---

### Queue.dequeue

`dequeue(self: Queue) -> any`

Remove and return the front element. Throws a runtime error if the queue is empty.

---

### Queue.peek

`peek(self: Queue) -> any`

Return the front element without removing it. Throws a runtime error if the queue is empty.

**Example:**
```kryos
let q = Queue.new().enqueue(1).enqueue(2)
println(q.peek())   // 1
```

---

### Queue.len

`len(self: Queue) -> i64`

Return the number of elements.

---

### Queue.is_empty

`is_empty(self: Queue) -> bool`

Return `true` if the queue contains no elements.

---

### Queue.clear

`clear(self: Queue) -> Queue`

Return an empty queue.

---

### Queue.to_string

`to_string(self: Queue) -> str`

Return a string representation in the format `Queue[...]` with the front element on the left.

**Example:**
```kryos
println(Queue.new().enqueue(1).enqueue(2).enqueue(3).to_string())   // Queue[1, 2, 3]
```

---

## Deque

A double-ended queue. Elements can be added to or removed from either end.

```kryos
struct Deque {
    data:   [any],
    length: i64
}
```

### Deque.new

`Deque.new() -> Deque`

Create an empty deque.

**Example:**
```kryos
use std::collections

let mut d = Deque.new()
d = d.push_back(2)
d = d.push_front(1)
d = d.push_back(3)
println(d.to_string())   // Deque[1, 2, 3]
```

---

### Deque.push_front

`push_front(self: Deque, val: any) -> Deque`

Return a new deque with `val` added to the front.

---

### Deque.push_back

`push_back(self: Deque, val: any) -> Deque`

Return a new deque with `val` added to the back.

---

### Deque.pop_front

`pop_front(self: Deque) -> any`

Remove and return the front element. Throws a runtime error if the deque is empty.

---

### Deque.pop_back

`pop_back(self: Deque) -> any`

Remove and return the back element. Throws a runtime error if the deque is empty.

---

### Deque.front

`front(self: Deque) -> any`

Return the front element without removing it. Throws a runtime error if the deque is empty.

---

### Deque.back

`back(self: Deque) -> any`

Return the back element without removing it. Throws a runtime error if the deque is empty.

**Example:**
```kryos
let d = Deque.new().push_back(10).push_back(20).push_back(30)
println(d.front())   // 10
println(d.back())    // 30
```

---

### Deque.len

`len(self: Deque) -> i64`

Return the number of elements.

---

### Deque.is_empty

`is_empty(self: Deque) -> bool`

Return `true` if the deque contains no elements.

---

### Deque.clear

`clear(self: Deque) -> Deque`

Return an empty deque.

---

### Deque.to_string

`to_string(self: Deque) -> str`

Return a string representation in the format `Deque[...]`.

**Example:**
```kryos
println(Deque.new().push_back(1).push_back(2).to_string())   // Deque[1, 2]
```

---

## Choosing the Right Type

| Need | Use |
|------|-----|
| Ordered sequence with random access | `List` |
| Key-value lookup by string key | `Map` |
| Membership testing, deduplication | `Set` |
| LIFO access (undo stack, call stack) | `Stack` |
| FIFO access (task queue, BFS) | `Queue` |
| Insert/remove at both ends | `Deque` |
