// rustc -O binary_trees.rs -o binary_trees_rs

fn tree_sum(depth: i64, value: i64) -> i64 {
    if depth == 0 {
        return value;
    }
    tree_sum(depth - 1, value * 2) + tree_sum(depth - 1, value * 2 + 1)
}

fn main() {
    let depth: i64 = 18;
    let s = tree_sum(depth, 1);
    println!("binary_trees(depth=18) = {}", s);
}
