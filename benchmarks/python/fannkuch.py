# Benchmark: Fannkuch-redux — mirrors c/fannkuch.c exactly.
def count_flips(perm):
    flips = 0
    p0 = perm[0]
    while p0 != 0:
        a, b = 0, int(p0)
        while a < b:
            perm[a], perm[b] = perm[b], perm[a]
            a += 1
            b -= 1
        flips += 1
        p0 = perm[0]
    return flips

def rotate_left(perm, n):
    first = perm[0]
    for i in range(n - 1):
        perm[i] = perm[i + 1]
    perm[n - 1] = first

n = 10
perm = list(range(n))
max_flips = 0
total = 362880  # 9!
for _ in range(total):
    work = perm[:]
    f = count_flips(work)
    if f > max_flips:
        max_flips = f
    rotate_left(perm, n)

print(f"fannkuch(10) = {max_flips}")
