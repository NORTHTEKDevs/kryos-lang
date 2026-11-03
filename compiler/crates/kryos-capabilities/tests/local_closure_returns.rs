//! Shape-layer tests for LEDGER item 77: the authority of the value a CALL
//! through a local closure returns is not the authority of invoking that
//! closure.
//!
//! These run the capability checker alone, on the source text, so a row-layer
//! (`kryos-types`) verdict can neither hide a shape-layer regression nor stand
//! in for it. `deny!(fs:read)` sits directly in `main`; the programs differ
//! only in how the privileged closure `reader` reaches the block.

use kryos_capabilities::{check_capabilities_mode, CapabilityMode};
use kryos_lexer::Lexer;

const PRELUDE: &str = r#"
fn harmless() -> str {
    return "not a secret"
}
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}
"#;

/// Build a program: the prelude, `decls`, then a `main` holding fs:read that
/// builds `reader`, runs `body`, and calls into `deny` inside `deny!(fs:read)`.
fn program(decls: &str, body: &str, deny: &str) -> String {
    let main = r#"
@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("secret.txt")
@@BODY@@
    deny!(fs:read) {
@@DENY@@
    }
}
"#;
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

/// Error codes of the capability checker for `src` in `mode`.
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

/// An attack must be rejected with E0507 (a call through a function value that
/// needs a capability the denied scope does not hold) in both enforcing modes.
/// Returns a description of every mode that accepted it.
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

/// A control (the same shape with a pure closure) must stay accepted.
/// Returns a description of every mode that rejected it.
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

struct Shape {
    name: &'static str,
    /// Whether the pure twin is accepted. The two `wrap` shapes are not: a local
    /// closure that forwards its parameter through a nested closure is charged
    /// `all` at every call by master as well (`call through a function value
    /// requires capabilities [all]`), so they only pin that the attack stays
    /// rejected.
    control: bool,
    decls: &'static str,
    /// `@@ARG@@` is the closure handed through the shape: `reader` for the
    /// attack, `harmless` for the control.
    body: &'static str,
    deny: &'static str,
}

