//! Bit-level reinterpretation between `f64` and `i64`.
//!
//! Several runtime FFI functions return or accept an `f64` value transported
//! through an `i64` slot (e.g. `kryos_tensor_sum` returns the sum's bits as
//! `i64`, `kryos_tensor_set` takes the value's bits as `i64`). Kryos has no
//! bit-reinterpret operator of its own — `x as f64` is a numeric conversion,
//! not a bitcast — so stdlib wrappers need these two leaf functions to move a
//! value across the i64 transport without changing its bits.
//!
//! These are pure, total, and called only from stdlib `.kry` code; the
//! self-host compiler never references them, so they cannot affect the
//! bootstrap fixed point.

/// Reinterpret the bits of an `i64` as an `f64` (no numeric conversion).
#[no_mangle]
pub extern "C" fn kryos_f64_from_bits(bits: i64) -> f64 {
    f64::from_bits(bits as u64)
}

/// Reinterpret the bits of an `f64` as an `i64` (no numeric conversion).
#[no_mangle]
pub extern "C" fn kryos_f64_to_bits(value: f64) -> i64 {
    value.to_bits() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_value() {
        for v in [0.0f64, 1.0, -1.0, 3.14159, 1e300, -1e-300, f64::MAX, f64::MIN] {
            let bits = kryos_f64_to_bits(v);
            let back = kryos_f64_from_bits(bits);
            assert_eq!(v.to_bits(), back.to_bits(), "roundtrip changed {v}");
        }
    }

    #[test]
    fn nan_and_infinities_survive() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let back = kryos_f64_from_bits(kryos_f64_to_bits(v));
            // Compare bit patterns: NaN != NaN under ==.
            assert_eq!(v.to_bits(), back.to_bits());
        }
    }

    #[test]
    fn from_bits_matches_std() {
        // A known bit pattern: 1.0f64 == 0x3FF0000000000000.
        assert_eq!(kryos_f64_from_bits(0x3FF0000000000000u64 as i64), 1.0);
        assert_eq!(kryos_f64_to_bits(1.0), 0x3FF0000000000000u64 as i64);
    }
}
