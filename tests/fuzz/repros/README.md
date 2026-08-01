# Confirmed divergence repros

Minimal (post-`shrink.py`) repros for JIT/AOT stdout/exit-code divergences
found by this harness go here, one `.kry` file per finding, with a header
comment giving the originating seed, the divergence symptom, and the root
cause once known.

Empty as of the initial 1000+-seed sweep (see the wave report in
`tools/loop/LEDGER.md` for the honest count and rate) -- no case in that
sweep diverged. The one real bug this effort found while building the
harness's own generic-struct template (a trailing-underscore generic base
name breaking bare-passthrough methods) was NOT a backend divergence -- both
backends failed to build/link identically, pointing at shared MIR, not one
backend -- so its regression test lives at
`tests/conformance/conf_generic_underscore_name.kry` instead of here.
