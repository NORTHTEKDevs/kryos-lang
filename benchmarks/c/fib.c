// Equivalent C impl. Build: cc -O2 fib.c -o fib_c
#include <stdio.h>
#include <stdint.h>

int64_t fib(int64_t n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

int main(void) {
    int64_t n = 35;
    int64_t r = fib(n);
    printf("fib(35) = %lld\n", (long long)r);
    return 0;
}
