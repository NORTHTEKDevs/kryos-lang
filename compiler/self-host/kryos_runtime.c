/*
 * kryos_runtime.c -- minimal Windows runtime for Kryos self-hosted output.
 *
 * Exports the kryos_* symbols that the self-hosted compiler's codegen
 * emits CALL instructions against. Each function is a thin shim over
 * the Win32 API.
 *
 * Build:
 *   cl /nologo /c /O1 /Zl kryos_runtime.c
 *   lib /nologo /OUT:kryos_runtime.lib kryos_runtime.obj
 */

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

static HANDLE kryos__stdout(void) {
    static HANDLE h = NULL;
    if (h == NULL) { h = GetStdHandle(STD_OUTPUT_HANDLE); }
    return h;
}

static SIZE_T kryos__strlen(const char *s) {
    const char *p = s;
    while (*p) { p++; }
    return (SIZE_T)(p - s);
}

void kryos_print_str(const char *s) {
    DWORD written;
    DWORD len = (DWORD)kryos__strlen(s);
    WriteFile(kryos__stdout(), s, len, &written, NULL);
}

void kryos_println_str(const char *s) {
    DWORD written;
    DWORD len = (DWORD)kryos__strlen(s);
    WriteFile(kryos__stdout(), s, len, &written, NULL);
    WriteFile(kryos__stdout(), "\r\n", 2, &written, NULL);
}

void kryos_eprint_str(const char *s) {
    HANDLE h = GetStdHandle(STD_ERROR_HANDLE);
    DWORD written;
    DWORD len = (DWORD)kryos__strlen(s);
    WriteFile(h, s, len, &written, NULL);
}

void kryos_eprintln_str(const char *s) {
    HANDLE h = GetStdHandle(STD_ERROR_HANDLE);
    DWORD written;
    DWORD len = (DWORD)kryos__strlen(s);
    WriteFile(h, s, len, &written, NULL);
    WriteFile(h, "\r\n", 2, &written, NULL);
}

void kryos_builtin_exit(int code) {
    ExitProcess((UINT)code);
}

/*
 * Process entry point. The Kryos self-hosted compiler emits `main` as
 * a regular function returning void; we provide the actual CRT-free
 * entry that calls it and exits cleanly.
 */
extern void main(void);

void mainCRTStartup(void) {
    main();
    ExitProcess(0);
}
