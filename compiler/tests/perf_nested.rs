fn main() {
    let mut total: i64 = 0;
    for i in 0..1000i64 {
        for j in 0..1000i64 {
            total += i * j;
        }
    }
    let _ = total;
    println!("nested done");
}
