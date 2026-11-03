//! Shape-layer tests for LEDGER item 130: a `return` the return flow could not
//! see.
//!
//! `collect_return_exprs` listed the explicit returns of `if` / `for` / `while`
//! / `try` bodies only, so a `return r` inside a non-final `match`, a `select`
//! arm, a `deny!` block, a bare / `unsafe` block, or an `if` / `match` used as a
//! value was invisible: the return flow of a closure (LEDGER item 77) saw only
//! `return harmless`, and a call through it was charged nothing. Every
//! placement below hides the same `return` of the parameter that carries the
//! privileged closure, in every route a call can take to the returned value.
//!
//! The container tests at the end cover the other half of the same revision
//! (a value read out of a container local and handed through a local closure,
//! `id(reg.items[0])`): precise only where the program cannot change or alias
//! the container.
//!
//! These run the capability checker alone, on the source text, so a row-layer
//! (`kryos-types`) verdict can neither hide a shape-layer regression nor stand
//! in for it.

use kryos_capabilities::{check_capabilities_mode, CapabilityMode};
use kryos_lexer::Lexer;

const PRELUDE: &str = r#"
fn harmless() -> str {
    return "not a secret"
}
fn other() -> str {
    return "other"
}
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}
"#;

/// Build a program: the prelude, `decls`, then a `main` holding fs:read that
/// builds `reader`, runs `body`, and runs `deny` inside `deny!(fs:read)`.
fn program(decls: &str, body: &str, deny: &str) -> String {
    program_denying(decls, body, "fs:read", deny)
}

/// The same, with the block denying `denied` instead of `fs:read`.
fn program_denying(decls: &str, body: &str, denied: &str, deny: &str) -> String {
    let main = r#"
@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("secret.txt")
@@BODY@@
    deny!(@@DENIED@@) {
@@DENY@@
    }
}
"#
    .replace("@@DENIED@@", denied);
    let indent = |s: &str, n: usize| {
        s.lines()
            .map(|l| format!("{}{}", " ".repeat(n), l))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{PRELUDE}\n{decls}\n{}",
        main.replace("@@BODY@@", &indent(body, 4))
            .replace("@@DENY@@", &indent(deny, 8))
    )
}

fn codes(src: &str, mode: CapabilityMode) -> Vec<String> {
    let tokens = Lexer::new(src, 0).tokenize();
    let module = kryos_parser::parse(tokens).unwrap_or_else(|d| {
        panic!(
            "test program does not parse: {:?}\n{src}",
            d.iter().map(|x| x.message.clone()).collect::<Vec<_>>()
        )
    });
    check_capabilities_mode(&module, mode)
        .into_iter()
        .filter(|d| d.is_error())
        .filter_map(|d| d.code)
        .collect()
}

/// An attack must be rejected with E0507 in both enforcing modes.
fn not_rejected(name: &str, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for mode in [CapabilityMode::Inferred, CapabilityMode::Strict] {
        let c = codes(src, mode);
        if !c.iter().any(|x| x == "E0507") {
            out.push(format!("{name}: expected E0507 under {mode:?}, got {c:?}"));
        }
    }
    out
}

/// A control must stay accepted in both enforcing modes.
fn not_accepted(name: &str, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for mode in [CapabilityMode::Inferred, CapabilityMode::Strict] {
        let c = codes(src, mode);
        if !c.is_empty() {
            out.push(format!("{name}: expected no error under {mode:?}, got {c:?}"));
        }
    }
    out
}

/// Where the hidden `return r` sits. `c` is an `i64` parameter, `r` the
/// function-valued one.
const HIDDEN_PLACEMENTS: &[(&str, &str)] = &[
    (
        "non-final match statement",
        "match c {\n    1 => { return r }\n    _ => { }\n}",
    ),
    (
        "select arm",
        "let c2 = chan()\nsend(c2, 1)\nselect {\n    v c2 => { return r }\n}",
    ),
    ("deny! block", "deny!(net) {\n    return r\n}"),
    ("bare block", "{\n    if c == 1 { return r }\n}"),
    (
        "if expression bound by let",
        "let t = if c == 1 { return r } else { 0 }",
    ),
    ("unsafe block", "unsafe {\n    if c == 1 { return r }\n}"),
    (
        "match bound by let",
        "let t = match c {\n    1 => { return r }\n    _ => { 0 }\n}",
    ),
    (
        "call argument",
        "println(to_string(match c {\n    1 => { return r }\n    _ => { 0 }\n}))",
    ),
    (
        "match inside a select arm inside a deny! block",
        "deny!(net) {\n    let c2 = chan()\n    send(c2, 1)\n    select {\n        v c2 => {\n            match c {\n                1 => { return r }\n                _ => { }\n            }\n        }\n    }\n}",
    ),
    // The placements the return flow always saw: they pin that completing the
    // walk did not lose them.
    ("if statement", "if c == 1 { return r }"),
    ("while loop", "while c == 1 { return r }"),
    (
        "try block",
        "try {\n    if c == 1 { return r }\n} catch e {\n}",
    ),
];

