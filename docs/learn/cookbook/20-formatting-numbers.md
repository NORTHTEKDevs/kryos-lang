# Cookbook 20 · Formatting numbers

`std::numfmt` ships hex / binary / zero-padded decimal / human-readable bytes. Output goes into caller-provided buffers — no allocation in the FFI layer.

## The program

```kryos
use std::numfmt::{fmt_hex, fmt_bin, fmt_decimal_padded, fmt_bytes}

@capabilities(io)
fn main() {
    println("hex:   " + fmt_hex(255))               // → 0xff
    println("bin:   " + fmt_bin(5))                 // → 0b101
    println("pad:   " + fmt_decimal_padded(42, 5))  // → 00042

    println("100 B size:    " + fmt_bytes(100))
    println("1500 B size:   " + fmt_bytes(1500))
    println("1.5 MB size:   " + fmt_bytes(1572864))
    println("1 GB size:     " + fmt_bytes(1073741824))
}
```

## Things to know

- All four functions return caller-allocated strings via `kryos-rt`'s string
  builder — no manual buffer management at the Kryos level.
- `fmt_bytes` reports in **binary** units (KB = 1024, MB = 1024², etc.).
  Use IEC suffixes (KiB/MiB) if your domain disambiguates from SI.
- `fmt_decimal_padded(42, 5)` returns `"00042"`. For width less than digits,
  you get the unpadded string.
- Negative values: hex / binary use sign-magnitude (`fmt_hex(-1) = -0x1`),
  bytes treats sign as informational ("−100 B").
