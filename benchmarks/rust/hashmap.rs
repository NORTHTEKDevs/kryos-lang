use std::collections::HashMap;
fn main() {
    let n: i64 = 1_000_000;
    let mut m: HashMap<i64, i64> = HashMap::new();
    for i in 0..n { m.insert(i, i * 2); }
    let mut sum: i64 = 0;
    for j in 0..n { sum += m[&j]; }
    println!("{}", sum);
}
