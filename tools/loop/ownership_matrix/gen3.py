# gen3.py <outdir>: the third matrix -- block values and function tails.
# One program per (type, source); one function per consumer. Consumers:
#   blockval   let y = { SRC }
#   blockfield let y = { let w = H { f: SRC, n: 1 }  w.f }
#   letmatch   let y = match i % 2 { 0 => SRC, _ => SRC }   (both arms read)
#   tailret    fn _in(..) -> T { ..  SRC }                 (implicit tail)
#   tailif     fn _in(..) -> T { ..  if i > 0 { SRC } else { SRC } }
#   matchret   fn _in(..) -> T { ..  return match i % 2 { 0 => SRC, _ => SRC } }
import os, sys
OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
OPT_SHOW = 'return match v {\n        Some(p) => p.name,\n        None => "none",\n    }'
OTUP_SHOW = 'return match v {\n        Some(t) => t.0,\n        None => "none",\n    }'
TYPES = {
    'tup': dict(t='(str, i64)', ctor='("v{i}", i)', show='return v.0'),
    'st': dict(t='P', ctor='P { name: "v{i}", n: i }', show='return v.name'),
    'map': dict(t='map<str, str>', ctor='mkm(i)', show='return v["k"]'),
    'fn': dict(t='fn(i64) -> str', ctor='mkf(i)', show='return v(0)'),
    'opt': dict(t='Option<P>', ctor='Some(P { name: "v{i}", n: i })', show=OPT_SHOW),
    'str': dict(t='str', ctor='"v{i}"', show='return v'),
    'arr': dict(t='[str]', ctor='["v{i}"]', show='return v[0]'),
    'tst': dict(t='PT', ctor='PT { t: ("v{i}", i), n: i }', show='return v.t.0'),
    'parr': dict(t='[P]', ctor='[P { name: "v{i}", n: i }]', show='return v[0].name'),
    'otup': dict(t='Option<(str, i64)>', ctor='Some(("v{i}", i))', show=OTUP_SHOW),
    'nest': dict(t='Q', ctor='Q { p: P { name: "v{i}", n: i }, xs: ["x"] }', show='return v.p.name'),
    'rtree': dict(t='Tree', ctor='Tree.Node(Tree.Leaf("v{i}"), Tree.Leaf("c"))',
                  show='return match v {\n        Tree.Leaf(s) => s,\n        Tree.Node(a, b) => match a {\n            Tree.Leaf(s) => s,\n            Tree.Node(x, y) => "deep",\n        },\n    }'),
}
# source kind -> (prelude lines, expr, kill lines)
SOURCES = {
    'local': (['let mut x = @C'], 'x', ['x = @K']),
    'param': None,
    'cap': None,
    'fld': (['let mut h = H { f: @C, n: 1 }'], 'h.f', ['h.f = @K', 'h = H { f: @K, n: 2 }']),
    'fldtmp': ([], 'mkh(i).f', []),
    'fldconst': ([], 'GH.f', []),
    'idx': (['let mut xs = [@C]'], 'xs[0]', ['xs[0] = @K']),
    'idxtmp': ([], 'mkxs(i)[0]', []),
    'fldidx': (['let mut hs = [H { f: @C, n: 1 }]'], 'hs[0].f', ['hs[0] = H { f: @K, n: 2 }']),
    'mapval': (['let mut m: map<str, @T> = {}', 'm["k"] = @C'], 'm["k"]', ['m["k"] = @K']),
    'gmut': (['GM.f = @C'], 'GM.f', ['GM.f = @K']),
    'nestfld': (['let mut hh = HH { h: H { f: @C, n: 1 }, n: 2 }'], 'hh.h.f', ['hh.h.f = @K', 'hh = HH { h: H { f: @K, n: 3 }, n: 4 }']),
}
LOCAL_CONSUMERS = {
    'blockval': lambda e: ['let y = {', f'    {e}', '}'],
    'blockfield': lambda e: ['let y = {', f'    let w = H {{ f: {e}, n: 1 }}', '    w.f', '}'],
    'letmatch': lambda e: ['let y = match i % 2 {', f'    0 => {e},', f'    _ => {e},', '}'],
}
TAIL_CONSUMERS = {
    'tailret': lambda e: [e],
    'tailif': lambda e: [f'if i > 0 {{ {e} }} else {{ {e} }}'],
    'matchret': lambda e: ['return match i % 2 {', f'    0 => {e},', f'    _ => {e},', '}'],
}
CONSUMERS = list(LOCAL_CONSUMERS) + list(TAIL_CONSUMERS)


def sub(lines, m):
    out = []
    for l in lines:
        for k, v in m.items():
            l = l.replace(k, v)
        out.append(l)
    return out


