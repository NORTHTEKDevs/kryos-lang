# Benchmark: integer matrix multiplication 256×256 — mirrors c/matmul.c exactly.
n = 256
a = [((i * 3) % 7) for i in range(n * n)]
b = [((i * 5) % 11) for i in range(n * n)]
c = [0] * (n * n)

for row in range(n):
    for col in range(n):
        s = 0
        for k in range(n):
            s += a[row * n + k] * b[k * n + col]
        c[row * n + col] = s

checksum = sum(c)
print(f"matmul(256) checksum = {checksum}")
