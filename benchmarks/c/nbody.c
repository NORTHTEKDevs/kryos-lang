// cc -O2 nbody.c -o nbody_c -lm
#include <stdio.h>
#include <stdint.h>
#include <math.h>

int main(void) {
    const int n = 5;
    const int64_t steps = 2000000;
    const double dt = 0.01;

    double x[5]  = {0.0, 1.0, 2.0, 3.0, 4.0};
    double y[5]  = {0.0, 0.5, 1.0, 1.5, 2.0};
    double z[5]  = {0,0,0,0,0};
    double vx[5] = {0,0,0,0,0};
    double vy[5] = {0,0,0,0,0};
    double vz[5] = {0,0,0,0,0};
    double mass[5] = {1.0, 0.5, 0.5, 0.3, 0.2};

    for (int64_t s = 0; s < steps; s++) {
        for (int i = 0; i < n; i++) {
            double fx = 0, fy = 0, fz = 0;
            for (int j = 0; j < n; j++) {
                if (i != j) {
                    double dx = x[j] - x[i];
                    double dy = y[j] - y[i];
                    double dz = z[j] - z[i];
                    double r2 = dx*dx + dy*dy + dz*dz + 0.001;
                    double r  = sqrt(r2);
                    double inv_r3 = mass[j] / (r2 * r);
                    fx += dx * inv_r3;
                    fy += dy * inv_r3;
                    fz += dz * inv_r3;
                }
            }
            vx[i] += fx * dt;
            vy[i] += fy * dt;
            vz[i] += fz * dt;
        }
        for (int k = 0; k < n; k++) {
            x[k] += vx[k] * dt;
            y[k] += vy[k] * dt;
            z[k] += vz[k] * dt;
        }
    }

    double checksum = 0;
    for (int k = 0; k < n; k++) checksum += x[k] + y[k] + z[k];
    printf("nbody checksum (rounded) = %lld\n", (long long)checksum);
    return 0;
}
