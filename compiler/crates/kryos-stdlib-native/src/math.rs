//! Mathematical functions for the Kryos native stdlib.
//!
//! Provides basic math operations including trigonometric, logarithmic,
//! and utility functions.

/// Square root of x.
#[no_mangle]
pub extern "C" fn kryos_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Absolute value of a 64-bit float.
#[no_mangle]
pub extern "C" fn kryos_math_abs_f64(x: f64) -> f64 {
    x.abs()
}

/// Absolute value of a 64-bit signed integer.
#[no_mangle]
pub extern "C" fn kryos_math_abs_i64(x: i64) -> i64 {
    x.abs()
}

/// Floor of x.
#[no_mangle]
pub extern "C" fn kryos_math_floor(x: f64) -> f64 {
    x.floor()
}

/// Ceiling of x.
#[no_mangle]
pub extern "C" fn kryos_math_ceil(x: f64) -> f64 {
    x.ceil()
}

/// Rounds x to the nearest integer.
#[no_mangle]
pub extern "C" fn kryos_math_round(x: f64) -> f64 {
    x.round()
}

/// Raises base to the power of exp.
#[no_mangle]
pub extern "C" fn kryos_math_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

/// Natural logarithm (ln) of x.
#[no_mangle]
pub extern "C" fn kryos_math_log(x: f64) -> f64 {
    x.ln()
}

/// Base-2 logarithm (log2) of x.
#[no_mangle]
pub extern "C" fn kryos_math_log2(x: f64) -> f64 {
    x.log2()
}

/// Base-10 logarithm (log10) of x.
#[no_mangle]
pub extern "C" fn kryos_math_log10(x: f64) -> f64 {
    x.log10()
}

/// Sine of x (in radians).
#[no_mangle]
pub extern "C" fn kryos_math_sin(x: f64) -> f64 {
    x.sin()
}

/// Cosine of x (in radians).
#[no_mangle]
pub extern "C" fn kryos_math_cos(x: f64) -> f64 {
    x.cos()
}

/// Tangent of x (in radians).
#[no_mangle]
pub extern "C" fn kryos_math_tan(x: f64) -> f64 {
    x.tan()
}

/// Minimum of two 64-bit floats.
#[no_mangle]
pub extern "C" fn kryos_math_min_f64(a: f64, b: f64) -> f64 {
    a.min(b)
}

/// Maximum of two 64-bit floats.
#[no_mangle]
pub extern "C" fn kryos_math_max_f64(a: f64, b: f64) -> f64 {
    a.max(b)
}

/// Minimum of two 64-bit signed integers.
#[no_mangle]
pub extern "C" fn kryos_math_min_i64(a: i64, b: i64) -> i64 {
    a.min(b)
}

/// Maximum of two 64-bit signed integers.
#[no_mangle]
pub extern "C" fn kryos_math_max_i64(a: i64, b: i64) -> i64 {
    a.max(b)
}

/// Clamps x to the range [lo, hi].
#[no_mangle]
pub extern "C" fn kryos_math_clamp_f64(x: f64, lo: f64, hi: f64) -> f64 {
    x.clamp(lo, hi)
}

/// Returns the mathematical constant π.
#[no_mangle]
pub extern "C" fn kryos_math_pi() -> f64 {
    std::f64::consts::PI
}

/// Returns the mathematical constant e.
#[no_mangle]
pub extern "C" fn kryos_math_e() -> f64 {
    std::f64::consts::E
}
