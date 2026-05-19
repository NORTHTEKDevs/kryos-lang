//! Integer math helpers beyond std::math: gcd, lcm, isqrt, integer log2,
//! popcount, leading/trailing zeros, primality test.

/// Greatest common divisor (Euclid). Returns abs if one input is zero.
#[no_mangle]
pub extern "C" fn kryos_math_gcd(a: i64, b: i64) -> i64 {
    let mut a = a.unsigned_abs();
    let mut b = b.unsigned_abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a as i64
}

/// Least common multiple. Returns 0 if either input is 0.
#[no_mangle]
pub extern "C" fn kryos_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        return 0;
    }
    let g = kryos_math_gcd(a, b);
    (a / g).saturating_mul(b.abs())
}

/// Integer square root (floor). Returns 0 for negative input.
#[no_mangle]
pub extern "C" fn kryos_math_isqrt(n: i64) -> i64 {
    if n < 0 {
        return 0;
    }
    let n = n as u64;
    (n as f64).sqrt() as i64
}

/// Integer log2 (floor). Returns -1 for n <= 0.
#[no_mangle]
pub extern "C" fn kryos_math_ilog2(n: i64) -> i64 {
    if n <= 0 {
        return -1;
    }
    63 - (n as u64).leading_zeros() as i64
}

/// Number of set bits.
#[no_mangle]
pub extern "C" fn kryos_math_popcount(n: i64) -> i64 {
    (n as u64).count_ones() as i64
}

/// Trailing zero bits.
#[no_mangle]
pub extern "C" fn kryos_math_trailing_zeros(n: i64) -> i64 {
    if n == 0 {
        return 64;
    }
    (n as u64).trailing_zeros() as i64
}

/// Leading zero bits.
#[no_mangle]
pub extern "C" fn kryos_math_leading_zeros(n: i64) -> i64 {
    (n as u64).leading_zeros() as i64
}

/// Probabilistic primality test (Miller-Rabin with deterministic
/// witness set for u64). Returns 1 if prime, 0 if composite.
#[no_mangle]
pub extern "C" fn kryos_math_is_prime(n: i64) -> i32 {
    if n < 2 {
        return 0;
    }
    let n = n as u64;
    for &p in &[2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n == p {
            return 1;
        }
        if n % p == 0 {
            return 0;
        }
    }
    // Miller-Rabin with witnesses sufficient for u64.
    let mut d = n - 1;
    let mut r = 0u32;
    while d & 1 == 0 {
        d >>= 1;
        r += 1;
    }
    let witnesses = [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    'outer: for a in witnesses {
        if a >= n {
            continue;
        }
        let mut x = mod_pow(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 0..r - 1 {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'outer;
            }
        }
        return 0;
    }
    1
}

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % m as u128) as u64
}

fn mod_pow(mut base: u64, mut exp: u64, m: u64) -> u64 {
    let mut result = 1u64;
    base %= m;
    while exp > 0 {
        if exp & 1 == 1 {
            result = mul_mod(result, base, m);
        }
        exp >>= 1;
        base = mul_mod(base, base, m);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_known() {
        assert_eq!(kryos_math_gcd(12, 18), 6);
        assert_eq!(kryos_math_gcd(7, 11), 1);
        assert_eq!(kryos_math_gcd(0, 5), 5);
        assert_eq!(kryos_math_gcd(-12, 18), 6);
    }

    #[test]
    fn lcm_known() {
        assert_eq!(kryos_math_lcm(4, 6), 12);
        assert_eq!(kryos_math_lcm(7, 11), 77);
        assert_eq!(kryos_math_lcm(0, 5), 0);
    }

    #[test]
    fn isqrt_floor() {
        assert_eq!(kryos_math_isqrt(0), 0);
        assert_eq!(kryos_math_isqrt(1), 1);
        assert_eq!(kryos_math_isqrt(15), 3);
        assert_eq!(kryos_math_isqrt(16), 4);
        assert_eq!(kryos_math_isqrt(99), 9);
    }

    #[test]
    fn bit_helpers() {
        assert_eq!(kryos_math_ilog2(1), 0);
        assert_eq!(kryos_math_ilog2(8), 3);
        assert_eq!(kryos_math_popcount(7), 3);
        assert_eq!(kryos_math_popcount(0), 0);
        assert_eq!(kryos_math_trailing_zeros(8), 3);
    }

    #[test]
    fn primes() {
        assert_eq!(kryos_math_is_prime(2), 1);
        assert_eq!(kryos_math_is_prime(17), 1);
        assert_eq!(kryos_math_is_prime(1009), 1);
        assert_eq!(kryos_math_is_prime(1_000_000_007), 1);
        assert_eq!(kryos_math_is_prime(4), 0);
        assert_eq!(kryos_math_is_prime(1), 0);
    }
}
