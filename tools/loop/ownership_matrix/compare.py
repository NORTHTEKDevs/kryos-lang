# compare.py <master_out> <cand_out>: regressions of the candidate against master,
# per probe and backend (jit = jd/jr, llvm = ld/lr, cl = cd/cr). A backend is
# CLEAN for a run when its diag leg has no double-free report and rc 0 and its
# real-free leg prints the same lines as the diag leg. Reported:
#   REGRESSION  master clean on that backend, candidate not, or candidate
#               values differ from master's
#   CRASH       master not clean but ran (rc 0), candidate crashes / fails
# Differences where both are unclean are UAF noise (allocation-dependent), so
# they are counted, not listed.
import os, re, sys
P = re.compile(r'DOUBLE|ALREADY-FREED|NON-BOX|double free|bogus|AFTER-FREE')
BACKENDS = {'jit': ('jd', 'jr', 'jde'), 'llvm': ('ld', 'lr', 'lde'), 'cl': ('cd', 'cr', 'cde')}


def read(d, b, ext):
    try:
        return open(os.path.join(d, f'{b}.{ext}'), encoding='utf-8', errors='replace').read().replace('\r', '')
    except FileNotFoundError:
        return None


def leg(d, b, diag, real, err):
    dt, rt, et = read(d, b, diag), read(d, b, real), read(d, b, err) or ''
    if dt is None or rt is None:
        return None
    dl = [l for l in dt.split('\n') if l.strip()]
    rl = [l for l in rt.split('\n') if l.strip()]
    drc = next((l for l in reversed(dl) if l.startswith('rc=')), 'BUILDFAIL' if dl and dl[0] == 'BUILDFAIL' else 'rc=?')
    rrc = next((l for l in reversed(rl) if l.startswith('rc=')), 'BUILDFAIL' if rl and rl[0] == 'BUILDFAIL' else 'rc=?')
    dv = [l for l in dl if not l.startswith('rc=')]
    rv = [l for l in rl if not l.startswith('rc=')]
    df = len(P.findall(et))
    clean = drc == 'rc=0' and rrc == 'rc=0' and df == 0 and dv == rv
    return dict(clean=clean, drc=drc, rrc=rrc, df=df, dv=dv, rv=rv)


def expected_ok(lines):
    # Every generated line is `consumer=<value>`; the value-semantics answer is
    # v<iteration> (vg / v7 for a global's constant, v<i>/v<i+100> where the
    # source is printed too). True when every line is that answer.
    seen = {}
    for l in lines:
        k, _, v = l.partition('=')
        seen[k] = seen.get(k, 0) + 1
        i = seen[k]
        if v not in (f'v{i}', 'vg', 'v7', f'v{i}/v{i + 100}'):
            return False
    return True


base, cand = sys.argv[1], sys.argv[2]
probes = sorted(f[:-4] for f in os.listdir(cand) if f.endswith('.kry'))
reg = crash = noise = fixed = same = 0
for b in probes:
    notes = []
    for be, (dg, rl, er) in BACKENDS.items():
        m, c = leg(base, b, dg, rl, er), leg(cand, b, dg, rl, er)
        if m is None or c is None:
            continue
        if m['clean']:
            if not c['clean'] or (c['rv'] != m['rv'] and not expected_ok(c['rv'])):
                diff = [f'{x!r} vs master {y!r}' for x, y in zip(c['rv'], m['rv']) if x != y][:3]
                notes.append(f'REGRESSION {be}: cand rc={c["drc"]}/{c["rrc"]} df={c["df"]} {diff}')
            else:
                same += 1
        else:
            if c['clean']:
                fixed += 1
            elif m['drc'] == 'rc=0' and m['rrc'] == 'rc=0' and (c['drc'] != 'rc=0' or c['rrc'] != 'rc=0'):
                notes.append(f'CRASH {be}: cand rc={c["drc"]}/{c["rrc"]} (master ran, unclean: df={m["df"]})')
            else:
                noise += 1
    if notes:
        if any(n.startswith('REGRESSION') for n in notes):
            reg += 1
        else:
            crash += 1
        print(f'== {b}')
        for n in notes:
            print('   ' + n)
print(f'probes {len(probes)}: regressions {reg}, crash-only {crash}; backend legs: same-clean {same}, fixed {fixed}, both-unclean {noise}')
