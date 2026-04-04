# std::term

Terminal control: screen management, cursor positioning, ANSI colors, text styling, and raw key input.

All functions in this module are available after `use std::term`. Functions emit ANSI escape sequences, so they work in any terminal emulator that supports them. On Windows, raw key reading uses `msvcrt`; on Unix it uses `termios`.

---

## Output Control

### term_clear

```
term_clear() -> none
```

Clear the entire terminal screen and move the cursor to the top-left corner (row 1, column 1).

**Example:**

```kryos
term_clear()
println("Fresh screen")
```

---

### term_write

```
term_write(text: str) -> none
```

Write text directly to stdout without a trailing newline. The output is flushed immediately.

**Example:**

```kryos
term_write("Loading")
term_write(".")
term_write(".")
term_write(".")
println("")  // now add the newline
```

**Edge cases:**

- Unlike `print` and `println`, this writes raw text with no formatting.

**See also:** `term_flush`

---

### term_flush

```
term_flush() -> none
```

Flush stdout. Useful after writing partial output that you want to appear immediately.

**Example:**

```kryos
term_write("Processing... ")
// do work
term_flush()
```

**Edge cases:**

- `term_write` already flushes after every call, so you only need this after using lower-level output.

---

### term_move

```
term_move(row: i32, col: i32) -> none
```

Move the cursor to a specific row and column. Coordinates are 1-based (top-left is row 1, column 1).

**Example:**

```kryos
term_clear()
term_move(5, 10)
term_write("Hello at row 5, col 10")
```

**Edge cases:**

- Values outside the terminal dimensions are accepted but may have no visible effect.

**See also:** `term_size`

---

### term_hide_cursor

```
term_hide_cursor() -> none
```

Hide the terminal cursor. Useful for full-screen TUI applications.

**Example:**

```kryos
term_hide_cursor()
// draw UI without cursor blinking
term_show_cursor()  // always restore before exiting
```

**See also:** `term_show_cursor`

---

### term_show_cursor

```
term_show_cursor() -> none
```

Show the terminal cursor. Call this to restore the cursor after `term_hide_cursor`.

**Example:**

```kryos
term_show_cursor()
```

**See also:** `term_hide_cursor`

---

## Screen Management

### term_alt_screen

```
term_alt_screen() -> none
```

Switch to the alternate screen buffer. The current screen content is preserved and restored when you switch back. Used by full-screen TUI applications.

**Example:**

```kryos
term_alt_screen()
term_clear()
// draw full-screen UI
// when done:
term_main_screen()
```

**Edge cases:**

- Always pair with `term_main_screen` to restore the original screen.

**See also:** `term_main_screen`

---

### term_main_screen

```
term_main_screen() -> none
```

Switch back to the main screen buffer, restoring whatever was on screen before `term_alt_screen` was called.

**Example:**

```kryos
term_main_screen()
```

**See also:** `term_alt_screen`

---

### term_size

```
term_size() -> [i32]
```

Return the terminal dimensions as a two-element array: `[columns, lines]`.

**Example:**

```kryos
let size = term_size()
let cols = size[0]
let rows = size[1]
println("Terminal is " + to_string(cols) + "x" + to_string(rows))
```

**Edge cases:**

- Returns `[80, 24]` as a fallback if the terminal size cannot be detected (e.g., when stdout is redirected to a file).

**See also:** `term_move`

---

## Styling

### term_color

```
term_color(fg: str, bg: str?) -> none
```

Set the foreground (and optionally background) text color for subsequent output. Colors are applied via ANSI codes and persist until `term_reset` is called.

