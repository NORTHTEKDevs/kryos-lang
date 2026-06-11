// Canonical allocation-stress binary trees (same algorithm as kryos port).
struct Tree { left: Option<Box<Tree>>, right: Option<Box<Tree>> }

fn make(depth: i64) -> Tree {
    if depth == 0 { return Tree { left: None, right: None }; }
    Tree { left: Some(Box::new(make(depth - 1))), right: Some(Box::new(make(depth - 1))) }
}

fn check(t: &Tree) -> i64 {
    match &t.left {
        None => 1,
        Some(l) => 1 + check(l) + check(t.right.as_ref().unwrap()),
    }
}

fn main() {
    let max_depth: i64 = 16;
    let long_lived = make(max_depth);
    let mut total: i64 = 0;
    let mut depth: i64 = 4;
    while depth <= max_depth {
        let iterations: i64 = 1 << (max_depth - depth + 4);
        let mut sum: i64 = 0;
        for _ in 0..iterations { sum += check(&make(depth)); }
        total += sum;
        depth += 2;
    }
    total += check(&long_lived);
    println!("binary_trees(canonical, depth=16) checksum = {}", total);
}
