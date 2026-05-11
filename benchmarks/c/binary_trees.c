// cc -O2 binary_trees.c -o binary_trees_c
#include <stdio.h>
#include <stdint.h>

int64_t tree_sum(int64_t depth, int64_t value) {
    if (depth == 0) return value;
    return tree_sum(depth - 1, value * 2) + tree_sum(depth - 1, value * 2 + 1);
}

int main(void) {
    int64_t s = tree_sum(18, 1);
    printf("binary_trees(depth=18) = %lld\n", (long long)s);
    return 0;
}
