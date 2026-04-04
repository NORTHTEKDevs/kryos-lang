fn fib(n: i32) -> i32 {
    if n <= 1 { return n; }
    fib(n - 1) + fib(n - 2)
}

fn main() {
    let result = fib(42);
    let _ = result;
    println!("fib done");
}
