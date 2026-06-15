#include <cstdio>
#include <cstdint>
struct Tree { Tree *left, *right; };
static Tree *make(int64_t depth) {
    Tree *t = new Tree;
    if (depth == 0) { t->left = t->right = nullptr; return t; }
    t->left = make(depth - 1);
    t->right = make(depth - 1);
    return t;
}
static int64_t check(Tree *t) {
    if (!t->left) return 1;
    return 1 + check(t->left) + check(t->right);
}
static void freet(Tree *t) { if (t->left) { freet(t->left); freet(t->right); } delete t; }
int main() {
    int64_t max_depth = 16;
    Tree *long_lived = make(max_depth);
    int64_t total = 0;
    for (int64_t depth = 4; depth <= max_depth; depth += 2) {
        int64_t iterations = 1LL << (max_depth - depth + 4);
        int64_t sum = 0;
        for (int64_t i = 0; i < iterations; i++) { Tree *t = make(depth); sum += check(t); freet(t); }
        total += sum;
    }
    total += check(long_lived);
    freet(long_lived);
    std::printf("binary_trees(canonical, depth=16) checksum = %lld\n", (long long)total);
    return 0;
}
