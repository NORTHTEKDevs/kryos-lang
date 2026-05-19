# Cookbook 23 · Fuzzy search

`std::fuzzy` ships Levenshtein edit distance + Jaro–Winkler similarity.
Pair them with `std::trie` for autocomplete + `did you mean?` UX.

## The program

```kryos
use std::fuzzy::{fuzzy_levenshtein, fuzzy_jaro_winkler_x1000}
use std::trie::{trie_new, trie_insert, trie_has_prefix, trie_drop}

fn main() {
    let words = ["alice", "alicia", "bob", "carol", "carla", "dan"]

    // Build a prefix tree for fast prefix matching.
    let t = trie_new()
    for w in words {
        trie_insert(t, w)
    }

    let typo = "alise"

    // 1. Fast prefix check
    if trie_has_prefix(t, "ali") == 1 {
        println("got prefix \"ali\" matches")
    }

    // 2. Edit distance (smaller = closer)
    let mut best: str = ""
    let mut best_dist: i64 = 999
    for w in words {
        let d = fuzzy_levenshtein(typo, w)
        if d < best_dist {
            best_dist = d
            best = w
        }
    }
    println("typo `" + typo + "` closest by edit distance: `" + best + "` (d=" + to_string(best_dist) + ")")

    // 3. Jaro–Winkler (higher = closer; 1000 = identical)
    let mut best_jw: str = ""
    let mut best_score: i64 = 0
    for w in words {
        let s = fuzzy_jaro_winkler_x1000(typo, w)
        if s > best_score {
            best_score = s
            best_jw = w
        }
    }
    println("typo `" + typo + "` closest by Jaro-Winkler: `" + best_jw + "` (s=" + to_string(best_score) + "/1000)")

    trie_drop(t)
}
```

## When to use which

- **Trie** — exact prefix matching at large scale. O(prefix length) per query.
- **Levenshtein** — typo correction with sliding tolerance ("did you mean alice?").
  Quadratic time; bound input lengths.
- **Jaro–Winkler** — better for short strings with common prefixes
  (names, identifiers). Higher scores than Levenshtein for "close enough"
  matches and built-in prefix bonus.
