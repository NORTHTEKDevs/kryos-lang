# std::fmt

String formatting, numeric display, and human-readable output. Covers positional templates, numeric bases, float notation, and convenience formatters for sizes, durations, and ordinals.

```kryos
use std::fmt
```

---

## Templates

### format

`format(template: str, args: [any]) -> str`

Interpolate `args` into `template` using positional placeholders `{0}`, `{1}`, etc. Each placeholder is replaced with the string representation of the corresponding argument.

**Example:**
```kryos
use std::fmt

let s = format("Hello, {0}! You have {1} messages.", ["Alice", 3])
println(s)   // "Hello, Alice! You have 3 messages."

let coords = format("({0}, {1})", [42, -7])
println(coords)   // "(42, -7)"
```

---

## Alignment and Padding

### pad_left

`pad_left(s: str, width: i64, ch: str) -> str`

Right-align `s` in a field of `width` characters by padding with `ch` on the left. If `s` is already at least `width` characters, it is returned unchanged.

**Example:**
```kryos
use std::fmt

println(pad_left("42", 6, " "))    // "    42"
println(pad_left("42", 6, "0"))    // "000042"
```

---

### pad_right

`pad_right(s: str, width: i64, ch: str) -> str`

Left-align `s` in a field of `width` characters by padding with `ch` on the right.

**Example:**
```kryos
use std::fmt

println(pad_right("hello", 10, " "))   // "hello     "
println(pad_right("hi", 5, "."))       // "hi..."
```

---

### center

`center(s: str, width: i64, ch: str) -> str`

Center `s` in a field of `width` characters, padding both sides with `ch`. When the padding is odd, the extra character goes on the right.

**Example:**
```kryos
use std::fmt

println(center("ok", 8, "-"))   // "---ok---"
println(center("hi", 7, " "))   // "  hi   "
```

---

## Numeric Bases

### hex

`hex(n: i64) -> str`

Format `n` as lowercase hexadecimal with a `"0x"` prefix.

**Example:**
```kryos
use std::fmt

println(hex(255))     // "0xff"
println(hex(65535))   // "0xffff"
```

---

### hex_upper

`hex_upper(n: i64) -> str`

Format `n` as uppercase hexadecimal with a `"0x"` prefix.

**Example:**
```kryos
use std::fmt

println(hex_upper(255))   // "0xFF"
```

---

### oct

`oct(n: i64) -> str`

Format `n` as octal with a `"0o"` prefix.

**Example:**
```kryos
use std::fmt

println(oct(8))    // "0o10"
println(oct(255))  // "0o377"
```

---

### bin

`bin(n: i64) -> str`

Format `n` as binary with a `"0b"` prefix.

**Example:**
```kryos
use std::fmt

println(bin(10))   // "0b1010"
println(bin(255))  // "0b11111111"
```

---

## Floating-Point Notation

### float_fixed

`float_fixed(n: f64, decimals: i64) -> str`

Format `n` in fixed-point notation with exactly `decimals` decimal places.

**Example:**
```kryos
use std::fmt

println(float_fixed(3.14159, 2))   // "3.14"
println(float_fixed(42.0, 4))      // "42.0000"
println(float_fixed(0.1, 6))       // "0.100000"
```

---

### float_sci

`float_sci(n: f64, decimals: i64) -> str`

Format `n` in scientific notation with `decimals` digits after the decimal point.

**Example:**
```kryos
use std::fmt

println(float_sci(12300.0, 2))    // "1.23e+4"
println(float_sci(0.00042, 3))    // "4.200e-4"
```

---

## Human-Readable Numbers

### thousands

`thousands(n: i64) -> str`

Format `n` with comma separators for thousands groups.

**Example:**
```kryos
use std::fmt

println(thousands(1234567))   // "1,234,567"
println(thousands(1000))      // "1,000"
println(thousands(42))        // "42"
```

---

### percent

`percent(ratio: f64, decimals: i64) -> str`

Format `ratio` as a percentage string with `decimals` decimal places. A ratio of `1.0` equals `100%`.

**Example:**
```kryos
use std::fmt

println(percent(0.857, 1))   // "85.7%"
println(percent(1.0, 0))     // "100%"
println(percent(0.5, 2))     // "50.00%"
```

---

### ordinal

`ordinal(n: i64) -> str`

Format `n` as an English ordinal string.

**Example:**
```kryos
use std::fmt

println(ordinal(1))    // "1st"
println(ordinal(2))    // "2nd"
println(ordinal(3))    // "3rd"
println(ordinal(11))   // "11th"
println(ordinal(22))   // "22nd"
```

---

### bytes

`bytes(n: i64) -> str`

Format `n` bytes as a human-readable size string using binary prefixes (KB = 1024, MB = 1024^2, etc.).

**Example:**
```kryos
use std::fmt

println(bytes(512))         // "512 B"
println(bytes(1536))        // "1.5 KB"
println(bytes(1048576))     // "1.0 MB"
println(bytes(2147483648))  // "2.0 GB"
```

---

### duration

`duration(seconds: i64) -> str`

Format `seconds` as a compact human-readable duration string.

**Example:**
```kryos
use std::fmt

println(duration(45))      // "45s"
println(duration(90))      // "1m 30s"
println(duration(3661))    // "1h 1m 1s"
```

---

## Debug and Display

### debug

`debug(val: any) -> str`

Return a debug representation of `val`. Strings are quoted, arrays are bracketed, and maps are braced. Useful for logging and diagnostics.

**Example:**
```kryos
use std::fmt

println(debug("hello"))         // "\"hello\""
println(debug([1, 2, 3]))       // "[1, 2, 3]"
println(debug(true))            // "true"
```

---

### display

`display(val: any) -> str`

Return a display representation of `val`. Strings are unquoted; otherwise behaves like `debug`.

**Example:**
```kryos
use std::fmt

println(display("hello"))   // "hello"
println(display(42))        // "42"
```

---

## Complete Example

```kryos
use std::fmt

// Build a formatted table row
let name = pad_right("Alice", 12, " ")
let score = pad_left("98", 5, " ")
let rank = ordinal(1)
println(format("{0} {1}  ({2})", [name, score, rank]))
// "Alice         98  (1st)"

// Format financial data
let revenue = 1234567
let margin = 0.2345
println(format("Revenue: {0}  Margin: {1}", [thousands(revenue), percent(margin, 1)]))
// "Revenue: 1,234,567  Margin: 23.5%"

// Numeric bases for a hex dump header
println(format("addr={0}  bin={1}", [hex(0xDEAD), bin(0b1010)]))
// "addr=0xdead  bin=0b1010"

// File size and elapsed time
println(bytes(4096000))    // "4.0 MB"
println(duration(7384))    // "2h 3m 4s"
```
