# gen2.py <outdir>: the ownership matrix, part 2 -- more types (struct with a
# tuple field, [struct], Option<tuple>, nested struct), more sources (a
# mutable global's field, a nested field) and the consumers if / match value,
# index store, element field store, `return if`, nested literal. Same
# conventions as gen1.py.
import os, sys
OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
TYPES = {
    'tup': dict(t='(str, i64)', ctor='("v{i}", i)', show='return v.0'),
    'st': dict(t='P', ctor='P { name: "v{i}", n: i }', show='return v.name'),
    'map': dict(t='map<str, str>', ctor='mkm(i)', show='return v["k"]'),
    'fn': dict(t='fn(i64) -> str', ctor='mkf(i)', show='return v(0)'),
    'opt': dict(t='Option<P>', ctor='Some(P { name: "v{i}", n: i })',
                show='return match v {\n        Some(p) => p.name,\n        None => "none",\n    }'),
    'str': dict(t='str', ctor='"v{i}"', show='return v'),
    'arr': dict(t='[str]', ctor='["v{i}"]', show='return v[0]'),
    'tst': dict(t='PT', ctor='PT { t: ("v{i}", i), n: i }', show='return v.t.0'),
    'parr': dict(t='[P]', ctor='[P { name: "v{i}", n: i }]', show='return v[0].name'),
    'otup': dict(t='Option<(str, i64)>', ctor='Some(("v{i}", i))',
                 show='return match v {\n        Some(t) => t.0,\n        None => "none",\n    }'),
    'nest': dict(t='Q', ctor='Q { p: P { name: "v{i}", n: i }, xs: ["x"] }', show='return v.p.name'),
}
CONSUMERS = ['ifval', 'matchval', 'idxstore', 'elemfield', 'retif', 'nested']
# source kind -> (prelude lines, expr, kill lines); @C = ctor at i, @K = ctor at i + 100, @D = default
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
RT = {'let': '@T', 'reassign': '@T', 'structlit': 'H', 'arraylit': '[@T]', 'push': '[@T]',
      'mapstore': 'map<str, @T>', 'payload': 'W', 'tuplelit': '(@T, i64)', 'fieldstore': 'H',
      'ifval': '@T', 'matchval': '@T', 'idxstore': '[@T]', 'elemfield': '[H]', 'nested': 'HH'}


