# kryos-notebook-caps demo

A literate notebook where every cell carries its compiler-verified capability
badge. Read it top to bottom: the badges tell you which cells are safe.

Cell 0 is pure compute -- it just defines `n`:

```kryos
let n: i64 = 21
println("n = " + to_string(n))
```

Cell 1 touches the filesystem (io). It also reads `n` from cell 0, proving
that state threads from one cell into the next:

```kryos
file_write("kryos_nb_demo_artifact.txt", "n is " + to_string(n))
println("wrote artifact for n=" + to_string(n))
```

Cell 2 is pure again and still sees `n` from cell 0 (threaded state), even when
cell 1 is skipped under --pure-only:

```kryos
println("double = " + to_string(n * 2))
```
