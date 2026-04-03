//! Terminal operations for the Kryos native stdlib.
//!
//! Uses `crossterm` for terminal manipulation: raw mode, cursor positioning,
//! terminal size, and screen clearing.

use crossterm::{
    cursor,
    execute,
    terminal::{self, ClearType},
};
use std::io::stdout;

/// Enables raw mode on the terminal.
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_term_raw_enable() -> i32 {
    match terminal::enable_raw_mode() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Disables raw mode on the terminal.
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_term_raw_disable() -> i32 {
    match terminal::disable_raw_mode() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Returns the terminal width (columns), or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_term_width() -> i32 {
    match terminal::size() {
        Ok((cols, _rows)) => cols as i32,
        Err(_) => -1,
    }
}

/// Returns the terminal height (rows), or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_term_height() -> i32 {
    match terminal::size() {
        Ok((_cols, rows)) => rows as i32,
        Err(_) => -1,
    }
}

/// Moves the cursor to the given (col, row) position (0-indexed).
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_term_cursor_move(col: u16, row: u16) -> i32 {
    match execute!(stdout(), cursor::MoveTo(col, row)) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Clears the terminal screen.
///
/// `mode`: 0 = all, 1 = current line, 2 = from cursor down.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_term_clear(mode: u8) -> i32 {
    let clear_type = match mode {
        0 => ClearType::All,
        1 => ClearType::CurrentLine,
        2 => ClearType::FromCursorDown,
        _ => return -1,
    };
    match execute!(stdout(), terminal::Clear(clear_type)) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
