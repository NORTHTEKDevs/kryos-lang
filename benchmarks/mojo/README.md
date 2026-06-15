# Mojo benchmark ports

These are reference ports for the Modular **Mojo** toolchain. They are
**UNVERIFIED on this repo's CI host** because no Mojo toolchain is installed
here (Windows, no WSL distro / Modular SDK). `benchmarks/measure.py` builds and
times them automatically *iff* a `mojo` binary is on PATH (`mojo build <f>.mojo`);
otherwise the Mojo column is reported as `n/a` rather than fabricated.

To measure: install the Modular/Mojo SDK, ensure `mojo` is on PATH, then run
`python benchmarks/measure.py`. Verify each port prints the SAME checksum as the
Kryos/Rust/C++ ports before trusting timings (the harness does not currently
auto-cross-check Mojo output; do it by eye on first run).

Ports provided: fib, mandelbrot, matmul (compute kernels, most stable syntax).
nbody/fannkuch/binary_trees/hashmap are intentionally omitted until they can be
authored against a real toolchain.
