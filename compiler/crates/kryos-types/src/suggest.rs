//! "Did you mean?" suggestions using Levenshtein edit distance.

/// Compute the Levenshtein edit distance between two strings.
fn edit_distance(a: &str, b: &str) -> usize {
    let m = a.len();
    let n = b.len();
    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a.as_bytes()[i - 1] == b.as_bytes()[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Find the closest match to `name` from `candidates`.
/// Returns `None` if no candidate is within a reasonable edit distance.
pub fn closest_match<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let max_distance = match name.len() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };

    candidates
        .into_iter()
        .filter_map(|c| {
            let d = edit_distance(name, c);
            if d > 0 && d <= max_distance {
                Some((d, c))
            } else {
                None
            }
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_returns_none() {
        assert_eq!(closest_match("foo", ["foo"].iter().copied()), None);
    }

    #[test]
    fn off_by_one_returns_suggestion() {
        assert_eq!(
            closest_match("coutn", ["count", "counter", "zebra"].iter().copied()),
            Some("count".to_string())
        );
    }

    #[test]
    fn no_close_match_returns_none() {
        assert_eq!(
            closest_match("xyz", ["abc", "def", "ghi"].iter().copied()),
            None
        );
    }

    #[test]
    fn short_name_strict_threshold() {
        // "ab" -> max distance 1, "ac" is distance 1
        assert_eq!(
            closest_match("ab", ["ac", "zz"].iter().copied()),
            Some("ac".to_string())
        );
        // "ab" -> max distance 1, "zz" is distance 2 — too far
        assert_eq!(closest_match("ab", ["zz"].iter().copied()), None);
    }
}
