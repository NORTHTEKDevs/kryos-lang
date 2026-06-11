// cc -O2 fannkuch.c -o fannkuch_c
// Canonical fannkuch-redux (Benchmarks Game shape), single-threaded.
#include <stdio.h>
#include <stdint.h>

int main(void) {
    const int n = 10;
    int64_t perm1[16], count[16], perm[16];
    for (int i = 0; i < n; i++) perm1[i] = i;
    int64_t max_flips = 0, checksum = 0, perm_count = 0;
    int r = n;
    for (;;) {
        while (r != 1) { count[r - 1] = r; r--; }
        for (int i = 0; i < n; i++) perm[i] = perm1[i];
        int64_t flips = 0;
        int64_t k = perm[0];
        while (k != 0) {
            int a = 0, b = (int)k;
            while (a < b) { int64_t t = perm[a]; perm[a] = perm[b]; perm[b] = t; a++; b--; }
            flips++;
            k = perm[0];
        }
        if (flips > max_flips) max_flips = flips;
        checksum += (perm_count % 2 == 0) ? flips : -flips;
        for (;;) {
            if (r == n) {
                printf("%lld\n", (long long)checksum);
                printf("Pfannkuchen(%d) = %lld\n", n, (long long)max_flips);
                return 0;
            }
            int64_t perm0 = perm1[0];
            for (int i = 0; i < r; i++) perm1[i] = perm1[i + 1];
            perm1[r] = perm0;
            if (--count[r] > 0) break;
            r++;
        }
        perm_count++;
    }
}