struct Route {
    name: &'static str,
    /// Whether the pure twin (the same shape handed `harmless`) is accepted. A
    /// top-level function that returns a parameter on one path and a closure
    /// on another merges two answers `ClosureCapsResult` cannot represent as
    /// one set, so its result is charged `all` at every call (as it is for the
    /// same function with the return written where the walk always saw it), and
    /// the twin is rejected.
    twin_accepted: bool,
    decls: &'static str,
    /// `@@HIDE@@` stands for the hiding statement(s), indented as a block body;
    /// `@@ARG@@` is the closure handed through the route.
    body: &'static str,
    deny: &'static str,
}

const HIDDEN_ROUTES: &[Route] = &[
    Route {
        name: "local closure",
        twin_accepted: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet sel = |c: i64, r: fn() -> str| {\n@@HIDE@@\n    return harmless\n}\nf = sel(1, @@ARG@@)",
        deny: "println(f())",
    },
    Route {
        name: "nested fn",
        twin_accepted: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nfn sel(c: i64, r: fn() -> str) -> fn() -> str {\n@@HIDE@@\n    return harmless\n}\nf = sel(1, @@ARG@@)",
        deny: "println(f())",
    },
    Route {
        name: "alias of a local closure",
        twin_accepted: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet sel = |c: i64, r: fn() -> str| {\n@@HIDE@@\n    return harmless\n}\nlet g = sel\nf = g(1, @@ARG@@)",
        deny: "println(f())",
    },
    Route {
        name: "curried local closure",
        twin_accepted: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet mk = |a: fn() -> str| {\n    |c: i64, r: fn() -> str| {\n@@HIDE@@\n        return harmless\n    }\n}\nf = mk(harmless)(1, @@ARG@@)",
        deny: "println(f())",
    },
    Route {
        name: "result invoked in place",
        twin_accepted: true,
        decls: "",
        body: "let sel = |c: i64, r: fn() -> str| {\n@@HIDE@@\n    return harmless\n}",
        deny: "println(sel(1, @@ARG@@)())",
    },
    Route {
        name: "top-level function, direct call",
        twin_accepted: false,
        decls: "fn pick(c: i64, r: fn() -> str) -> fn() -> str {\n@@HIDE@@\n    return harmless\n}\n",
        body: "let mut f: fn() -> str = harmless\nf = pick(1, @@ARG@@)",
        deny: "println(f())",
    },
    Route {
        name: "alias of a top-level function",
        twin_accepted: false,
        decls: "fn pick(c: i64, r: fn() -> str) -> fn() -> str {\n@@HIDE@@\n    return harmless\n}\n",
        body: "let mut f: fn() -> str = harmless\nlet g = pick\nf = g(1, @@ARG@@)",
        deny: "println(f())",
    },
    Route {
        name: "top-level function with authority of its own, alias",
        twin_accepted: false,
        decls: "@capabilities(fs:read)\nfn pick(c: i64, r: fn() -> str) -> fn() -> str {\n    let cfg = file_read(\"secret.txt\")\n@@HIDE@@\n    return harmless\n}\n",
        body: "let mut f: fn() -> str = harmless\nlet g = pick\nf = g(1, @@ARG@@)",
        deny: "println(f())",
    },
    Route {
        name: "chain of a top-level function",
        twin_accepted: false,
        decls: "@capabilities(fs:read)\nfn mk(c: i64, r: fn() -> fn() -> str) -> fn() -> fn() -> str {\n@@HIDE@@\n    return || {\n        let cfg = file_read(\"secret.txt\")\n        harmless\n    }\n}\n",
        body: "let mut f: fn() -> str = harmless\nlet getter = || @@ARG@@\nf = mk(1, getter)()",
        deny: "println(f())",
    },
];