const SHAPES: &[Shape] = &[
    Shape {
        name: "lambda returns its parameter (item 77a)",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet setter = |r: fn() -> str| { r }\nf = setter(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "nested fn returns its second parameter (item 77b)",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nfn pick(a: fn() -> str, b: fn() -> str) -> fn() -> str {\n    return b\n}\nf = pick(harmless, @@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "closure returns a captured closure",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet cap = @@ARG@@\nlet getter = || { cap }\nf = getter()",
        deny: "println(f())",
    },
    Shape {
        name: "closure returns a captured local that is rebound after the closure is defined",
        control: true,
        decls: "",
        body: "let mut cap: fn() -> str = harmless
let getter = || { cap }
cap = @@ARG@@
let f = getter()",
        deny: "println(f())",
    },
    Shape {
        name: "closure returns one of several parameters",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet choose = |c: bool, a: fn() -> str, b: fn() -> str| { if c { a } else { b } }\nf = choose(false, harmless, @@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "explicit returns of two parameters",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet choose = |c: bool, a: fn() -> str, b: fn() -> str| {\n    if c { return a }\n    return b\n}\nf = choose(false, harmless, @@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "result invoked in place",
        control: true,
        decls: "",
        body: "let setter = |r: fn() -> str| { r }",
        deny: "println(setter(@@ARG@@)())",
    },
    Shape {
        name: "result bound by let",
        control: true,
        decls: "",
        body: "let setter = |r: fn() -> str| { r }\nlet f = setter(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "result stored in a struct field",
        control: true,
        decls: "struct Holder {\n    run: fn() -> str,\n}\n",
        body: "let setter = |r: fn() -> str| { r }\nlet h = Holder { run: setter(@@ARG@@) }",
        deny: "println(h.run())",
    },
    Shape {
        name: "closure calls another local closure",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet id = |r: fn() -> str| { r }\nlet twice = |r: fn() -> str| { id(r) }\nf = twice(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "result handed to a function that calls it",
        control: true,
        decls: "fn run_it(g: fn() -> str) -> str {\n    return g()\n}\n",
        body: "let setter = |r: fn() -> str| { r }",
        deny: "println(run_it(setter(@@ARG@@)))",
    },
    Shape {
        name: "alias of a local closure",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet setter = |r: fn() -> str| { r }\nlet alias = setter\nf = alias(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "alias of a top-level function",
        control: true,
        decls: "fn pick_second(a: fn() -> str, b: fn() -> str) -> fn() -> str {\n    return b\n}\n",
        body: "let mut f: fn() -> str = harmless\nlet g = pick_second\nf = g(harmless, @@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "one name bound to two closures",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet mut op = |r: fn() -> str| { harmless }\nop = |r: fn() -> str| { r }\nf = op(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "two levels of local closures, applied through a local",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet mk = |a: fn() -> str| { |b: fn() -> str| { b } }\nlet inner = mk(harmless)\nf = inner(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "two levels of local closures, applied as one chain",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet mk = |a: fn() -> str| { |b: fn() -> str| { b } }\nf = mk(harmless)(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "a top-level function returns a closure that returns its captured parameter, bound to a local",
        control: true,
        decls: "fn make_getter(c: fn() -> str) -> fn() -> fn() -> str {\n    return || c\n}\n",
        body: "let mut f: fn() -> str = harmless\nlet g = make_getter(@@ARG@@)\nf = g()",
        deny: "println(f())",
    },
    Shape {
        name: "a top-level function returns a closure that returns its captured parameter, applied in place (item 128)",
        control: true,
        decls: "fn make_getter(c: fn() -> str) -> fn() -> fn() -> str {\n    return || c\n}\n",
        body: "let mut f: fn() -> str = harmless\nf = make_getter(@@ARG@@)()",
        deny: "println(f())",
    },
    Shape {
        name: "a local closure returns a closure it builds around a captured privileged local",
        control: true,
        decls: "",
        body: "let mut f: fn() -> str = harmless\nlet outer_c = |x: i64| {\n    let inner = || { @@ARG@@ }\n    return inner\n}\nlet h = outer_c(1)\nf = h()",
        deny: "println(f())",
    },
    Shape {
        name: "a generic wrapper returns its captured parameter, nested twice and applied twice",
        control: true,
        decls: "fn wrap<T>(x: T) -> fn() -> T {\n    return || x\n}\n",
        body: "let inner = wrap(@@ARG@@)\nlet outer = wrap(inner)\nlet f = outer()()",
        deny: "println(f())",
    },
    Shape {
        name: "closure returns a closure that calls its captured parameter",
        control: false,
        decls: "",
        body: "let wrap = |inner: fn() -> str| { || inner() }\nlet f = wrap(@@ARG@@)",
        deny: "println(f())",
    },
    Shape {
        name: "closure returns a closure that calls one of two captured parameters",
        control: false,
        decls: "",
        body: "let wrap = |a: fn() -> str, b: fn() -> str| { || { a() + b() } }\nlet f = wrap(harmless, @@ARG@@)",
        deny: "println(f())",
    },
];

#[test]
fn a_call_through_a_local_closure_charges_what_the_closure_returns() {
    let mut failures = Vec::new();
    for s in SHAPES {
        let attack = program(
            s.decls,
            &s.body.replace("@@ARG@@", "reader"),
            &s.deny.replace("@@ARG@@", "reader"),
        );
        failures.extend(not_rejected(s.name, &attack));
    }
    assert!(failures.is_empty(), "accepted attacks:
{}", failures.join("
"));
}

#[test]
fn the_pure_twin_of_each_shape_stays_accepted() {
    let mut failures = Vec::new();
    for s in SHAPES.iter().filter(|s| s.control) {
        let control = program(
            s.decls,
            &s.body.replace("@@ARG@@", "harmless"),
            &s.deny.replace("@@ARG@@", "harmless"),
        );
        failures.extend(not_accepted(s.name, &control));
    }
    assert!(failures.is_empty(), "rejected controls:
{}", failures.join("
"));
}

#[test]
fn a_recursive_nested_fn_returning_its_parameter_is_not_traced_and_fails_closed() {
    let src = program(
        "",
        "let mut f: fn() -> str = harmless
fn rec(n: i64, r: fn() -> str) -> fn() -> str {
    if n <= 0 { return r }
    return rec(n - 1, r)
}
f = rec(2, reader)",
        "println(f())",
    );
    let failures = not_rejected("recursive nested fn", &src);
    assert!(failures.is_empty(), "accepted attacks:
{}", failures.join("
"));
}

#[test]
fn a_parameter_bound_to_a_block_local_before_it_is_returned_is_not_traced_and_fails_closed() {
    let src = program(
        "",
        "let mut f: fn() -> str = harmless\nlet relay = |r: fn() -> str| {\n    let q = r\n    return q\n}\nf = relay(reader)",
        "println(f())",
    );
    let failures = not_rejected("parameter bound to a block local", &src);
    assert!(failures.is_empty(), "accepted attacks:\n{}", failures.join("\n"));
}

#[test]
fn a_curried_generic_function_applied_three_deep_over_pure_values_stays_accepted() {
    let src = program(
        "fn wrap<T>(x: T) -> fn() -> T {\n    return || x\n}\n",
        "let inner2 = wrap(42)\nlet mid2 = wrap(inner2)\nlet outer2 = wrap(mid2)\nlet deep: i64 = outer2()()()\nlet f: fn() -> str = harmless",
        "println(f() + to_string(deep))",
    );
    let failures = not_accepted("generic wrapper nested three deep", &src);
    assert!(failures.is_empty(), "rejected controls:\n{}", failures.join("\n"));
}

#[test]
fn a_curried_local_closure_whose_result_is_data_stays_accepted() {
    let src = program(
        "",
        "let add = |a: i64| { |b: i64| { a + b } }
let add2 = add(2)
let n = add2(3)
let f: fn() -> str = harmless",
        "println(f() + to_string(n))",
    );
    let failures = not_accepted("curried closure, data result", &src);
    assert!(failures.is_empty(), "rejected controls:
{}", failures.join("
"));
}
