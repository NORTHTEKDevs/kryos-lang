//! Fuzzy-match helpers: Levenshtein edit distance, Jaro–Winkler.

/// Levenshtein distance between two byte slices. Cost: insert/delete/
/// substitute = 1.
#[no_mangle]
pub extern "C" fn kryos_fuzzy_levenshtein(
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
) -> i64 {
    if a.is_null() || b.is_null() {
        return -1;
    }
    let sa = unsafe { std::slice::from_raw_parts(a, a_len) };
    let sb = unsafe { std::slice::from_raw_parts(b, b_len) };
    let m = sa.len();
    let n = sb.len();
    if m == 0 {
        return n as i64;
    }
    if n == 0 {
        return m as i64;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if sa[i - 1] == sb[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n] as i64
}

/// Jaro–Winkler similarity, returned as a fixed-point integer in
/// 0..=1000 (1000 = identical). Useful for fuzzy autocomplete ranking.
#[no_mangle]
pub extern "C" fn kryos_fuzzy_jaro_winkler_x1000(
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
) -> i64 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    let sa = unsafe { std::slice::from_raw_parts(a, a_len) };
    let sb = unsafe { std::slice::from_raw_parts(b, b_len) };
    let m = sa.len();
    let n = sb.len();
    if m == 0 && n == 0 {
        return 1000;
    }
    if m == 0 || n == 0 {
        return 0;
    }
    let match_distance = (m.max(n) / 2).saturating_sub(1);
    let mut a_match = vec![false; m];
    let mut b_match = vec![false; n];
    let mut matches: usize = 0;
    for i in 0..m {
        let lo = i.saturating_sub(match_distance);
        let hi = (i + match_distance + 1).min(n);
        for j in lo..hi {
            if b_match[j] || sa[i] != sb[j] {
                continue;
            }
            a_match[i] = true;
            b_match[j] = true;
            matches += 1;
            break;
        }
    }
    if matches == 0 {
        return 0;
    }
    let mut k: usize = 0;
    let mut transpositions: usize = 0;
    for i in 0..m {
        if !a_match[i] {
            continue;
        }
        while !b_match[k] {
            k += 1;
        }
        if sa[i] != sb[k] {
            transpositions += 1;
        }
        k += 1;
    }
    let mt = matches as f64;
    let jaro = (mt / m as f64
        + mt / n as f64
        + (matches - transpositions / 2) as f64 / mt)
        / 3.0;
    // Winkler prefix bonus: up to 4 matching prefix chars, scale 0.1.
    let prefix = sa
        .iter()
        .zip(sb.iter())
        .take(4)
        .take_while(|(x, y)| x == y)
        .count() as f64;
    let jw = jaro + prefix * 0.1 * (1.0 - jaro);
    (jw * 1000.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lev(a: &str, b: &str) -> i64 {
        kryos_fuzzy_levenshtein(a.as_ptr(), a.len(), b.as_ptr(), b.len())
    }

    #[test]
    fn levenshtein_known_values() {
        assert_eq!(lev("kitten", "sitting"), 3);
        assert_eq!(lev("hello", "hello"), 0);
        assert_eq!(lev("", "abc"), 3);
        assert_eq!(lev("abc", ""), 3);
        assert_eq!(lev("flaw", "lawn"), 2);
    }

    fn jw(a: &str, b: &str) -> i64 {
        kryos_fuzzy_jaro_winkler_x1000(a.as_ptr(), a.len(), b.as_ptr(), b.len())
    }

    #[test]
    fn jaro_winkler_identical() {
        assert_eq!(jw("hello", "hello"), 1000);
    }

    #[test]
    fn jaro_winkler_close_match() {
        // "MARTHA" vs "MARHTA" — classic reference case.
        let score = jw("MARTHA", "MARHTA");
        assert!(score >= 950, "expected ≥0.95, got {score}/1000");
    }

    #[test]
    fn jaro_winkler_distant() {
        let score = jw("hello", "xyz");
        assert!(score < 500, "expected < 0.5, got {score}/1000");
    }
}
