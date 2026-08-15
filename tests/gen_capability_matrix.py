#!/usr/bin/env python3
"""Generate the capability-laundering shape matrix.

WHY: all 17 shapes in `tools/loop/escape_status.sh` were found BY HAND, by
directed adversarial search. That is exactly the corpus whose weakness is "the
shape nobody thought of". This enumerates the space combinatorially instead, so
the claim becomes "no counterexample in a systematically enumerated space of
size N" rather than "nobody has thought of another one".

Each generated program routes a PRIVILEGED closure (one that closes over
`file_read`) from a SOURCE, through a CONTAINER, along a TRANSPORT, to an
INVOKE site, inside a `deny!(fs:read)` block. Enforcement must REJECT it.

Every attack has a twin CONTROL that is byte-identical except the closure is
pure (`|| "no secrets"`). The control must COMPILE with no annotation. Without
that twin a gate can be trivially satisfied by rejecting everything, which is
the failure mode that makes an over-rejecting checker look "secure".

Language rules that constrain the generated source (CLAUDE.md):
  - no semicolons; newline terminates a statement
  - never start a line with `-`, `(`, `[`, `|`, `||` -- it CONTINUES the
    previous expression silently
  - closures are `|x|` / `||`, never `(x) =>`
  - `elif`, not `else if`
  - actors are constructed `Name()`, never `Name { .. }`
  - no struct-style enum variants
"""
import argparse
import itertools
import os
import sys

PRIV_BODY = 'return || file_read(path)'
PURE_BODY = 'return || "no secrets here"'


def source(kind, priv):
    """How the fn-value is produced. Returns (decls, expr_producing_the_fn)."""
    body = PRIV_BODY if priv else PURE_BODY
    if kind == 'direct':
        return ([
            '@capabilities(fs:read)' if priv else '',
            'fn make_fn(path: str) -> fn() -> str {',
            f'    {body}',
            '}',
        ], 'make_fn("secret.txt")')
    if kind == 'wrapper':
        # a zero-capability wrapper that just forwards -- LEDGER item 10's shape
        return ([
            '@capabilities(fs:read)' if priv else '',
            'fn inner_fn(path: str) -> fn() -> str {',
            f'    {body}',
            '}',
            'fn make_fn(path: str) -> fn() -> str {',
            '    let f = inner_fn(path)',
            '    return || f()',
            '}',
        ], 'make_fn("secret.txt")')
    if kind == 'generic':
        return ([
            '@capabilities(fs:read)' if priv else '',
            'fn inner_fn(path: str) -> fn() -> str {',
            f'    {body}',
            '}',
            'fn ident<T>(x: T) -> T { return x }',
            'fn make_fn(path: str) -> fn() -> str {',
            '    return ident(inner_fn(path))',
            '}',
        ], 'make_fn("secret.txt")')
    raise ValueError(kind)


def container(kind):
    """Where the fn-value is stored. (decls, store_stmts, read_expr)."""
    if kind == 'none':
        return ([], ['let held = FNEXPR'], 'held')
    if kind == 'struct':
        return (['struct Holder { f: fn() -> str }'],
                ['let holder = Holder { f: FNEXPR }'], 'holder.f')
    if kind == 'array':
        return ([], ['let arr: [fn() -> str] = [FNEXPR]'], 'arr[0]')
    if kind == 'map':
        return ([], ['let m: map<str, fn() -> str> = {"k": FNEXPR}'], 'm["k"]')
    if kind == 'tuple':
        return ([], ['let pair: (i64, fn() -> str) = (0, FNEXPR)'], 'pair.1')
    if kind == 'nested':
        return (['struct Inner { f: fn() -> str }',
                 'struct Outer { inner: Inner }'],
                ['let outer = Outer { inner: Inner { f: FNEXPR } }'],
                'outer.inner.f')
    raise ValueError(kind)


