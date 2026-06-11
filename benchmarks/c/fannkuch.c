// cc -O2 fannkuch.c -o fannkuch_c
#include <stdio.h>
#include <stdint.h>
#include <string.h>

static int64_t count_flips(int64_t *perm, int n) {
    int64_t flips = 0;
    int64_t p0 = perm[0];
    while (p0 != 0) {
        int a = 0;
        int b = (int)p0;
        while (a < b) {
            int64_t t = perm[a]; perm[a] = perm[b]; perm[b] = t;
            a++; b--;
        }
        flips++;
        p0 = perm[0];
    }
    return flips;
}

static void rotate_left(int64_t *perm, int n) {
    int64_t first = perm[0];
    for (int i = 0; i < n - 1; i++) perm[i] = perm[i + 1];
    perm[n - 1] = first;
}

int main(void) {
    const int n = 10;
    int64_t perm[9];
    for (int i = 0; i < n; i++) perm[i] = i;
    int64_t max_flips = 0;
    int64_t work[9];
    const int64_t total = 362880;
    for (int64_t iter = 0; iter < total; iter++) {
        memcpy(work, perm, sizeof(perm));
        int64_t f = count_flips(work, n);
        if (f > max_flips) max_flips = f;
        rotate_left(perm, n);
    }
    printf("fannkuch(10) = %lld\n", (long long)max_flips);
    return 0;
}
