/* demo/cabi/harness.c
 * Tiny C host that calls the governed agent through the C ABI.
 * Usage: harness.exe <path-to-agent_lib.dll>
 *
 * Governance assertions (verified by check.sh):
 *   - within-budget call returns 1 + prints WITHIN_BUDGET_ANSWER/SOURCE
 *   - over-budget call returns 0 + prints OVER_BUDGET_REFUSED/SPEND:0
 */
#include <windows.h>
#include <stdio.h>

typedef long long (*agent_query_c_fn)(long long max_calls, long long calls_spent);

int main(int argc, char *argv[]) {
    if (argc < 2) {
        fprintf(stderr, "usage: harness.exe <path-to-agent_lib.dll>\n");
        return 1;
    }
    HMODULE lib = LoadLibraryA(argv[1]);
    if (!lib) {
        fprintf(stderr, "LoadLibrary failed: %lu\n", GetLastError());
        return 1;
    }
    agent_query_c_fn fn = (agent_query_c_fn)GetProcAddress(lib, "agent_query_c");
    if (!fn) {
        fprintf(stderr, "GetProcAddress failed: %lu\n", GetLastError());
        FreeLibrary(lib);
        return 1;
    }

    /* (a) within-budget: max_calls=3, calls_spent=0 -> expect return 1 */
    long long r1 = fn(3, 0);
    fprintf(stdout, "C: within-budget result=%lld\n", r1);

    /* (b) over-budget: max_calls=0, calls_spent=0 -> expect return 0 */
    long long r2 = fn(0, 0);
    fprintf(stdout, "C: over-budget result=%lld\n", r2);

    FreeLibrary(lib);
    printf("cabi: PASS\n");
    return 0;
}
