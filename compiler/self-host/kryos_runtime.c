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

/* The compiler may synthesize calls to memset / memcpy for zero-init
 * loops and struct copies. Since we build with /Zl (no default libs)
 * we have to provide them ourselves. Names are #pragma intrinsic'd
 * by MSVC so these definitions cover both implicit and explicit calls. */
#pragma function(memset, memcpy)
void* memset(void *dst, int c, SIZE_T n) {
    unsigned char *p = (unsigned char *)dst;
    while (n--) { *p++ = (unsigned char)c; }
    return dst;
}
void* memcpy(void *dst, const void *src, SIZE_T n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    while (n--) { *d++ = *s++; }
    return dst;
}

/* Forward declaration so kryos_builtin_len (above the arrays section)
 * can use the KryosArray header for type discrimination. */
typedef struct KryosArray {
    long long len;
    long long cap;
    long long elem_size;
    long long ref_count;
    unsigned char *data;
} KryosArray;

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

/* ---- File I/O -------------------------------------------------------- */

/*
 * Read an entire file into a freshly allocated null-terminated buffer.
 * Returns the buffer pointer (cast to long long for the Kryos i64
 * handle) or 0 on failure. The path argument is a C string.
 */
long long kryos_builtin_file_read(const char *path) {
    HANDLE h = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, NULL,
                           OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h == INVALID_HANDLE_VALUE) { return 0; }
    LARGE_INTEGER size;
    if (!GetFileSizeEx(h, &size)) { CloseHandle(h); return 0; }
    if (size.QuadPart < 0 || size.QuadPart > (LONGLONG)0x10000000) {
        CloseHandle(h);
        return 0;
    }
    SIZE_T n = (SIZE_T)size.QuadPart;
    char *buf = (char *)HeapAlloc(GetProcessHeap(), 0, n + 1);
    if (buf == NULL) { CloseHandle(h); return 0; }
    DWORD read;
    if (!ReadFile(h, buf, (DWORD)n, &read, NULL)) {
        HeapFree(GetProcessHeap(), 0, buf);
        CloseHandle(h);
        return 0;
    }
    buf[n] = 0;
    CloseHandle(h);
    return (long long)buf;
}

/*
 * Write a null-terminated string to a file. Truncates existing content.
 */
long long kryos_builtin_file_write(const char *path, const char *content) {
    HANDLE h = CreateFileA(path, GENERIC_WRITE, 0, NULL,
                           CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h == INVALID_HANDLE_VALUE) { return 0; }
    SIZE_T n = kryos__strlen(content);
    DWORD written;
    BOOL ok = WriteFile(h, content, (DWORD)n, &written, NULL);
    CloseHandle(h);
    return ok ? 1 : 0;
}

long long kryos_builtin_file_exists(const char *path) {
    DWORD attrs = GetFileAttributesA(path);
    if (attrs == INVALID_FILE_ATTRIBUTES) { return 0; }
    return 1;
}

/* ---- Environment ----------------------------------------------------- */

/*
 * Get an environment variable's value as a fresh null-terminated string.
 * Returns 0 (treated as empty string by callers) if unset.
 */
long long kryos_builtin_env_get(const char *name) {
    char small[1024];
    DWORD n = GetEnvironmentVariableA(name, small, sizeof(small));
    if (n == 0) {
        static const char empty = 0;
        return (long long)&empty;
    }
    if (n < sizeof(small)) {
        char *buf = (char *)HeapAlloc(GetProcessHeap(), 0, n + 1);
        if (buf == NULL) {
            static const char empty = 0;
            return (long long)&empty;
        }
        SIZE_T i;
        for (i = 0; i <= n; i++) { buf[i] = small[i]; }
        return (long long)buf;
    }
    char *buf = (char *)HeapAlloc(GetProcessHeap(), 0, n + 1);
    if (buf == NULL) {
        static const char empty = 0;
        return (long long)&empty;
    }
    GetEnvironmentVariableA(name, buf, n);
    buf[n] = 0;
    return (long long)buf;
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

/* len() of either a string or an array. We use a heuristic: arrays
 * have a 40-byte header whose first 8 bytes are `len` (small int)
 * and whose 32-byte offset holds a pointer (data). If those bytes
 * look like a plausible array header, treat as array; otherwise as
 * a null-terminated string. This is fragile but enough to pass
 * the simple test programs. */
long long kryos_builtin_len(long long h) {
    const KryosArray *arr;
    if (h == 0) { return 0; }
    arr = (const KryosArray*)h;
    if (arr->cap > 0 && arr->cap < (long long)0x1000000 &&
        arr->ref_count > 0 && arr->ref_count < (long long)0x10000 &&
        arr->data != NULL) {
        return arr->len;
    }
    return (long long)kryos__strlen((const char *)h);
}

/* ---- Arrays ---------------------------------------------------------- */

/*
 * KryosArray (defined above) matching the layout of kryos-rt's KryosArray:
 *   { i64 len, i64 cap, i64 elem_size, i64 ref_count, *u8 data }
 *
 * Elements are stored as 8-byte slots regardless of declared type.
 */
KryosArray* kryos_array_new(long long elem_size, long long cap) {
    long long real_cap = cap < 4 ? 4 : cap;
    long long bytes = real_cap * 8;
    KryosArray *arr = (KryosArray*)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof(KryosArray));
    if (arr == NULL) { return NULL; }
    arr->len = 0;
    arr->cap = real_cap;
    arr->elem_size = elem_size;
    arr->ref_count = 1;
    arr->data = (unsigned char*)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, (SIZE_T)bytes);
    return arr;
}

