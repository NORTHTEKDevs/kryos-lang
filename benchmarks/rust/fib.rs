// Equivalent Rust impl. Build: rustc -O fib.rs -o fib_rs

fn fib(n: i64) -> i64 {
    if n < 2 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn main() {
    let n: i64 = 40;
    let r = fib(n);
    println!("fib(40) = {}", r);
}
