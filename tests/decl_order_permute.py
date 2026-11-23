#!/usr/bin/env python3
"""Declaration-order permutation check (LEDGER items 124 and 125, root F).

usage: decl_order_permute.py <kryos> <workdir> <orders> <file.kry>...

A program's verdict must not depend on the textual order of its top-level
declarations. For each file: split it into top-level items (a leading comment or
annotation belongs to the item below it; `use` / `import` lines stay on top),
check the original order, the reversed order and <orders> seeded random orders in
default and --strict-capabilities mode, and require the same (exit code, set of
error codes) for all of them. Exit 1 on any divergence.
"""
import hashlib, os, random, re, subprocess, sys

K, WORK, NORDERS, FILES = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4:]
os.makedirs(WORK, exist_ok=True)


def code_part(line):
    line = re.sub(r'"(\\.|[^"\\])*"', '""', line)
    i = line.find('//')
    return line if i < 0 else line[:i]


def split(src):
    head, items, cur, depth = [], [], [], 0
    for ln in src.split('\n'):
        s = code_part(ln).strip()
        if not cur and depth == 0 and re.match(r'(use|import)\s', s):
            head.append(ln)
            continue
        if not cur and s == '':
            continue
        cur.append(ln)
        depth += s.count('{') - s.count('}')
        pending = s == '' or s.startswith('@') and '{' not in s and s.endswith(')')
        if depth == 0 and s != '' and not pending:
            items.append('\n'.join(cur))
            cur = []
    if any(c.strip() for c in cur):
        items.append('\n'.join(cur))
    return head, items


def impl_parts(item):
    """(header, methods, footer) for an `impl` item with 2+ methods, else None."""
    lines = item.split('\n')
    h = next((i for i, ln in enumerate(lines) if code_part(ln).strip().startswith('impl')), None)
    if h is None or not code_part(lines[h]).rstrip().endswith('{') or code_part(lines[-1]).strip() != '}':
        return None
    _, methods = split('\n'.join(lines[h + 1:-1]))
    if len(methods) < 2:
        return None
    return '\n'.join(lines[:h + 1]), methods, lines[-1]


def render(item, rng, reverse):
    parts = impl_parts(item)
    if parts is None:
        return item
    header, methods, footer = parts
    ms = list(methods)
    if reverse:
        ms.reverse()
    elif rng is not None:
        rng.shuffle(ms)
    return header + '\n' + '\n'.join(ms) + '\n' + footer


def verdict(path, strict):
    cmd = [K, 'check'] + (['--strict-capabilities'] if strict else []) + [path]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=120, encoding='utf-8', errors='replace')
    return r.returncode, tuple(sorted(set(re.findall(r'\bE\d{4}\b', r.stdout + r.stderr))))


bad = 0
for f in FILES:
    head, items = split(open(f, encoding='utf-8').read())
    tag = hashlib.md5(f.encode()).hexdigest()[:8]
    rng = random.Random(tag)
    orders = [list(range(len(items))), list(reversed(range(len(items))))]
    seen = {tuple(o) for o in orders}
    for _ in range(400):
        if len(orders) >= NORDERS + 2:
            break
        o = list(range(len(items)))
        rng.shuffle(o)
        if tuple(o) not in seen:
            seen.add(tuple(o))
            orders.append(o)
    results = {}
    for k, o in enumerate(orders):
        p = os.path.join(WORK, f'{tag}_{k}.kry')
        mrng = random.Random(f'{tag}-{k}') if k > 1 else None
        with open(p, 'w', encoding='utf-8', newline='\n') as fh:
            fh.write('\n'.join(head) + '\n' + '\n\n'.join(render(items[i], mrng, k == 1) for i in o) + '\n')
        results.setdefault((verdict(p, False), verdict(p, True)), []).append(o)
    if len(results) == 1:
        (v,) = results
        print(f'  ok   {f}: {len(items)} items, {len(orders)} orders, one verdict {v[0]}')
    else:
        bad = 1
        print(f'  FAIL {f}: {len(results)} different verdicts over {len(orders)} orders')
        for v, os_ in results.items():
            print(f'       {v} first at order {os_[0]} ({len(os_)} orders)')
sys.exit(bad)