fn hide(route: &Route, placement: &str, arg: &str) -> String {
    let indent = |s: &str, n: usize| {
        s.lines()
            .map(|l| format!("{}{}", " ".repeat(n), l))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let hide_block = indent(placement, 4);
    let sub = |s: &str| s.replace("@@HIDE@@", &hide_block).replace("@@ARG@@", arg);
    program(&sub(route.decls), &sub(route.body), &sub(route.deny))
}

#[test]
fn a_return_hidden_in_any_statement_or_expression_is_charged_to_the_call() {
    let mut failures = Vec::new();
    let mut n = 0;
    for route in HIDDEN_ROUTES {
        for (pname, placement) in HIDDEN_PLACEMENTS {
            let src = hide(route, placement, "reader");
            let label = format!("{} / {}", route.name, pname);
            failures.extend(not_rejected(&label, &src));
            n += 1;
        }
    }
    assert!(n >= 100, "the table shrank: {n} programs");
    assert!(
        failures.is_empty(),
        "accepted attacks ({} of {n} programs x 2 modes):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_pure_twin_of_a_hidden_return_through_a_local_closure_stays_accepted() {
    let mut failures = Vec::new();
    for route in HIDDEN_ROUTES.iter().filter(|r| r.twin_accepted) {
        for (pname, placement) in HIDDEN_PLACEMENTS {
            let src = hide(route, placement, "harmless");
            let label = format!("{} / {}", route.name, pname);
            failures.extend(not_accepted(&label, &src));
        }
    }
    assert!(failures.is_empty(), "rejected controls:\n{}", failures.join("\n"));
}

#[test]
fn a_tail_value_the_return_flow_always_saw_is_still_charged() {
    // Tail `match`, tail `if`, tail block: the value of the body, not a `return`.
    let tails: &[(&str, &str)] = &[
        (
            "tail match",
            "|c: i64, r: fn() -> str| {\n    match c {\n        1 => { r }\n        _ => { harmless }\n    }\n}",
        ),
        (
            "tail if",
            "|c: i64, r: fn() -> str| {\n    if c == 1 { r } else { harmless }\n}",
        ),
    ];
    let mut failures = Vec::new();
    for (name, closure) in tails {
        let attack = program(
            "",
            &format!("let mut f: fn() -> str = harmless\nlet sel = {closure}\nf = sel(1, reader)"),
            "println(f())",
        );
        failures.extend(not_rejected(name, &attack));
        let twin = program(
            "",
            &format!("let mut f: fn() -> str = harmless\nlet sel = {closure}\nf = sel(1, harmless)"),
            "println(f())",
        );
        failures.extend(not_accepted(&format!("{name} (pure twin)"), &twin));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---------------------------------------------------------------------------
// A value read out of a container local and handed through a local closure.
// ---------------------------------------------------------------------------

struct Case {
    name: &'static str,
    decls: &'static str,
    body: &'static str,
    deny: &'static str,
}

const REGISTRY: &str = "struct Registry {\n    items: [fn() -> str],\n}\n";

/// Programs that must stay ACCEPTED: the pure registry of the verifier of
/// PR 41 and its natural neighbours. Each one was accepted by master (a call
/// through a local closure was charged what invoking it needs) and must be
/// accepted now that the call is charged what it returns.
const CONTAINER_CONTROLS: &[Case] = &[
    Case {
        name: "struct registry read through an identity closure",
        decls: REGISTRY,
        body: "let reg = Registry { items: [harmless, other] }\nlet id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[0])\nprintln(h())",
    },
    Case {
        name: "array local read through an identity closure",
        decls: "",
        body: "let items = [harmless, other]\nlet id = |g: fn() -> str| { g }",
        deny: "let h = id(items[1])\nprintln(h())",
    },
    Case {
        name: "a closure that looks an element up",
        decls: "",
        body: "let items = [harmless, other]\nlet get = |i: i64| { items[i] }",
        deny: "let h = get(1)\nprintln(h())",
    },
    Case {
        name: "a closure that looks a handler up in a map",
        decls: "",
        body: "let handlers = {\"a\": harmless, \"b\": other}\nlet lookup = |k: str| { handlers[k] }",
        deny: "let h = lookup(\"b\")\nprintln(h())",
    },
    Case {
        name: "registry of lambda literals",
        decls: REGISTRY,
        body: "let reg = Registry { items: [|| \"a\", || \"b\"] }\nlet id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[1])\nprintln(h())",
    },
    Case {
        name: "len and a for loop over the registry do not make it unreadable",
        decls: REGISTRY,
        body: "let reg = Registry { items: [harmless, other] }\nlet id = |g: fn() -> str| { g }\nlet n = len(reg.items)\nlet m = len(reg.items)\nfor it in reg.items {\n    println(\"item\")\n}",
        deny: "let h = id(reg.items[0])\nprintln(h() + to_string(n + m))",
    },
    Case {
        name: "registry of local closures that compute from literals",
        decls: REGISTRY,
        body: "let a = || \"a\"
let b = || \"b\"
let reg = Registry { items: [a, b] }
let id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[0])
println(h())",
    },
    Case {
        name: "registry built by a factory without parameters",
        decls: "struct Registry {
    items: [fn() -> str],
}
fn make_reg() -> Registry {
    return Registry { items: [harmless, other] }
}
",
        body: "let reg = make_reg()
let id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[1])
println(h())",
    },
    Case {
        name: "registry declared mut and never changed",
        decls: REGISTRY,
        body: "let mut reg = Registry { items: [harmless, other] }
let id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[0])
println(h())",
    },
    Case {
        name: "a parameter forwarded through an identity closure",
        decls: "fn run(g: fn() -> str) -> str {\n    let id = |x: fn() -> str| { x }\n    let h = id(g)\n    return h()\n}\n",
        body: "",
        deny: "println(run(harmless))",
    },
];

