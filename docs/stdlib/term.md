# std::term

Terminal control: dimensions, cursor positioning, ANSI colors, raw mode, and screen management. All functions emit ANSI escape sequences and work in any terminal emulator that supports them. On Windows, raw mode uses `msvcrt`; on Unix it uses `termios`.

```kryos
use std::term
```

---

## Terminal Dimensions

### width

`width() -> i32`

Return the current terminal width in columns.

**Example:**
```kryos
use std::term

let cols = width()
println(cols)   // e.g. 220
```

---

### height

`height() -> i32`

Return the current terminal height in rows.

**Example:**
```kryos
use std::term

let rows = height()
println(rows)   // e.g. 50
```

---

### size

`size() -> [i32]`

Return `[width, height]` as a two-element array.

**Example:**
```kryos
use std::term

let dims = size()
println(dims[0])   // width
println(dims[1])   // height
```

---

## Screen Control

### clear

`clear() -> bool`

Clear the terminal screen and return the cursor to the home position. Returns `true` on success.

**Example:**
```kryos
use std::term

clear()
```

---

### clear_line

`clear_line() -> bool`

Clear the current line from the cursor to the end of the line. Returns `true` on success.

**Example:**
```kryos
use std::term

clear_line()
```

---

### clear_below

`clear_below() -> bool`

Clear from the current cursor position to the end of the screen. Returns `true` on success.

**Example:**
```kryos
use std::term

clear_below()
```

---

## Cursor Control

### cursor_move

`cursor_move(row: i32, col: i32) -> bool`

Move the cursor to the given `row` and `col` (1-indexed). Returns `true` on success.

**Example:**
```kryos
use std::term

cursor_move(1, 1)   // top-left corner
cursor_move(10, 40)
```

---

### cursor_home

`cursor_home() -> bool`

Move the cursor to position (1, 1) -- the top-left corner. Returns `true` on success.

**Example:**
```kryos
use std::term

cursor_home()
```

---

### cursor_hide

`cursor_hide() -> bool`

Hide the cursor. Returns `true` on success. Restore with `cursor_show`.

**Example:**
```kryos
use std::term

cursor_hide()
// ... render UI ...
cursor_show()
```

---

### cursor_show

`cursor_show() -> bool`

Show the cursor. Returns `true` on success.

---

### cursor_save

`cursor_save() -> bool`

Save the current cursor position. Restore with `cursor_restore`. Returns `true` on success.

**Example:**
```kryos
use std::term

cursor_save()
cursor_move(5, 10)
// ... write something at (5, 10) ...
cursor_restore()   // return to saved position
```

---

### cursor_restore

`cursor_restore() -> bool`

Restore the cursor to the position saved by the most recent `cursor_save`. Returns `true` on success.

---

## Colors

Color functions return a styled string -- the original text wrapped in ANSI escape codes. Print the result to see the color. The terminal state is not modified persistently.

### color

`color(text: str, name: str) -> str`

Wrap `text` in ANSI color codes for the named color. Returns the styled string.

**Supported names:** `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `bright_black`, `bright_red`, `bright_green`, `bright_yellow`, `bright_blue`, `bright_magenta`, `bright_cyan`, `bright_white`.

**Example:**
```kryos
use std::term

println(color("error: file not found", "red"))
println(color("success", "green"))
println(color("warning", "yellow"))
```

---

### color_256

`color_256(text: str, code: i32) -> str`

Wrap `text` in a 256-color ANSI foreground code. `code` must be in the range `0-255`.

**Example:**
```kryos
use std::term

println(color_256("hello", 208))   // orange
println(color_256("world", 93))    // purple
```

**Reference:** Standard 256-color palette -- 0-7 are standard colors, 8-15 bright colors, 16-231 a 6x6x6 RGB cube, 232-255 a grayscale ramp.

---

### color_rgb

`color_rgb(text: str, r: i32, g: i32, b: i32) -> str`

Wrap `text` in a 24-bit RGB ANSI foreground code. `r`, `g`, `b` must each be in `0-255`.

**Example:**
```kryos
use std::term

println(color_rgb("vivid orange", 255, 140, 0))
println(color_rgb("deep blue", 0, 80, 200))
```

---

### bg_color

`bg_color(text: str, name: str) -> str`

Wrap `text` in an ANSI background color code. Accepts the same color names as `color`.

**Example:**
```kryos
use std::term

println(bg_color(" ALERT ", "red"))
println(bg_color(" OK ", "green"))
```

---

### bg_color_rgb

`bg_color_rgb(text: str, r: i32, g: i32, b: i32) -> str`

Wrap `text` in a 24-bit RGB ANSI background color code.

**Example:**
```kryos
use std::term

println(bg_color_rgb("highlighted", 50, 50, 120))
```

---

## Raw Mode

Raw mode disables line buffering and echo, allowing character-by-character key reading without the user pressing Enter.

### raw_enable

`raw_enable() -> bool`

Enable raw mode for stdin. Returns `true` on success.

**Example:**
```kryos
use std::term

raw_enable()
// read characters one at a time
raw_disable()
```

---

### raw_disable

`raw_disable() -> bool`

Disable raw mode and restore normal terminal settings. Returns `true` on success.

---

### with_raw_mode

`with_raw_mode(f: fn) -> str`

Enable raw mode, call `f` with no arguments, disable raw mode, and return whatever `f` returns as a string. Raw mode is always restored even if `f` throws.

**Example:**
```kryos
use std::term

let key = with_raw_mode(fn() -> str {
    // read a single character
    // return it
    return "q"
})
println(key)
```

**Note:** Use `with_raw_mode` over `raw_enable`/`raw_disable` when possible -- it guarantees the terminal is restored on error.

---

## Complete Example

```kryos
use std::term

// Clear screen and render a simple status dashboard
clear()
cursor_hide()

cursor_move(1, 1)
println(color("=== Status Dashboard ===", "cyan"))

cursor_move(3, 1)
println(color("Server:  ", "white") + color("running", "green"))

cursor_move(4, 1)
println(color("Workers: ", "white") + color_256("4 / 4", 82))

cursor_move(5, 1)
println(color("Errors:  ", "white") + color("0", "green"))

let w = width()
cursor_move(7, 1)
println(color(repeat("-", w), "bright_black"))

cursor_move(9, 1)
cursor_show()
```
