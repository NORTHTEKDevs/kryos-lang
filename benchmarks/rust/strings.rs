fn main() {
    let n = 100_000usize;
    let token = "ab";
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(token);
    }
    let slen = s.len();
    let checksum: i64 = s.bytes().map(|b| b as i64).sum();
    println!("{}", slen);
    println!("{}", checksum);
}
