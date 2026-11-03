# Mojo benchmark ports

These are reference ports for the Modular **Mojo** toolchain. They are
**UNVERIFIED on this repo's CI host** because no Mojo toolchain is installed
here (no Modular SDK, no `mojo` on PATH). `benchmarks/measure.py` builds and
times them automatically *iff* a `mojo` binary is on PATH (`mojo build <f>.mojo`);
otherwise the Mojo column is reported as `n/a` rather than fabricated.

To measure: install the Modular/Mojo SDK, ensure `mojo` is on PATH, then run
`python benchmarks/measure.py`, then `python benchmarks/check_outputs.py`, which
fails unless each Mojo port prints the same output as the other ports of its
benchmark. Do not trust the Mojo timings before that check passes.

Ports provided: fib, mandelbrot, matmul (compute kernels, most stable syntax).
nbody/fannkuch/binary_trees/hashmap are intentionally omitted until they can be
authored against a real toolchain.