/// Programs that must be REJECTED: the privileged closure reaches the block
/// through a container the flow must not trust.
const CONTAINER_ATTACKS: &[Case] = &[
    Case {
        name: "the privileged closure sits in the registry (a local is not a safe leaf)",
        decls: REGISTRY,
        body: "let reg = Registry { items: [harmless, reader] }\nlet id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[1])\nprintln(h())",
    },
    Case {
        name: "a closure parameter shadows the pristine array",
        decls: "",
        body: "let items = [harmless, other]\nlet id = |g: fn() -> str| { g }\nlet run = |items: [fn() -> str]| { id(items[0]) }\nlet h = run([reader, harmless])",
        deny: "println(h())",
    },
    Case {
        name: "an inner let shadows the pristine array (inner first)",
        decls: "",
        body: "let id = |g: fn() -> str| { g }\nlet run = |n: i64| {\n    let items = [reader]\n    return id(items[0])\n}\nlet items = [harmless, other]\nlet h = run(1)",
        deny: "println(h())",
    },
    Case {
        name: "an inner let shadows the pristine array (inner last)",
        decls: "",
        body: "let items = [harmless, other]\nlet id = |g: fn() -> str| { g }\nlet run = |n: i64| {\n    let items = [reader]\n    return id(items[0])\n}\nlet h = run(1)",
        deny: "println(h())",
    },
    Case {
        name: "a factory-built registry that holds a privileged named function",
        decls: "struct Registry {
    items: [fn() -> str],
}
@capabilities(fs:read)
fn read_secret() -> str {
    return file_read(\"secret.txt\")
}
fn make_reg() -> Registry {
    return Registry { items: [read_secret] }
}
",
        body: "let reg = make_reg()
let id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[0])
println(h())",
    },
    Case {
        name: "a registry leaf that is a local closure which reads the file",
        decls: REGISTRY,
        body: "let a = || file_read(\"secret.txt\")
let reg = Registry { items: [harmless, a] }
let id = |g: fn() -> str| { g }",
        deny: "let h = id(reg.items[1])
println(h())",
    },
    Case {
        name: "a registry leaf closure that calls a local the program rebinds after the closure (the build order of the flat map)",
        decls: REGISTRY,
        body: "let mut g: fn() -> str = harmless
let a = || g()
let reg = Registry { items: [a] }
let id = |g2: fn() -> str| { g2 }
g = reader",
        deny: "let h = id(reg.items[0])
println(h())",
    },
    Case {
        name: "a mut registry is reassigned",
        decls: REGISTRY,
        body: "let mut reg = Registry { items: [harmless] }
let id = |g: fn() -> str| { g }
reg = Registry { items: [reader] }",
        deny: "let h = id(reg.items[0])
println(h())",
    },
    Case {
        name: "an element of a mut registry is overwritten",
        decls: REGISTRY,
        body: "let mut reg = Registry { items: [harmless] }
let id = |g: fn() -> str| { g }
reg.items[0] = reader",
        deny: "let h = id(reg.items[0])
println(h())",
    },
    Case {
        name: "a for-loop variable shadows the pristine array",
        decls: "",
        body: "let items = [harmless, other]\nlet id = |g: fn() -> str| { g }\nlet mut h: fn() -> str = harmless\nfor items in [[reader]] {\n    h = id(items[0])\n}",
        deny: "println(h())",
    },
    Case {
        name: "a nested fn parameter shadows the pristine array",
        decls: "",
        body: "let items = [harmless, other]\nlet id = |g: fn() -> str| { g }\nfn inner(items: [fn() -> str]) -> fn() -> str {\n    return id(items[0])\n}\nlet h = inner([reader])",
        deny: "println(h())",
    },
    Case {
        name: "an inner let of a privileged named function shadows the pristine array (inner first)",
        decls: "@capabilities(fs:read)\nfn read_secret() -> str {\n    return file_read(\"secret.txt\")\n}\n",
        body: "let id = |g: fn() -> str| { g }\nlet run = |n: i64| {\n    let items = [read_secret]\n    return id(items[0])\n}\nlet items = [harmless, other]\nlet h = run(1)",
        deny: "println(h())",
    },
    Case {
        name: "an inner let of a privileged named function shadows the pristine array (inner last)",
        decls: "@capabilities(fs:read)\nfn read_secret() -> str {\n    return file_read(\"secret.txt\")\n}\n",
        body: "let items = [harmless, other]\nlet id = |g: fn() -> str| { g }\nlet run = |n: i64| {\n    let items = [read_secret]\n    return id(items[0])\n}\nlet h = run(1)",
        deny: "println(h())",
    },
    Case {
        name: "a registry leaf that aliases a local the loop rebinds (the build order of the flat map, item 129)",
        decls: REGISTRY,
        body: "let mut f: fn() -> str = harmless\nlet mut h: fn() -> str = harmless\nlet id = |g: fn() -> str| { g }\nlet mut i = 0\nwhile i < 2 {\n    let g = f\n    let reg = Registry { items: [g] }\n    h = id(reg.items[0])\n    f = reader\n    i = i + 1\n}",
        deny: "println(h())",
    },
    Case {
        name: "a callee grows the array in place",
        decls: "fn fill(a: [fn() -> str], r: fn() -> str) {\n    push(a, r)\n}\n",
        body: "let items = [harmless]\nlet id = |g: fn() -> str| { g }\nfill(items, reader)",
        deny: "let h = id(items[1])\nprintln(h())",
    },
    Case {
        name: "the array is aliased before it is grown",
        decls: "",
        body: "let items = [harmless]\nlet id = |g: fn() -> str| { g }\nlet alias = items\npush(alias, reader)",
        deny: "let h = id(items[1])\nprintln(h())",
    },
    Case {
        name: "a mutable array is pushed to",
        decls: "",
        body: "let mut items = [harmless]\nlet id = |g: fn() -> str| { g }\nitems = push(items, reader)",
        deny: "let h = id(items[1])\nprintln(h())",
    },
    Case {
        name: "a slice of the array is taken and grown",
        decls: "",
        body: "let items = [harmless, other]\nlet id = |g: fn() -> str| { g }\nlet part = items[0..1]\npush(part, reader)",
        deny: "let h = id(items[1])\nprintln(h())",
    },
    Case {
        name: "a sub-container is aliased and grown",
        decls: "",
        body: "let grid = [[harmless]]\nlet id = |g: fn() -> str| { g }\nlet row = grid[0]\npush(row, reader)",
        deny: "let h = id(grid[0][1])\nprintln(h())",
    },
    Case {
        name: "the registry field is passed to a function that grows it",
        decls: "struct Registry {\n    items: [fn() -> str],\n}\nfn fill(a: [fn() -> str], r: fn() -> str) {\n    push(a, r)\n}\n",
        body: "let reg = Registry { items: [harmless] }\nlet id = |g: fn() -> str| { g }\nfill(reg.items, reader)",
        deny: "let h = id(reg.items[1])\nprintln(h())",
    },
    Case {
        name: "a closure that looks an element up, over a registry holding the privileged closure",
        decls: "",
        body: "let items = [harmless, reader]\nlet get = |i: i64| { items[i] }",
        deny: "let h = get(1)\nprintln(h())",
    },
    Case {
        name: "the element is read before the let that binds the array shadows a parameter",
        decls: "fn run(items: [fn() -> str]) -> str {\n    let id = |g: fn() -> str| { g }\n    let h = id(items[0])\n    let items = [harmless]\n    return h()\n}\n",
        body: "",
        deny: "println(run([reader]))",
    },
    Case {
        name: "a container parameter's element handed through an identity closure",
        decls: "fn pick(items: [fn() -> str], i: i64) -> fn() -> str {\n    let id = |g: fn() -> str| { g }\n    return id(items[i])\n}\n",
        body: "let items = [harmless, reader]",
        deny: "let h = pick(items, 1)\nprintln(h())",
    },
];

