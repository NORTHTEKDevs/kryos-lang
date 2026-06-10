# Cookbook 23 · Fuzzy search

`std::fuzzy` ships Levenshtein edit distance + Jaro–Winkler similarity.
Pair them with `std::trie` for autocomplete + `did you mean?` UX.

## The program

```kryos
use std::fuzzy::{levenshtein, closest}
use std::trie::{new_trie, insert, has_prefix, contains}

fn main() {
    let words = ["alice", "alicia", "bob", "carol", "carla", "dan"]

    // Build a prefix tree for fast prefix matching.
    let mut t = new_trie()
    for w in words {
        t = insert(t, w)
    }

    let typo = "alise"

    // 1. Fast prefix check
    if has_prefix(t, "ali") {
        println("got prefix \"ali\" matches")
    }

    // 2. Exact containment check
    if contains(t, "alice") {
        println("alice is in the trie")
    }

    // 3. Edit distance (smaller = closer)
    let mut best: str = ""
    let mut best_dist: i64 = 999
    for w in words {
        let d = levenshtein(typo, w)
        if d < best_dist {
            best_dist = d
            best = w
        }
    }
    println("typo `" + typo + "` closest by edit distance: `" + best + "` (d=" + to_string(best_dist) + ")")

    // 4. Closest helper (scans all candidates, returns index of best match)
    let idx = closest(typo, words, 5)
    if idx >= 0 {
        println("closest match: " + words[idx])
    }
}
```

## When to use which

- **Trie** — exact prefix/membership matching. O(word length) per query.
- **Levenshtein** — typo correction ("did you mean alice?"). Quadratic time; bound input lengths.
- **`closest`** — convenience wrapper over Levenshtein; returns the index of the best match within `max_dist`, or `-1` if none qualify.
