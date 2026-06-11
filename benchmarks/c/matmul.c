// cc -O2 matmul.c -o matmul_c
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

int main(void) {
    const int n = 512;
    int64_t *a = malloc(sizeof(int64_t) * n * n);
    int64_t *b = malloc(sizeof(int64_t) * n * n);
    int64_t *c = malloc(sizeof(int64_t) * n * n);
    for (int i = 0; i < n * n; i++) {
        a[i] = ((int64_t)i * 3) % 7;
        b[i] = ((int64_t)i * 5) % 11;
        c[i] = 0;
    }
    for (int row = 0; row < n; row++) {
        for (int col = 0; col < n; col++) {
            int64_t sum = 0;
            for (int k = 0; k < n; k++) {
                sum += a[row * n + k] * b[k * n + col];
            }
            c[row * n + col] = sum;
        }
    }
    int64_t checksum = 0;
    for (int i = 0; i < n * n; i++) checksum += c[i];
    printf("matmul(512) checksum = %lld\n", (long long)checksum);
    free(a); free(b); free(c);
    return 0;
}