/// The measured cost of the container rule, pinned so that a change is a decision:
/// the verifier of PR 41 reported this program (its P7) as a legitimate program the
/// PR rejects. Master accepts it, and it also accepts the same function called with
/// `[harmless, reader]`, which prints the secret: the call-site argument is the only
/// difference, and the checker does not carry a container parameter's elements from
/// the call site into the function's return. The direct form (`return items[i]`) is
/// rejected by master (E0507 [all]) for the same reason.
#[test]
fn a_container_parameters_element_is_charged_all_even_over_pure_values() {
    let pure = program(
        "fn pick(items: [fn() -> str], i: i64) -> fn() -> str {\n    let id = |g: fn() -> str| { g }\n    return id(items[i])\n}\n",
        "let items = [harmless, other]",
        "let h = pick(items, 1)\nprintln(h())",
    );
    let direct = program(
        "fn pick(items: [fn() -> str], i: i64) -> fn() -> str {\n    return items[i]\n}\n",
        "let items = [harmless, other]",
        "let h = pick(items, 1)\nprintln(h())",
    );
    let mut failures = not_rejected("container parameter element via an identity closure, pure", &pure);
    failures.extend(not_rejected("container parameter element returned directly, pure", &direct));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A registry that holds a privileged closure is charged what it holds, exactly like
/// the direct read `reg.items[1]()`: the block that denies another capability may use
/// it (the programs L3e and L3g of the verifier of PR 41, accepted by master), the block
/// that denies the capability it needs may not.
#[test]
fn a_registry_holding_a_privileged_closure_is_charged_what_it_holds() {
    let body = "let reg = Registry { items: [harmless, reader] }\nlet id = |g: fn() -> str| { g }";
    let deny = "let h = id(reg.items[1])\nprintln(h())";
    let deny0 = "let h = id(reg.items[0])\nprintln(h())";
    let mut failures = Vec::new();
    failures.extend(not_accepted(
        "privileged registry, element 1, under deny!(net)",
        &program_denying(REGISTRY, body, "net", deny),
    ));
    failures.extend(not_accepted(
        "privileged registry, element 0, under deny!(net)",
        &program_denying(REGISTRY, body, "net", deny0),
    ));
    // The index is not tracked: element 0 is as privileged as element 1.
    failures.extend(not_rejected(
        "privileged registry, element 0, under deny!(fs:read)",
        &program(REGISTRY, body, deny0),
    ));
    failures.extend(not_rejected(
        "privileged registry, element 1, under deny!(fs:read)",
        &program(REGISTRY, body, deny),
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_pure_container_read_through_a_local_closure_stays_accepted() {
    let mut failures = Vec::new();
    for c in CONTAINER_CONTROLS {
        failures.extend(not_accepted(c.name, &program(c.decls, c.body, c.deny)));
    }
    assert!(failures.is_empty(), "rejected controls:\n{}", failures.join("\n"));
}

#[test]
fn a_container_the_flow_cannot_trust_is_charged_to_the_call() {
    let mut failures = Vec::new();
    for c in CONTAINER_ATTACKS {
        failures.extend(not_rejected(c.name, &program(c.decls, c.body, c.deny)));
    }
    assert!(failures.is_empty(), "accepted attacks:\n{}", failures.join("\n"));
}
