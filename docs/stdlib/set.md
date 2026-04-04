# std::set

Set data structure for unique, unordered collections. Sets mutate in place for `set_add` and `set_remove`. Set algebra operations (`union`, `intersection`, `difference`) return new sets.

```kryos
import std::set
```

---

### set_new

`set_new() -> Set`
`set_new(item1: Any, item2: Any, ...) -> Set`
`set_new(items: Array) -> Set`

Create a new set. Can be called with no arguments (empty set), with individual items, or with a single array argument.

**Example:**
```kryos
let empty = set_new()
let colors = set_new("red", "green", "blue")
let from_array = set_new(["a", "b", "c", "a"])  // duplicates removed
print(set_size(from_array))  // 3
```

**See also:** set_from_array

---

### set_add

`set_add(s: Set, value: Any) -> Set`

Add a value to the set. Mutates and returns the set.

**Example:**
```kryos
let s = set_new()
set_add(s, "hello")
set_add(s, "world")
set_add(s, "hello")  // no-op, already in set
print(set_size(s))   // 2
```

**Edge cases:**
- Adding a value that already exists is a no-op.
- Raises if the first argument is not a set.
- Values must be hashable (strings, numbers, booleans).

**See also:** set_remove, set_has

---

### set_remove

`set_remove(s: Set, value: Any) -> Set`

Remove a value from the set. Mutates and returns the set.

**Example:**
```kryos
let s = set_new("a", "b", "c")
set_remove(s, "b")
print(set_has(s, "b"))  // false
print(set_size(s))      // 2
```

**Edge cases:**
- Removing a value that does not exist is a no-op (does not raise).

**See also:** set_add

---

### set_has

`set_has(s: Set, value: Any) -> Bool`

Check whether a value is in the set.

**Example:**
```kryos
let admins = set_new("alice", "bob")
if set_has(admins, current_user) {
    print("Access granted")
}
```

**See also:** set_add, set_size

---

### set_size

`set_size(s: Set) -> Int`

Get the number of elements in the set.

**Example:**
```kryos
let s = set_new(1, 2, 3)
print(set_size(s))  // 3
```

**See also:** set_has

---

### set_union

`set_union(a: Set, b: Set) -> Set`

Return a new set containing all elements from both sets.

**Example:**
```kryos
let a = set_new(1, 2, 3)
let b = set_new(3, 4, 5)
let c = set_union(a, b)
print(set_size(c))  // 5
// c contains: {1, 2, 3, 4, 5}
```

**See also:** set_intersection, set_difference

---

### set_intersection

`set_intersection(a: Set, b: Set) -> Set`

Return a new set containing only elements present in both sets.

**Example:**
```kryos
let a = set_new(1, 2, 3, 4)
let b = set_new(3, 4, 5, 6)
let c = set_intersection(a, b)
print(set_to_array(c))  // [3, 4] (order may vary)
```

**See also:** set_union, set_difference

---

### set_difference

`set_difference(a: Set, b: Set) -> Set`

Return a new set containing elements in `a` that are not in `b`.

**Example:**
```kryos
let all_users = set_new("alice", "bob", "charlie")
let banned = set_new("charlie")
let active = set_difference(all_users, banned)
print(set_to_array(active))  // ["alice", "bob"] (order may vary)
```

**Edge cases:**
- Not symmetric: `set_difference(a, b)` is different from `set_difference(b, a)`.

**See also:** set_union, set_intersection

---

### set_to_array

`set_to_array(s: Set) -> Array`

Convert a set to an array.

**Example:**
```kryos
let s = set_new("x", "y", "z")
let arr = set_to_array(s)
print(len(arr))  // 3
```

**Edge cases:**
- The order of elements in the resulting array is not guaranteed.

**See also:** set_from_array

---

### set_from_array

`set_from_array(arr: Array) -> Set`

Create a set from an array, removing duplicates.

**Example:**
```kryos
let names = ["alice", "bob", "alice", "charlie", "bob"]
let unique = set_from_array(names)
print(set_size(unique))  // 3
```

**See also:** set_new, set_to_array
