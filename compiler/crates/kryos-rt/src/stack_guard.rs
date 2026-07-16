//! Stack overflow detection.
//!
//! Installs a platform-specific handler that converts stack overflow
//! into a readable error message instead of a raw crash/segfault.
//!
//! On Unix, this installs a SIGSEGV handler on an alternate signal stack
//! (`sigaltstack`) and writes "kryos panic: stack overflow" before exiting.
//! On Windows, stack overflow produces STATUS_STACK_OVERFLOW which the
//! Rust runtime intercepts — we set a custom panic hook to print a
//! friendlier message.
//!
//! The signal handler is `async-signal-safe`: it only uses `write(2)` and
//! `_exit(2)` directly via libc — no allocations, no locks.

use std::io::Write;
use std::sync::Once;

/// Install the stack overflow handler for the current platform.
///
/// Idempotent — multiple calls are safe.
pub fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Always set a panic hook (works for Windows + Rust-runtime-intercepted
        // stack overflows on Unix when the program is linked against std).
        std::panic::set_hook(Box::new(|info| {
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("unknown panic");

            if msg.contains("stack overflow") {
                let _ = writeln!(
                    std::io::stderr(),
                    "kryos panic: stack overflow (possible infinite recursion)"
                );
            } else {
                let _ = writeln!(std::io::stderr(), "kryos panic: {}", msg);
            }
        }));

        // On Unix, install SIGSEGV handler on an alternate signal stack.
        #[cfg(unix)]
        unix::install_sigsegv_handler();
    });
}

/// Runtime init function — called from generated main() or at startup.
#[no_mangle]
pub extern "C" fn kryos_rt_init() {
    install();
    // Also install the vectored-exception fault tracer (`fault::install`,
    // gated behind KRYOS_FAULT_TRACE/KRYOS_WATCHDOG/KRYOS_HANG_TRAP -- a
    // no-op unless one of those env vars is set) so AOT binaries get the
    // same [FAULT] RVA reporting the JIT path already gets via kryos-cli's
    // main.rs installing it directly. Previously the ONLY caller of
    // `fault::install` besides that JIT-only call site was `kryos_alloc`,
    // which the struct/array pool allocators (`kryos_calloc`, `pool_alloc`)
    // bypass entirely -- so an AOT program whose first heap traffic is a
    // struct/array never installed the handler at all, and a real crash
    // (found while root-causing the F1 struct/loop/nested-match bug) showed
    // a silent access violation instead of a `[FAULT] ... RVA=...` line.
    // `kryos_rt_init` is called once from `@main` (see
    // kryos-codegen-llvm's emit_main_wrapper), before any user code runs,
    // so this now covers every AOT program regardless of allocator path.
    crate::fault::install();
}

