#include <stdio.h>
typedef long i64;
static i64 mandel(i64 cr, i64 ci, i64 max_iter, i64 scale) {
    i64 zr = 0, zi = 0, i = 0;
    i64 four = 4 * scale * scale;
    while (i < max_iter) {
        i64 zr2 = (zr * zr) / scale;
        i64 zi2 = (zi * zi) / scale;
        if (zr2 + zi2 > four) return i;
        i64 new_zr = zr2 - zi2 + cr;
        i64 new_zi = (2 * zr * zi) / scale + ci;
        zr = new_zr;
        zi = new_zi;
        i++;
    }
    return max_iter;
}
int main() {
    i64 scale = 1000, w = 800, h = 800, max_iter = 1000;
    i64 xmin = -2 * scale, xmax = 1 * scale, ymin = -1 * scale, ymax = 1 * scale;
    i64 total = 0;
    for (i64 y = 0; y < h; y++) {
        i64 ci = ymin + ((ymax - ymin) * y) / h;
        for (i64 x = 0; x < w; x++) {
            i64 cr = xmin + ((xmax - xmin) * x) / w;
            total += mandel(cr, ci, max_iter, scale);
        }
    }
    printf("total iterations = %ld\n", total);
    return 0;
}
