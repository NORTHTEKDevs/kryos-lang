# Benchmark: N-body simulation — mirrors c/nbody.c exactly.
import math

n = 5
steps = 50000
dt = 0.01

x  = [0.0, 1.0, 2.0, 3.0, 4.0]
y  = [0.0, 0.5, 1.0, 1.5, 2.0]
z  = [0.0] * n
vx = [0.0] * n
vy = [0.0] * n
vz = [0.0] * n
mass = [1.0, 0.5, 0.5, 0.3, 0.2]

for _ in range(steps):
    for i in range(n):
        fx = fy = fz = 0.0
        for j in range(n):
            if i != j:
                dx = x[j] - x[i]
                dy = y[j] - y[i]
                dz = z[j] - z[i]
                r2 = dx*dx + dy*dy + dz*dz + 0.001
                r = math.sqrt(r2)
                inv_r3 = mass[j] / (r2 * r)
                fx += dx * inv_r3
                fy += dy * inv_r3
                fz += dz * inv_r3
        vx[i] += fx * dt
        vy[i] += fy * dt
        vz[i] += fz * dt
    for k in range(n):
        x[k] += vx[k] * dt
        y[k] += vy[k] * dt
        z[k] += vz[k] * dt

checksum = sum(x[k] + y[k] + z[k] for k in range(n))
print(f"nbody checksum (rounded) = {int(checksum)}")