// ---------------------------------------------------------------------------
// Unix implementation
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod unix {
    use core::ffi::{c_int, c_void};
    use core::mem::MaybeUninit;
    use core::ptr;

    // libc constants & types (subset we need). Values are stable on
    // linux-glibc, linux-musl, macOS, and *BSD for the architectures we care
    // about (x86_64, aarch64).
    #[cfg(target_os = "linux")]
    const SIGSEGV: c_int = 11;
    #[cfg(target_os = "linux")]
    const SIGBUS: c_int = 7;
    #[cfg(target_os = "linux")]
    const SA_SIGINFO: c_int = 0x00000004;
    #[cfg(target_os = "linux")]
    const SA_ONSTACK: c_int = 0x08000000;
    #[cfg(target_os = "linux")]
    const SA_NODEFER: c_int = 0x40000000;
    #[cfg(target_os = "linux")]
    const SA_RESETHAND: c_int = 0x80000000u32 as c_int;

    #[cfg(target_os = "macos")]
    const SIGSEGV: c_int = 11;
    #[cfg(target_os = "macos")]
    const SIGBUS: c_int = 10;
    #[cfg(target_os = "macos")]
    const SA_SIGINFO: c_int = 0x0040;
    #[cfg(target_os = "macos")]
    const SA_ONSTACK: c_int = 0x0001;
    #[cfg(target_os = "macos")]
    const SA_NODEFER: c_int = 0x0010;
    #[cfg(target_os = "macos")]
    const SA_RESETHAND: c_int = 0x0004;

    // Generic fallback (BSDs etc.).
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const SIGSEGV: c_int = 11;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const SIGBUS: c_int = 10;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const SA_SIGINFO: c_int = 0x0040;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const SA_ONSTACK: c_int = 0x0001;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const SA_NODEFER: c_int = 0x0010;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const SA_RESETHAND: c_int = 0x0004;

    const STDERR_FD: c_int = 2;
    const MINSIGSTKSZ_FALLBACK: usize = 32 * 1024; // 32 KiB — generous

    // Layout of `struct sigaction` differs across platforms, so we rely on
    // libc by linking these wrappers ourselves. Use the C-ABI `sigaction(2)`
    // via a thin wrapper that we declare extern; the layout of `struct
    // sigaction` is OS-specific so we define it per-platform.

    #[cfg(target_os = "linux")]
    #[repr(C)]
    struct sigaction_t {
        sa_sigaction: usize, // function pointer or SIG_DFL/SIG_IGN
        sa_mask: [u64; 16],  // sigset_t — 128 bytes on glibc
        sa_flags: c_int,
        sa_restorer: usize,
    }

    #[cfg(target_os = "macos")]
    #[repr(C)]
    struct sigaction_t {
        sa_sigaction: usize,
        sa_mask: u32, // sigset_t on macOS is 32-bit
        sa_flags: c_int,
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[repr(C)]
    struct sigaction_t {
        sa_sigaction: usize,
        sa_mask: [u64; 16],
        sa_flags: c_int,
        _pad: [u8; 64],
    }

    #[repr(C)]
    struct stack_t {
        ss_sp: *mut c_void,
        ss_flags: c_int,
        ss_size: usize,
    }

    extern "C" {
        fn sigaction(
            signum: c_int,
            act: *const sigaction_t,
            oldact: *mut sigaction_t,
        ) -> c_int;
        fn sigaltstack(ss: *const stack_t, old_ss: *mut stack_t) -> c_int;
        fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        fn _exit(status: c_int) -> !;
    }

    // ---------------------------------------------------------------------
    // Signal handler — must be async-signal-safe.
    // ---------------------------------------------------------------------
    extern "C" fn handle_segv(
        _signum: c_int,
        _info: *mut c_void,
        _ctx: *mut c_void,
    ) {
        // We cannot easily distinguish a true stack overflow from a generic
        // SIGSEGV inside a signal handler without inspecting `siginfo_t` and
        // current stack pointer. We conservatively print "stack overflow"
        // since that's the most common reason a Kryos program faults — wild
        // pointer derefs from safe code should be impossible without
        // `unsafe`, and the stdlib's `unsafe` blocks are audited.
        const MSG: &[u8] = b"kryos panic: stack overflow (possible infinite recursion)\n";
        unsafe {
            // SAFETY: `write` is async-signal-safe. We pass a valid pointer
            // and length to a static buffer.
            let _ = write(STDERR_FD, MSG.as_ptr() as *const c_void, MSG.len());
            // SAFETY: `_exit` is async-signal-safe and does not return.
            _exit(134); // 128 + SIGABRT-ish — distinct from 139 (SIGSEGV)
        }
    }

    /// Install the SIGSEGV/SIGBUS handler with an alternate signal stack.
    ///
    /// SAFETY: This is a process-wide modification. We only call this from
    /// the `install()` initializer guarded by `Once`, so it runs exactly
    /// once and the resulting state is valid for the rest of the process
    /// lifetime. The alternate stack is leaked intentionally — it must live
    /// for the rest of the program.
    pub fn install_sigsegv_handler() {
        unsafe {
            // 1. Allocate alternate stack. Use 64 KiB — plenty for our
            //    minimal handler. Leak intentionally (lives for process).
            let alt_size = MINSIGSTKSZ_FALLBACK.max(64 * 1024);
            let alt_stack = vec![0u8; alt_size].leak();
            let ss = stack_t {
                ss_sp: alt_stack.as_mut_ptr() as *mut c_void,
                ss_flags: 0,
                ss_size: alt_size,
            };
            if sigaltstack(&ss, ptr::null_mut()) != 0 {
                // Couldn't install alt stack — bail out, panic hook still
                // catches Rust-level stack overflows.
                return;
            }

            // 2. Install SIGSEGV handler.
            let mut act: sigaction_t = MaybeUninit::zeroed().assume_init();
            act.sa_sigaction = handle_segv as *const () as usize;
            act.sa_flags = SA_SIGINFO | SA_ONSTACK | SA_NODEFER | SA_RESETHAND;
            let _ = sigaction(SIGSEGV, &act, ptr::null_mut());
            let _ = sigaction(SIGBUS, &act, ptr::null_mut());
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent() {
        // Multiple installs must not panic or crash.
        install();
        install();
        install();
    }

    #[test]
    fn kryos_rt_init_callable() {
        // The extern "C" entry point should be safely callable.
        kryos_rt_init();
    }
}
