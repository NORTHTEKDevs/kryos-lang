/*
 * kryos_runtime.c -- minimal Windows runtime for Kryos self-hosted output.
 *
 * Exports the kryos_* symbols that the self-hosted compiler's codegen
 * emits CALL instructions against. Strings are passed as raw
 * null-terminated UTF-8 pointers (the rodata format that stage-1's
 * codegen produces for `RV_CONST_STRING`).
 *
 * Build:
 *   cl /nologo /c /O1 /Zl kryos_runtime.c
 *   lib /nologo /OUT:kryos_runtime.lib kryos_runtime.obj
 */

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

/* ---- Helpers --------------------------------------------------------- */

static HANDLE kryos__stdout(void) {
    static HANDLE h = NULL;
    if (h == NULL) { h = GetStdHandle(STD_OUTPUT_HANDLE); }
    return h;
}

static HANDLE kryos__stderr(void) {
    static HANDLE h = NULL;
    if (h == NULL) { h = GetStdHandle(STD_ERROR_HANDLE); }
    return h;
}

static SIZE_T kryos__strlen(const char *s) {
    const char *p = s;
    while (*p) { p++; }
    return (SIZE_T)(p - s);
}

static void kryos__write(HANDLE h, const char *s, SIZE_T n) {
    DWORD written;
    WriteFile(h, s, (DWORD)n, &written, NULL);
}

/* ---- I/O ------------------------------------------------------------- */

void kryos_print_str(const char *s) {
    kryos__write(kryos__stdout(), s, kryos__strlen(s));
}

void kryos_println_str(const char *s) {
    HANDLE h = kryos__stdout();
    kryos__write(h, s, kryos__strlen(s));
    kryos__write(h, "\r\n", 2);
}

void kryos_eprint_str(const char *s) {
    kryos__write(kryos__stderr(), s, kryos__strlen(s));
}

void kryos_eprintln_str(const char *s) {
    HANDLE h = kryos__stderr();
    kryos__write(h, s, kryos__strlen(s));
    kryos__write(h, "\r\n", 2);
}

/* ---- Process --------------------------------------------------------- */

void kryos_builtin_exit(int code) {
    ExitProcess((UINT)code);
}

/* ---- Numeric conversion --------------------------------------------- */

/*
 * to_string(i64) -> str. Returns a pointer to a static thread-local
 * buffer big enough to hold any 64-bit integer in decimal.
 *
 * The buffer is rotated through a small ring so consecutive calls in
 * a single println chain (e.g. println(to_string(a) + " " + to_string(b)))
 * do not stomp on each other before the result is used.
 */
/* Static buffers (process-wide; single-threaded use). TLS would
 * require linking the CRT, which we explicitly avoid via /Zl. */
static char kryos__num_bufs[8][32];
static int  kryos__num_slot = 0;

const char* kryos_i64_to_string(long long n) {
    int slot = kryos__num_slot;
    kryos__num_slot = (slot + 1) & 7;
    char *buf = kryos__num_bufs[slot];
    char *end = buf + 31;
    *end = 0;
    char *p = end;
    int neg = 0;
    unsigned long long u;
    if (n < 0) {
        neg = 1;
        u = (unsigned long long)(-(n + 1)) + 1ULL;
    } else {
        u = (unsigned long long)n;
    }
    if (u == 0) {
        *--p = '0';
    } else {
        while (u != 0) {
            *--p = (char)('0' + (u % 10));
            u /= 10;
        }
    }
    if (neg) { *--p = '-'; }
    return p;
}

/* ---- String basics --------------------------------------------------- */

/*
 * String concatenation -- allocates a fresh buffer on the C heap.
 * Caller never frees (we leak; programs are short-lived). Production
 * runtime would wire this into Kryos's ARC.
 */
const char* kryos_str_concat(const char *a, const char *b) {
    SIZE_T la = kryos__strlen(a);
    SIZE_T lb = kryos__strlen(b);
    char *out = (char *)HeapAlloc(GetProcessHeap(), 0, la + lb + 1);
    if (out == NULL) { return ""; }
    SIZE_T i = 0;
    while (i < la) { out[i] = a[i]; i++; }
    SIZE_T j = 0;
    while (j < lb) { out[i + j] = b[j]; j++; }
    out[la + lb] = 0;
    return out;
}

long long kryos_builtin_len_str(const char *s) {
    return (long long)kryos__strlen(s);
}

/* For Kryos programs that pass a length-prefixed string handle via
 * `len(s)` we currently treat it the same as a null-terminated C string. */
long long kryos_builtin_len(long long h) {
    return (long long)kryos__strlen((const char *)h);
}

/* ---- Entry point ----------------------------------------------------- */

extern void main(void);

void mainCRTStartup(void) {
    main();
    ExitProcess(0);
}
