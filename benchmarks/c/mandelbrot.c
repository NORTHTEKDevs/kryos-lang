// cc -O2 mandelbrot.c -o mandelbrot_c
#include <stdio.h>
#include <stdint.h>

static int64_t mandel_iter(double cr, double ci, int64_t max_iter) {
    double zr = 0.0, zi = 0.0;
    for (int64_t i = 0; i < max_iter; i++) {
        double zr2 = zr * zr;
        double zi2 = zi * zi;
        if (zr2 + zi2 > 4.0) return i;
        double new_zr = zr2 - zi2 + cr;
        double new_zi = 2.0 * zr * zi + ci;
        zr = new_zr;
        zi = new_zi;
    }
    return max_iter;
}

int main(void) {
    int64_t width = 1000, height = 1000, max_iter = 1000;
    int64_t checksum = 0;
    for (int64_t y = 0; y < height; y++) {
        for (int64_t x = 0; x < width; x++) {
            double cr = -2.0 + 3.0 * (double)x / (double)width;
            double ci = -1.5 + 3.0 * (double)y / (double)height;
            checksum += mandel_iter(cr, ci, max_iter);
        }
    }
    printf("mandelbrot checksum = %lld\n", (long long)checksum);
    return 0;
}