def transport(kind, read_expr):
    """How it crosses a boundary before invocation. (decls, stmts, call_expr)."""
    if kind == 'local':
        return ([], [], f'{read_expr}()')
    if kind == 'param':
        return (['fn via_param(g: fn() -> str) -> str {',
                 '    return g()',
                 '}'], [], f'via_param({read_expr})')
    if kind == 'method':
        return (['struct Runner { tag: i64 }',
                 'impl Runner {',
                 '    fn run(self, g: fn() -> str) -> str {',
                 '        return g()',
                 '    }',
                 '}'], ['let runner = Runner { tag: 0 }'],
                f'runner.run({read_expr})')
    if kind == 'actor':
        # NOTE: an actor handler returns void, so this transport yields a
        # STATEMENT, not a value. Wrapping it in `len(..)` produced a spurious
        # E0110 in the first draft of this generator -- a generator bug that
        # would have been miscounted as a compiler defect.
        return (['actor Worker {',
                 '    fn handle(self, g: fn() -> str) {',
                 '        let v = g()',
                 '        println(v)',
                 '    }',
                 '}'], ['let w = Worker()'], f'STMT:w.handle({read_expr})')
    if kind == 'accessor':
        return (['struct Box2 { v: fn() -> str }',
                 'impl Box2 {',
                 '    fn get(self) -> fn() -> str {',
                 '        return self.v',
                 '    }',
                 '}'], ['let b2 = Box2 { v: READ_RAW }'], 'b2.get()()')
    raise ValueError(kind)


def build(src_kind, con_kind, tr_kind, priv):
    s_decls, fn_expr = source(src_kind, priv)
    c_decls, c_stmts, read_expr = container(con_kind)
    t_decls, t_stmts, call_expr = transport(tr_kind, read_expr)

    # the accessor transport supplies its own container read
    if tr_kind == 'accessor':
        t_stmts = [x.replace('READ_RAW', read_expr) for x in t_stmts]

    lines = []
    lines += [l for l in s_decls if l != '']
    lines += c_decls
    lines += t_decls
    lines.append('')
    if priv:
        lines.append('@capabilities(fs:read)')
    lines.append('fn main() {')
    for st in c_stmts:
        lines.append('    ' + st.replace('FNEXPR', fn_expr))
    for st in t_stmts:
        lines.append('    ' + st)
    is_stmt = call_expr.startswith('STMT:')
    expr = call_expr[5:] if is_stmt else call_expr
    if priv:
        lines.append('    deny!(fs:read) {')
        if is_stmt:
            lines.append('        ' + expr)
        else:
            lines.append('        let out = ' + expr)
            lines.append('        println("LEAK: " + to_string(len(out)))')
        lines.append('    }')
    else:
        if is_stmt:
            lines.append('    ' + expr)
        else:
            lines.append('    let out = ' + expr)
            lines.append('    println("ok " + to_string(len(out)))')
    lines.append('}')
    return '\n'.join(lines) + '\n'


SOURCES = ['direct', 'wrapper', 'generic']
CONTAINERS = ['none', 'struct', 'array', 'map', 'tuple', 'nested']
TRANSPORTS = ['local', 'param', 'method', 'actor', 'accessor']


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('outdir')
    ap.add_argument('--sources', default=','.join(SOURCES))
    ap.add_argument('--containers', default=','.join(CONTAINERS))
    ap.add_argument('--transports', default=','.join(TRANSPORTS))
    a = ap.parse_args()
    os.makedirs(a.outdir, exist_ok=True)

    n = 0
    for s, c, t in itertools.product(a.sources.split(','),
                                     a.containers.split(','),
                                     a.transports.split(',')):
        # `accessor` supplies its own container read, so pairing it with a
        # non-trivial container would double-wrap; keep it to the plain case.
        if t == 'accessor' and c != 'none':
            continue
        base = f'{s}__{c}__{t}'
        with open(os.path.join(a.outdir, base + '__attack.kry'), 'w') as fh:
            fh.write(build(s, c, t, True))
        with open(os.path.join(a.outdir, base + '__control.kry'), 'w') as fh:
            fh.write(build(s, c, t, False))
        n += 1
    print(n)


if __name__ == '__main__':
    sys.exit(main())
