# UNVERIFIED reference port (no Mojo toolchain on CI host). See README.md.
fn fib(n: Int) -> Int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

fn main():
    print("fib(40) =", fib(40))