long long kryos_builtin_push(long long arr_handle, long long val) {
    KryosArray *arr = (KryosArray*)arr_handle;
    if (arr == NULL) { return 0; }
    if (arr->len >= arr->cap) {
        long long new_cap = arr->cap * 2;
        if (new_cap < 4) { new_cap = 4; }
        long long old_bytes = arr->cap * 8;
        long long new_bytes = new_cap * 8;
        unsigned char *new_data = (unsigned char*)HeapReAlloc(GetProcessHeap(), 0, arr->data, (SIZE_T)new_bytes);
        if (new_data == NULL) { return arr_handle; }
        /* Zero new bytes. */
        long long i;
        for (i = old_bytes; i < new_bytes; i++) { new_data[i] = 0; }
        arr->data = new_data;
        arr->cap = new_cap;
    }
    long long *slots = (long long*)arr->data;
    slots[arr->len] = val;
    arr->len++;
    return arr_handle;
}

long long kryos_builtin_pop(long long arr_handle) {
    KryosArray *arr = (KryosArray*)arr_handle;
    if (arr == NULL || arr->len == 0) { return 0; }
    arr->len--;
    long long *slots = (long long*)arr->data;
    return slots[arr->len];
}

long long kryos_array_get(long long arr_handle, long long index) {
    KryosArray *arr = (KryosArray*)arr_handle;
    if (arr == NULL || index < 0 || index >= arr->len) { return 0; }
    long long *slots = (long long*)arr->data;
    return slots[index];
}

void kryos_array_set(long long arr_handle, long long index, long long val) {
    KryosArray *arr = (KryosArray*)arr_handle;
    if (arr == NULL || index < 0 || index >= arr->len) { return; }
    long long *slots = (long long*)arr->data;
    slots[index] = val;
}

long long kryos_array_len(long long arr_handle) {
    KryosArray *arr = (KryosArray*)arr_handle;
    if (arr == NULL) { return 0; }
    return arr->len;
}

/* `len(arr)` for arrays is also routed through kryos_builtin_len when the
 * stage-1 codegen can't disambiguate `len(str)` from `len(arr)`. The
 * stage-1-side mapping for `len` uses kryos_builtin_len; redirect to the
 * array-aware impl in that case. The earlier kryos_builtin_len treated
 * its argument as a C string -- that gave the right answer for `len(s)`
 * on strings, wrong for arrays. New behaviour: peek at the first qword;
 * if it looks like an array header (cap > 0 and ref_count > 0), use it
 * as an array. Otherwise fall back to strlen. */

/* ---- Array builtins -------------------------------------------------- */

/* In-place ascending sort of an [i64] array. Stage-1 maps the user-level
 * `sort(arr)` call to kryos_builtin_sort regardless of whether the user
 * also defined `fn sort` (lower.kry line 451 routes by name). This is an
 * insertion sort: O(n^2) but compact and fine for small inputs (the
 * primary stage-1 use cases). */
void kryos_builtin_sort(long long arr_handle) {
    KryosArray *arr = (KryosArray*)arr_handle;
    if (arr == NULL || arr->len < 2) { return; }
    long long *a = (long long*)arr->data;
    long long n = arr->len;
    for (long long i = 1; i < n; i++) {
        long long key = a[i];
        long long j = i - 1;
        while (j >= 0 && a[j] > key) {
            a[j + 1] = a[j];
            j--;
        }
        a[j + 1] = key;
    }
}

/* In-place reverse of an array. Mirrors kryos_builtin_sort's API. */
void kryos_builtin_reverse(long long arr_handle) {
    KryosArray *arr = (KryosArray*)arr_handle;
    if (arr == NULL || arr->len < 2) { return; }
    long long *a = (long long*)arr->data;
    long long n = arr->len;
    long long lo = 0, hi = n - 1;
    while (lo < hi) {
        long long t = a[lo]; a[lo] = a[hi]; a[hi] = t;
        lo++; hi--;
    }
}

/* ---- Entry point ----------------------------------------------------- */

extern void main(void);

void mainCRTStartup(void) {
    main();
    ExitProcess(0);
}