for tk, td in TYPES.items():
    T = td['t']
    C = td['ctor']
    K = C.replace('{i}', '{i + 100}').replace('(i)', '(i + 100)').replace(', i)', ', i + 100)').replace('n: i }', 'n: i + 100 }')
    G = C.replace('{i}', 'g').replace('(i)', '(7)').replace(', i)', ', 7)').replace('n: i }', 'n: 7 }')
    if C == 'mkm(i)':
        G = '{"k": "v7"}'
    M = {'@C': C, '@K': K, '@T': T}
    for sk, sv in SOURCES.items():
        if tk in ('fn', 'rtree') and sk in ('fldconst', 'gmut'):
            continue
        L = []
        if tk in ('opt', 'otup'):
            L.append('use std::option::{Option, Some, None}')
        L += ['enum Tree { Leaf(str), Node(Tree, Tree) }', 'struct P { name: str, n: i64 }', 'struct PT { t: (str, i64), n: i64 }', 'struct Q { p: P, xs: [str] }',
              f'struct H {{ f: {T}, n: i64 }}', 'struct HH { h: H, n: i64 }']
        if sk == 'fldconst':
            L.append(f'let GH: H = H {{ f: {G}, n: 1 }}')
        if sk == 'gmut':
            L.append(f'let mut GM: H = H {{ f: {G}, n: 1 }}')
        L += ['fn churn(i: i64) -> i64 {', '    let mut junk: [str] = []', '    let mut j = 0', '    while j < 40 {',
              '        junk = push(junk, "junk{i}-{j}")', '        j = j + 1', '    }', '    return len(junk)', '}']
        L += ['fn mkm(i: i64) -> map<str, str> {', '    let mut m: map<str, str> = {}', '    m["k"] = "v{i}"', '    return m', '}']
        L += ['fn mkf(i: i64) -> fn(i64) -> str {', '    let s = "v{i}"', '    return |x: i64| s', '}']
        L += [f'fn mkh(i: i64) -> H {{ return H {{ f: {C}, n: i }} }}', f'fn mkxs(i: i64) -> [{T}] {{ return [{C}] }}']
        L += [f'fn show(v: {T}) -> str {{', '    ' + td['show'], '}']
        calls = []
        for c in CONSUMERS:
            fn = f'c_{c}'
            if c in LOCAL_CONSUMERS:
                if sk == 'param':
                    L += [f'fn {fn}_in(i: i64, x: {T}) -> {T} {{'] + ['    ' + l for l in sub(LOCAL_CONSUMERS[c]('x'), M)] + ['    return y', '}']
                    L += [f'fn {fn}(i: i64) -> str {{', f'    let mut a = {C}', f'    let y = {fn}_in(i, a)', f'    a = {K}', '    churn(i)',
                          '    return show(y)', '}']
                elif sk == 'cap':
                    L += [f'fn {fn}(i: i64) -> str {{', f'    let mut x = {C}', f'    let g = || -> {T} {{'] + \
                         ['        ' + l for l in sub(LOCAL_CONSUMERS[c]('x'), M)] + ['        return y', '    }', '    let y = g()', f'    x = {K}',
                                                                                      '    churn(i)', '    return show(y)', '}']
                else:
                    pre, e, kill = sv
                    L += [f'fn {fn}(i: i64) -> str {{'] + ['    ' + l for l in sub(pre, M)] + ['    ' + l for l in sub(LOCAL_CONSUMERS[c](e), M)] + \
                         ['    ' + l for l in sub(kill, M)] + ['    churn(i)', '    return show(y)', '}']
            else:
                if sk == 'param':
                    L += [f'fn {fn}_in(i: i64, x: {T}) -> {T} {{'] + ['    ' + l for l in TAIL_CONSUMERS[c]('x')] + ['}']
                    L += [f'fn {fn}(i: i64) -> str {{', f'    let mut a = {C}', f'    let y = {fn}_in(i, a)', f'    a = {K}', '    churn(i)',
                          '    return show(y) + "/" + show(a)', '}']
                elif sk == 'cap':
                    L += [f'fn {fn}(i: i64) -> str {{', f'    let mut x = {C}', f'    let g = || -> {T} {{'] + \
                         ['        ' + l for l in TAIL_CONSUMERS[c]('x')] + ['    }', '    let y = g()', f'    x = {K}', '    churn(i)',
                                                                         '    return show(y)', '}']
                else:
                    pre, e, kill = sv
                    L += [f'fn {fn}_in(i: i64) -> {T} {{'] + ['    ' + l for l in sub(pre, M)] + ['    ' + l for l in TAIL_CONSUMERS[c](e)] + ['}']
                    L += [f'fn {fn}(i: i64) -> str {{', f'    let y = {fn}_in(i)', '    churn(i)', '    return show(y)', '}']
            calls.append(fn)
        L += ['@capabilities(all)', 'fn main() {', '    let mut i = 1', '    while i <= 3 {']
        for fn in calls:
            L.append(f'        println("{fn[2:]}=" + {fn}(i))')
        L += ['        i = i + 1', '    }', '}']
        with open(os.path.join(OUT, f'{tk}_{sk}.kry'), 'w', newline='\n') as fh:
            fh.write('\n'.join(L) + '\n')
print(len(os.listdir(OUT)))
