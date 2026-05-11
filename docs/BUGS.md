## Known Issue: String-in-struct ownership leak across function returns

When a function returns a struct that contains a `str` field which was
assigned from another moved string, the returned struct may carry a stale
string handle, causing `len(field)` to return garbage memory.

**Reproducer**: see git history before commit that introduced
examples/showcase/agent_runtime.kry. The original version used a
`struct Action { kind, name, arg_s, arg_i }` returned from a planner
function — observed `len(action.arg_s) = 7305790164731371552` and
empty string content.

**Workaround**: pass strings via out-parameter arrays of length 1
(`[str]` slot), as agent_runtime.kry currently does.

**Owner**: tracked for v0.5 — needs investigation of ownership analyzer
+ MIR lowering when struct literal field is assigned from a previously
moved-from local.

