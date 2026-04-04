# Operator Precedence

Operators listed from **lowest** to **highest** precedence. Operators at the same precedence level are evaluated left-to-right unless noted otherwise.

## Precedence Table

| Precedence | Operator | Description | Associativity | Example |
|:---:|----------|-------------|:---:|---------|
| 1 | `=` `+=` `-=` `*=` `/=` | Assignment | Right | `x = 5` |
| 2 | `\|>` | Pipe | Left | `data \|> transform \|> output` |
| 3 | `or` | Logical OR (short-circuit) | Left | `a or b` |
| 4 | `and` | Logical AND (short-circuit) | Left | `a and b` |
| 5 | `==` `!=` | Equality | Left | `x == y` |
| 6 | `<` `>` `<=` `>=` | Comparison | Left | `x < y` |
| 7 | `..` `..=` | Range | None | `1..10` |
| 8 | `+` `-` | Addition / Subtraction | Left | `a + b` |
| 9 | `*` `/` `%` | Multiplication / Division / Modulo | Left | `a * b` |
| 10 | `@` | Matrix multiply | Left | `A @ B` |
| 11 | `**` | Exponentiation | **Right** | `2 ** 3 ** 2` = `2 ** 9` |
| 12 | `-` `not` `~` | Unary negation / NOT / bitwise NOT | Right (prefix) | `-x`, `not done`, `~mask` |
| 13 | `()` `[]` `.` | Call / Index / Field access | Left (postfix) | `f(x)`, `arr[0]`, `obj.field` |

## Arithmetic Operators

| Operator | Description | Operand Types | Result Type | Example |
|----------|-------------|---------------|-------------|---------|
| `+` | Addition | `number + number` | Same as operands | `3 + 4` -> `7` |
| `+` | String concatenation | `str + str` | `str` | `"hi" + " there"` -> `"hi there"` |
| `-` | Subtraction | `number - number` | Same as operands | `10 - 3` -> `7` |
| `*` | Multiplication | `number * number` | Same as operands | `3 * 4` -> `12` |
| `*` | String repeat | `str * i64` | `str` | `"ha" * 3` -> `"hahaha"` |
| `/` | Division | `number / number` | `i64` if both int, else `f64` | `10 / 3` -> `3` |
| `%` | Modulo | `number % number` | Same as operands | `10 % 3` -> `1` |
| `**` | Exponentiation | `number ** number` | Same as operands | `2 ** 10` -> `1024` |
| `@` | Matrix multiply | `Tensor @ Tensor` | `Tensor` | `A @ B` |

**Division behavior:** Integer division (`i64 / i64`) truncates toward zero, returning an integer. Mixed or float division returns `f64`.

**Division by zero:** Both `/` and `%` raise a runtime error on division by zero. With self-healing enabled, the runtime substitutes `0` and logs a warning.

## Comparison Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Equal | `x == 5` |
| `!=` | Not equal | `x != 0` |
| `<` | Less than | `x < 10` |
| `>` | Greater than | `x > 0` |
| `<=` | Less than or equal | `x <= 100` |
| `>=` | Greater than or equal | `x >= 1` |

All comparison operators return `bool`. Values of different types can be compared with `==` and `!=` (they are never equal across types). Ordering operators (`<`, `>`, `<=`, `>=`) require comparable types.

## Logical Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `and` | Logical AND | `x > 0 and x < 100` |
| `or` | Logical OR | `x == 0 or y == 0` |
| `not` | Logical NOT (prefix) | `not done` |

Both `and` and `or` short-circuit: `and` returns the left operand if falsy, otherwise evaluates and returns the right. `or` returns the left operand if truthy, otherwise evaluates and returns the right.

**Truthiness rules:** `false`, `none`, `0`, `0.0`, and `""` are falsy. Everything else is truthy, including empty arrays.

## Bitwise Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `&` | Bitwise AND | `flags & mask` |
| `\|` | Bitwise OR | `flags \| bit` |
| `^` | Bitwise XOR | `a ^ b` |
| `~` | Bitwise NOT (prefix) | `~mask` |
| `<<` | Left shift | `1 << 8` -> `256` |
| `>>` | Right shift | `256 >> 4` -> `16` |

Bitwise operators work on integer types only.

## Assignment Operators

| Operator | Equivalent | Example |
|----------|-----------|---------|
| `=` | Direct assignment | `x = 5` |
| `+=` | `x = x + value` | `x += 1` |
| `-=` | `x = x - value` | `x -= 1` |
| `*=` | `x = x * value` | `x *= 2` |
| `/=` | `x = x / value` | `x /= 2` |

Assignment targets must be declared with `mut`. Assignment to an immutable binding raises a runtime error.

```kryos
let mut count = 0
count += 1          // ok

let name = "kryos"
// name = "other"   // error: cannot assign to immutable variable 'name'
```

## Pipe Operator

```
expr |> fn
```

The pipe operator passes the left-hand expression as the first argument to the right-hand function call.

```kryos
// Without pipe:
println(to_string(abs(floor(x))))

// With pipe:
x |> floor |> abs |> to_string |> println
```

## Range Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `..` | Exclusive range (end not included) | `1..5` produces `[1, 2, 3, 4]` |
| `..=` | Inclusive range (end included) | `1..=5` produces `[1, 2, 3, 4, 5]` |

Ranges are used in `for` loops and `match` arms:

```kryos
for i in 0..10 {
    println(i)
}

match score {
    90..=100 => "A",
    80..89   => "B",
    _        => "other",
}
```

## Other Punctuation

| Symbol | Description | Example |
|--------|-------------|---------|
| `->` | Return type annotation | `fn add(a: i32) -> i32` |
| `=>` | Match arm separator | `1 => "one"` |
| `::` | Namespace separator | `Color::Red`, `use utils::helpers` |
| `.` | Field access / method call | `point.x`, `list.len()` |
| `:` | Type annotation | `let x: i32 = 5` |
| `,` | Separator in lists | `fn f(a: i32, b: i32)` |
| `;` | Statement terminator (optional) | `let x = 5;` |
