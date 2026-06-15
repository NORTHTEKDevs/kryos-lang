#include <cstdio>
#include <cstdint>
static int64_t fib(int64_t n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
int main() {
    int64_t r = fib(40);
    std::printf("fib(40) = %lld\n", (long long)r);
    return 0;
}
