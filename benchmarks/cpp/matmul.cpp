#include <cstdio>
#include <cstdint>
#include <vector>
int main() {
    const int n = 512;
    std::vector<int64_t> a(n*n), b(n*n), c(n*n, 0);
    for (int i = 0; i < n*n; i++) { a[i] = ((int64_t)i*3)%7; b[i] = ((int64_t)i*5)%11; }
    for (int row = 0; row < n; row++)
        for (int col = 0; col < n; col++) {
            int64_t sum = 0;
            for (int k = 0; k < n; k++) sum += a[row*n+k] * b[k*n+col];
            c[row*n+col] = sum;
        }
    int64_t checksum = 0; for (int i = 0; i < n*n; i++) checksum += c[i];
    std::printf("matmul(512) checksum = %lld\n", (long long)checksum);
    return 0;
}