def consume(c, e):
    if c == 'let':
        return ([f'let y = {e}'], 'show(y)')
    if c == 'reassign':
        return (['let mut y = @D', f'y = {e}'], 'show(y)')
    if c == 'structlit':
        return ([f'let y = H {{ f: {e}, n: 1 }}'], 'show(y.f)')
    if c == 'arraylit':
        return ([f'let y = [{e}]'], 'show(y[0])')
    if c == 'push':
        return (['let mut y: [@T] = []', f'y = push(y, {e})'], 'show(y[0])')
    if c == 'mapstore':
        return (['let mut y: map<str, @T> = {}', f'y["a"] = {e}'], 'show(y["a"])')
    if c == 'payload':
        return ([f'let y = W.Has({e})'], 'showw(y)')
    if c == 'tuplelit':
        return ([f'let y = ({e}, 1)'], 'show(y.0)')
    if c == 'fieldstore':
        return (['let mut y = H { f: @D, n: 0 }', f'y.f = {e}'], 'show(y.f)')
    if c == 'ifval':
        return ([f'let y = if i > 0 {{ {e} }} else {{ @D }}'], 'show(y)')
    if c == 'matchval':
        return (['let y = match i {', '    0 => @D,', f'    _ => {e},', '}'], 'show(y)')
    if c == 'idxstore':
        return (['let mut y = [@D]', f'y[0] = {e}'], 'show(y[0])')
    if c == 'elemfield':
        return (['let mut y = [H { f: @D, n: 0 }]', f'y[0].f = {e}'], 'show(y[0].f)')
    if c == 'nested':
        return ([f'let y = HH {{ h: H {{ f: {e}, n: 1 }}, n: 2 }}'], 'show(y.h.f)')
    raise Exception(c)


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
    D = C.replace('{i}', 'd').replace('(i)', '(0)').replace(', i)', ', 0)').replace('n: i }', 'n: 0 }')
    G = C.replace('{i}', 'g').replace('(i)', '(7)').replace(', i)', ', 7)').replace('n: i }', 'n: 7 }')
    if C == 'mkm(i)':
        G = '{"k": "v7"}'
    M = {'@C': C, '@K': K, '@D': D, '@T': T}
    for sk, sv in SOURCES.items():
        if tk == 'fn' and sk in ('fldconst', 'gmut'):
            continue
        L = []
        if tk == 'opt':
            L.append('use std::option::{Option, Some, None}')
        if tk == 'otup' and 'use std::option' not in ''.join(L):
            L.append('use std::option::{Option, Some, None}')
        L += ['struct P { name: str, n: i64 }', 'struct PT { t: (str, i64), n: i64 }', 'struct Q { p: P, xs: [str] }',
              f'struct H {{ f: {T}, n: i64 }}', 'struct HH { h: H, n: i64 }', f'enum W {{ Has({T}), No }}']
        if sk == 'gmut':
            L.append(f'let mut GM: H = H {{ f: {G}, n: 1 }}')
        if sk == 'fldconst':
            L.append(f'let GH: H = H {{ f: {G}, n: 1 }}')
        L += ['fn churn(i: i64) -> i64 {', '    let mut junk: [str] = []', '    let mut j = 0', '    while j < 40 {',
              '        junk = push(junk, "junk{i}-{j}")', '        j = j + 1', '    }', '    return len(junk)', '}']
        L += ['fn mkm(i: i64) -> map<str, str> {', '    let mut m: map<str, str> = {}', '    m["k"] = "v{i}"', '    return m', '}']
        L += ['fn mkf(i: i64) -> fn(i64) -> str {', '    let s = "v{i}"', '    return |x: i64| s', '}']
        L += [f'fn mkh(i: i64) -> H {{ return H {{ f: {C}, n: i }} }}', f'fn mkxs(i: i64) -> [{T}] {{ return [{C}] }}']
        L += [f'fn show(v: {T}) -> str {{', '    ' + td['show'], '}']
        L += ['fn showw(w: W) -> str {', '    return match w {', '        W.Has(v) => show(v),', '        W.No => "no",', '    }', '}']
        calls = []
        for c in CONSUMERS:
            fn = f'c_{c}'
            if sk in ('param', 'cap'):
                if c == 'retif':
                    L += [f'fn {fn}_in(i: i64, x: {T}) -> {T} {{', '    return if len("a") > 0 {{ x }} else {{ x }}', '}']
                    L += [f'fn {fn}(i: i64) -> str {{', f'    let mut a = {C}', f'    let y = {fn}_in(i, a)', f'    a = {K}', '    churn(i)',
                          '    return show(y) + "/" + show(a)', '}']
                else:
                    b, r = consume(c, 'x')
                    rt = sub([RT[c]], M)[0]
                    if sk == 'param':
                        L += [f'fn {fn}_in(i: i64, x: {T}) -> {rt} {{'] + ['    ' + l for l in sub(b, M)] + ['    return y', '}']
                        L += [f'fn {fn}(i: i64) -> str {{', f'    let mut a = {C}', f'    let y = {fn}_in(i, a)', f'    a = {K}', '    churn(i)',
                              f'    return {r}', '}']
                    else:
                        L += [f'fn {fn}(i: i64) -> str {{', f'    let mut x = {C}', f'    let g = || -> {rt} {{'] + ['        ' + l for l in sub(b, M)] + \
                             ['        return y', '    }', '    let y = g()', f'    x = {K}', '    churn(i)', f'    return {r}', '}']
            else:
                pre, e, kill = sv
                if c == 'retif':
                    L += [f'fn {fn}_in(i: i64) -> {T} {{'] + ['    ' + l for l in sub(pre, M)] + [f'    return if i > 0 {{ {e} }} else {{ {e} }}', '}']
                    L += [f'fn {fn}(i: i64) -> str {{', f'    let y = {fn}_in(i)', '    churn(i)', '    return show(y)', '}']
                else:
                    b, r = consume(c, e)
                    L += [f'fn {fn}(i: i64) -> str {{'] + ['    ' + l for l in sub(pre, M)] + ['    ' + l for l in sub(b, M)] + \
                         ['    ' + l for l in sub(kill, M)] + ['    churn(i)', f'    return {r}', '}']
            calls.append(fn)
        L += ['@capabilities(all)', 'fn main() {', '    let mut i = 1', '    while i <= 3 {']
        for fn in calls:
            L.append(f'        println("{fn[2:]}=" + {fn}(i))')
        L += ['        i = i + 1', '    }', '}']
        with open(os.path.join(OUT, f'{tk}_{sk}.kry'), 'w', newline='\n') as fh:
            fh.write('\n'.join(L) + '\n')
print(len(os.listdir(OUT)))