Available colors: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`.

**Example:**

```kryos
term_color("red")
println("This is red text")
term_color("white", "blue")
println("White text on blue background")
term_reset()
println("Back to normal")
```

**Edge cases:**

- Color names are case-insensitive.
- An unrecognized color name defaults to white (foreground) or white (background).
- Colors persist across print calls until `term_reset`.

**See also:** `term_reset`, `term_rgb`

---

### term_reset

```
term_reset() -> none
```

Reset all terminal styling (colors, bold, dim, underline) back to the default.

**Example:**

```kryos
term_color("green")
term_write("green ")
term_reset()
term_write("normal")
println("")
```

**See also:** `term_color`, `term_bold`

---

### term_bold

```
term_bold(text: str) -> str
```

Wrap text in ANSI bold escape codes. Returns the styled string -- does not print it.

**Example:**

```kryos
println(term_bold("Important message"))
```

```kryos
let header = term_bold("STATUS REPORT")
println(header)
```

**See also:** `term_dim`, `term_underline`

---

### term_dim

```
term_dim(text: str) -> str
```

Wrap text in ANSI dim (faint) escape codes. Returns the styled string.

**Example:**

```kryos
println(term_dim("Secondary information"))
```

**See also:** `term_bold`, `term_underline`

---

### term_underline

```
term_underline(text: str) -> str
```

Wrap text in ANSI underline escape codes. Returns the styled string.

**Example:**

```kryos
println(term_underline("Click here"))
```

**See also:** `term_bold`, `term_dim`

---

### term_rgb

```
term_rgb(r: i32, g: i32, b: i32, text: str) -> str
```

Apply a 24-bit RGB foreground color to text. Returns the styled string. Requires a terminal that supports true color (most modern terminals do).

**Example:**

```kryos
let orange = term_rgb(255, 165, 0, "Warning!")
println(orange)
```

```kryos
// Kryos brand blue
println(term_rgb(10, 132, 255, "Kryos"))
```

**Edge cases:**

- RGB values should be 0-255. Values outside this range may produce unexpected results.
- Falls back to the nearest ANSI color on terminals without true color support.

**See also:** `term_color`

---

## Input

### term_raw_mode

```
term_raw_mode(enabled: bool) -> none
```

Enable or disable raw terminal mode. In raw mode, input is not line-buffered and special keys (Ctrl+C, etc.) are not intercepted by the shell.

**Example:**

```kryos
term_raw_mode(true)
// read individual keystrokes
term_raw_mode(false)
```

**Edge cases:**

- On Windows, this is a no-op. Raw mode is handled per-read by `term_read_key`.
- On Unix, uses `termios` to toggle raw mode.
- Always restore raw mode to `false` before your program exits, or the terminal will be left in a broken state.

**See also:** `term_read_key`

---

### term_read_key

```
term_read_key() -> str
```

Read a single keypress from the terminal. Blocks until a key is pressed. Returns a string identifying the key.

Regular characters return themselves (e.g., `"a"`, `"Z"`, `"1"`). Special keys return named strings:

| Key | Return value |
|-----|-------------|
| Enter | `"Enter"` |
| Tab | `"Tab"` |
| Escape | `"Escape"` |
| Backspace | `"Backspace"` |
| Arrow Up | `"ArrowUp"` |
| Arrow Down | `"ArrowDown"` |
| Arrow Left | `"ArrowLeft"` |
| Arrow Right | `"ArrowRight"` |
| Home | `"Home"` |
| End | `"End"` |
| Page Up | `"PageUp"` |
| Page Down | `"PageDown"` |
| Delete | `"Delete"` |
| Insert | `"Insert"` |
| F1-F12 | `"F1"` through `"F12"` |
| Ctrl+letter | `"Ctrl+a"` through `"Ctrl+z"` |

**Example:**

```kryos
println("Press a key:")
let key = term_read_key()
println("You pressed: " + key)
```

```kryos
// Simple key handler loop
let mut running = true
while running {
    let key = term_read_key()
    if key == "q" {
        running = false
    } elif key == "ArrowUp" {
        println("Up!")
    } elif key == "ArrowDown" {
        println("Down!")
    }
}
```

**Edge cases:**

- On Unix, temporarily enters raw mode for the duration of the read, then restores the terminal.
- On Windows, uses `msvcrt.getwch()` for key reading.
- Unknown extended key sequences return `"Unknown(code)"`.
- Blocks indefinitely until a key is pressed.

**See also:** `term_raw_mode`
