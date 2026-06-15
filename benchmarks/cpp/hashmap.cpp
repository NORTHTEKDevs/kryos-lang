#include <cstdio>
#include <cstdint>
#include <unordered_map>
int main() {
    const int64_t n = 1000000;
    // default-constructed, matching Rust HashMap::new() and Kryos #{} (no pre-size)
    std::unordered_map<int64_t, int64_t> m;
    for (int64_t i = 0; i < n; i++) m[i] = i * 2;
    int64_t sum = 0;
    for (int64_t j = 0; j < n; j++) sum += m[j];
    std::printf("%lld\n", (long long)sum);
    return 0;
}
