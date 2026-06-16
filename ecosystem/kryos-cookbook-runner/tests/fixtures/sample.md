# Sample cookbook

A short Kryos overview with runnable examples.

## Hello World

A minimal annotated program with no extra capabilities.

```kryos
@capabilities()
fn main() {
    println("Hello, Kryos!")
}
```

## Arithmetic

Pure computation with no side effects beyond printing.

```kryos
@capabilities()
fn square(n: i64) -> i64 {
    return n * n
}

@capabilities()
fn main() {
    let x = square(7)
    println(to_string(x))
}
```

## Bad Example

This block has a type error and should be reported as FAIL.

```kryos
fn main() {
    let x: i64 = "not a number"
    println(x)
}
```

## IO Example

This block uses the io capability.

```kryos
@capabilities(io)
fn main() {
    file_write("tmp_cookbook_out.txt", "written by cookbook-runner")
    println("wrote file")
}
```
