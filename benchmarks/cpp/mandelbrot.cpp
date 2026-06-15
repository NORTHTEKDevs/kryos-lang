#include <cstdio>
#include <cstdint>
static int64_t mandel_iter(double cr, double ci, int64_t max_iter) {
    double zr = 0.0, zi = 0.0;
    for (int64_t i = 0; i < max_iter; i++) {
        double zr2 = zr * zr, zi2 = zi * zi;
        if (zr2 + zi2 > 4.0) return i;
        double nzr = zr2 - zi2 + cr;
        zi = 2.0 * zr * zi + ci;
        zr = nzr;
    }
    return max_iter;
}
int main() {
    int64_t width = 1000, height = 1000, max_iter = 1000, checksum = 0;
    for (int64_t y = 0; y < height; y++)
        for (int64_t x = 0; x < width; x++) {
            double cr = -2.0 + 3.0 * (double)x / (double)width;
            double ci = -1.5 + 3.0 * (double)y / (double)height;
            checksum += mandel_iter(cr, ci, max_iter);
        }
    std::printf("mandelbrot checksum = %lld\n", (long long)checksum);
    return 0;
}
