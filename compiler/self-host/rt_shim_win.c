// rt_shim_win.c -- Windows provider for the 24 self-host intrinsics that
// codegen.kry inlines only on Linux (codegen.kry:1027 gates inlining behind
// target_os=="linux"). On the Windows/PE path stage-1 emits external CALLs to
// these; this shim resolves them so a stage-2 .exe can link + run via the
// external-link path. Forwards the array/string/map builtins to the existing
// Rust runtime ABI (kryos_rt.lib); translates Linux syscalls to Win32/CRT.
//
// Build:  cl /c /O2 /Zl rt_shim_win.c   (then add rt_shim_win.obj to the link)
// /Zl = omit default-lib refs so it composes with the /MD CRT used elsewhere.
#include <stdint.h>
#include <string.h>
#include <io.h>
#include <stdlib.h>
#include <fcntl.h>
#include <sys/stat.h>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

// ---- Rust runtime ABI (kryos_rt.lib). Handles passed as int64_t; on x64 a
// pointer and an int64_t share the same arg register, so this is ABI-safe. ----
extern int64_t kryos_array_new(int64_t elem_size, int64_t cap);
extern void    kryos_array_push(int64_t arr, int64_t val);
extern int64_t kryos_array_get(int64_t arr, int64_t idx);
extern void    kryos_array_set(int64_t arr, int64_t idx, int64_t val);
extern int64_t kryos_array_len(int64_t arr);
extern int64_t kryos_string_new(int64_t ptr, int64_t len);
extern int64_t kryos_string_len(int64_t s);
extern int64_t kryos_map_has(int64_t m, int64_t key);
extern int64_t kryos_map_has_str(int64_t m, int64_t key);
extern int64_t kryos_map_delete(int64_t m, int64_t key);
extern int64_t kryos_map_delete_str(int64_t m, int64_t key);
extern int64_t kryos_map_keys(int64_t m);
extern int64_t kryos_map_keys_str(int64_t m);

// KryosString layout: { len:i64@0, cap:i64@8, data:*u8@16, ref_count:i64@24 }
#define KSTR_DATA_OFF 16

// ---- Raw memory intrinsics ----
int64_t mem_read_i64(int64_t p)            { return *(int64_t*)(intptr_t)p; }
void    mem_write_i64(int64_t p, int64_t v){ *(int64_t*)(intptr_t)p = v; }
int64_t mem_read_byte(int64_t p)           { return (int64_t)*(uint8_t*)(intptr_t)p; }
void    mem_write_byte(int64_t p, int64_t v){ *(uint8_t*)(intptr_t)p = (uint8_t)v; }
void    mem_copy(int64_t src, int64_t dst, int64_t len){ memcpy((void*)(intptr_t)dst, (void*)(intptr_t)src, (size_t)len); }

// ---- String view intrinsics ----
int64_t str_byte_len(int64_t s) { return kryos_string_len(s); }
int64_t str_data_ptr(int64_t s) { return *(int64_t*)((intptr_t)s + KSTR_DATA_OFF); }
int64_t str_from_bytes(int64_t ptr, int64_t len) { return kryos_string_new(ptr, len); }

// ---- Numeric / process ----
double  __int_to_float(int64_t x) { return (double)x; }

int64_t __get_process_args(void) {
    int argc = __argc;
    char** argv = __argv;
    int64_t arr = kryos_array_new(8, argc);
    for (int i = 0; i < argc; i++) {
        int64_t s = kryos_string_new((int64_t)(intptr_t)argv[i], (int64_t)strlen(argv[i]));
        kryos_array_push(arr, s);
    }
    return arr;
}

// ---- __builtin_* (runtime.kry's len/push/pop/range/map_* delegate here).
// Self-host uses these on ARRAYS (strings use str_byte_len); map maps to kryos_map_*.
int64_t __builtin_len(int64_t c)              { return kryos_array_len(c); }
int64_t __builtin_push(int64_t arr, int64_t v){ kryos_array_push(arr, v); return arr; }
int64_t __builtin_pop(int64_t arr) {
    int64_t n = kryos_array_len(arr);
    if (n <= 0) return 0;
    int64_t v = kryos_array_get(arr, n - 1);
    *(int64_t*)(intptr_t)arr = n - 1;   // KryosArray.len is field@0
    return v;
}
int64_t __builtin_range(int64_t start, int64_t end) {
    int64_t n = end - start; if (n < 0) n = 0;
    int64_t arr = kryos_array_new(8, n);
    for (int64_t i = start; i < end; i++) kryos_array_push(arr, i);
    return arr;
}
int64_t __builtin_map_has(int64_t m, int64_t k)        { return kryos_map_has(m, k); }
int64_t __builtin_map_has_str(int64_t m, int64_t k)    { return kryos_map_has_str(m, k); }
int64_t __builtin_map_delete(int64_t m, int64_t k)     { return kryos_map_delete(m, k); }
int64_t __builtin_map_delete_str(int64_t m, int64_t k) { return kryos_map_delete_str(m, k); }
int64_t __builtin_map_keys(int64_t m)                  { return kryos_map_keys(m); }
int64_t __builtin_map_keys_str(int64_t m)              { return kryos_map_keys_str(m); }

// ---- Linux syscall translation (runtime.kry rt_alloc/rt_free/rt_write/exit) ----
// Linux x86_64 nrs: read=0 write=1 open=2 close=3 mmap=9 munmap=11 exit_group=231
static int64_t do_syscall(int64_t nr, int64_t a1, int64_t a2, int64_t a3) {
    switch (nr) {
        case 0:  return (int64_t)_read((int)a1, (void*)(intptr_t)a2, (unsigned)a3);     // read(fd,buf,n)
        case 1:  return (int64_t)_write((int)a1, (const void*)(intptr_t)a2, (unsigned)a3); // write(fd,buf,n)
        case 2: { // open(path, linux_flags, mode) -> translate Linux O_* to Win _O_*
            const char* path = (const char*)(intptr_t)a1;
            int lf = (int)a2;
            int wf = _O_BINARY;                 // avoid CRLF text translation
            if (lf & 1)   wf |= _O_WRONLY;      // Linux O_WRONLY=1
            if (lf & 64)  wf |= _O_CREAT;       // Linux O_CREAT=0100=64
            if (lf & 512) wf |= _O_TRUNC;       // Linux O_TRUNC=01000=512
            return (int64_t)_open(path, wf, _S_IREAD | _S_IWRITE);
        }
        case 3:  return (int64_t)_close((int)a1);                                        // close(fd)
        case 11: VirtualFree((void*)(intptr_t)a1, 0, MEM_RELEASE); return 0;             // munmap(ptr,len)
        case 231: ExitProcess((UINT)a1); return 0;                                       // exit_group(code)
        default: return -1;
    }
}
int64_t syscall1(int64_t nr, int64_t a1)                                   { return do_syscall(nr, a1, 0, 0); }
int64_t syscall2(int64_t nr, int64_t a1, int64_t a2)                       { return do_syscall(nr, a1, a2, 0); }
int64_t syscall3(int64_t nr, int64_t a1, int64_t a2, int64_t a3)           { return do_syscall(nr, a1, a2, a3); }
int64_t syscall6(int64_t nr, int64_t a1, int64_t a2, int64_t a3, int64_t a4, int64_t a5, int64_t a6) {
    if (nr == 9) { // mmap(addr,len,prot,flags,fd,off) -> commit RW pages
        void* p = VirtualAlloc(NULL, (SIZE_T)a2, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        return (int64_t)(intptr_t)p;
    }
    return do_syscall(nr, a1, a2, a3);
}
