//! Diagnostic fault tracer (gated by env KRYOS_FAULT_TRACE=1).
//!
//! Installs a Windows vectored exception handler that, on an access violation
//! (segfault in generated code), prints the faulting instruction's module-
//! relative address (RVA) and exits. The RVA maps directly to a function in
//! `dumpbin /disasm` output, pinpointing a crash without an external debugger.
//! Zero effect unless KRYOS_FAULT_TRACE is set.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};

    static INSTALLED: AtomicBool = AtomicBool::new(false);

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        flags: u32,
        next: *mut ExceptionRecord,
        address: *mut u8,
        // (trailing fields omitted; we only read up to `address`)
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut u8,
    }

    extern "system" {
        fn AddVectoredExceptionHandler(
            first: u32,
            handler: extern "system" fn(*mut ExceptionPointers) -> i32,
        ) -> *mut u8;
        fn GetModuleHandleW(name: *const u16) -> *mut u8;
        fn GetCurrentThread() -> *mut u8;
        fn GetCurrentProcess() -> *mut u8;
        fn DuplicateHandle(
            src_proc: *mut u8,
            src_handle: *mut u8,
            tgt_proc: *mut u8,
            tgt_handle: *mut *mut u8,
            access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
        fn SuspendThread(thread: *mut u8) -> u32;
        fn ResumeThread(thread: *mut u8) -> u32;
        fn GetThreadContext(thread: *mut u8, ctx: *mut u8) -> i32;
        fn ExitProcess(code: u32) -> !;
    }

    // x64 CONTEXT: 16-byte aligned, ~1232 bytes. ContextFlags at offset 0x30,
    // Rip at offset 0xF8. We only need those two, so use a raw aligned buffer.
    #[repr(C, align(16))]
    struct RawContext {
        buf: [u8; 1352],
    }

    const CONTEXT_AMD64: u32 = 0x0010_0000;
    const CONTEXT_CONTROL: u32 = CONTEXT_AMD64 | 0x0000_0001;
    const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;

    /// Watchdog: when KRYOS_WATCHDOG is set, sleep KRYOS_WATCHDOG_S seconds
    /// (default 8), then suspend the main thread and read its RIP. Catches a
    /// CPU-bound hang regardless of whether it calls any runtime function.
    fn start_watchdog() {
        let secs: u64 = std::env::var("KRYOS_WATCHDOG_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8);
        // Duplicate the (pseudo) current-thread handle into a real handle the
        // watchdog thread can use.
        let mut main_thread: *mut u8 = std::ptr::null_mut();
        unsafe {
            let proc = GetCurrentProcess();
            DuplicateHandle(
                proc,
                GetCurrentThread(),
                proc,
                &mut main_thread,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            );
        }
        let main_thread_usize = main_thread as usize;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let thread = main_thread_usize as *mut u8;
            let base = unsafe { GetModuleHandleW(std::ptr::null()) as usize };
            // Sample the main thread's RIP several times. CRITICAL: resume the
            // thread BEFORE printing -- a suspended main thread may hold the
            // stdio lock, so eprintln while suspended deadlocks. Read RIP, resume,
            // then record. Multiple samples distinguish a tight loop (stable RVA)
            // from a sprawling allocation path (varying RVA).
            let mut rvas = [0usize; 5];
            for slot in rvas.iter_mut() {
                let rip = unsafe {
                    SuspendThread(thread);
                    let mut ctx = RawContext { buf: [0u8; 1352] };
                    let flags_ptr = ctx.buf.as_mut_ptr().add(0x30) as *mut u32;
                    *flags_ptr = CONTEXT_CONTROL;
                    GetThreadContext(thread, ctx.buf.as_mut_ptr());
                    let r = *(ctx.buf.as_ptr().add(0xF8) as *const u64) as usize;
                    ResumeThread(thread);
                    r
                };
                *slot = rip.wrapping_sub(base);
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            eprintln!(
                "[WATCHDOG] {}s base={:#x} RVAs={:#x} {:#x} {:#x} {:#x} {:#x}",
                secs, base, rvas[0], rvas[1], rvas[2], rvas[3], rvas[4]
            );
            std::process::exit(78);
        });
    }

    extern "system" fn handler(info: *mut ExceptionPointers) -> i32 {
        unsafe {
            if !info.is_null() {
                let rec = (*info).record;
                if !rec.is_null() {
                    let code = (*rec).code;
                    // Stack overflow: the handler runs on the exhausted stack,
                    // so eprintln (which needs stack to format) would re-fault.
                    // Exit immediately with a distinctive code (253) to confirm
                    // the crash type without touching the stack.
                    if code == 0xC000_00FD {
                        ExitProcess(253);
                    }
                    // Catch all fatal (high-severity, 0xCxxxxxxx) exceptions:
                    // access violation 0xC0000005, stack overflow 0xC00000FD,
                    // illegal instruction 0xC000001D, etc. A silent crash with
                    // no [FAULT] line means we never even reached the handler.
                    if code & 0xF000_0000 == 0xC000_0000 {
                        let addr = (*rec).address as usize;
                        let base = GetModuleHandleW(std::ptr::null()) as usize;
                        let rva = addr.wrapping_sub(base);
                        let kind = if code == 0xC000_0005 {
                            "access violation"
                        } else if code == 0xC000_00FD {
                            "stack overflow"
                        } else {
                            "fatal exception"
                        };
                        eprintln!(
                            "[FAULT] {} code={:#x} VA={:#x} base={:#x} RVA={:#x}",
                            kind, code, addr, base, rva
                        );
                        std::process::exit(77);
                    }
                }
            }
        }
        0 // EXCEPTION_CONTINUE_SEARCH
    }

    use std::sync::atomic::AtomicU8;
    static PROBED: AtomicU8 = AtomicU8::new(0);

    pub fn install() {
        // Probe the environment at most ONCE for the whole process. Calling
        // std::env::var_os on every kryos_alloc puts env lookup in the hot
        // path and pollutes fault traces (the env code becomes the apparent
        // crash site). After the first call this is a single atomic load.
        if PROBED.swap(1, Ordering::Relaxed) != 0 {
            return;
        }
        if std::env::var_os("KRYOS_WATCHDOG").is_some() {
            start_watchdog();
        }
        if std::env::var_os("KRYOS_FAULT_TRACE").is_none() {
            return;
        }
        if INSTALLED.swap(true, Ordering::Relaxed) {
            return;
        }
        unsafe {
            AddVectoredExceptionHandler(1, handler);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn install() {}
}

#[inline]
pub fn install() {
    imp::install();
}

// ---------------------------------------------------------------------------
// Hang trap (gated by env KRYOS_HANG_TRAP=1).
//
// An infinite loop in generated code that allocates each iteration (e.g. a
// lexer rebuilding its state struct) calls kryos_alloc millions of times.
// After a large fixed budget of allocations we conclude the process is hung,
// scan the current stack for values that fall inside our module's .text
// range, and print them as RVAs. Mapping those against the linker /MAP file
// identifies the looping call chain. Diagnostic only; no effect unless
// KRYOS_HANG_TRAP is set.
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod hang_imp {
    use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

    static COUNT: AtomicU64 = AtomicU64::new(0);
    static ENABLED: AtomicU8 = AtomicU8::new(0); // 0=unprobed, 1=off, 2=on
    static THRESHOLD: AtomicU64 = AtomicU64::new(3_000_000);

    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut u8;
    }

    #[inline]
    pub fn tick() {
        let e = ENABLED.load(Ordering::Relaxed);
        let on = if e == 0 {
            let v = if std::env::var_os("KRYOS_HANG_TRAP").is_some() { 2u8 } else { 1u8 };
            if v == 2 {
                if let Some(s) = std::env::var_os("KRYOS_HANG_N") {
                    if let Ok(n) = s.to_string_lossy().parse::<u64>() {
                        THRESHOLD.store(n, Ordering::Relaxed);
                    }
                }
            }
            ENABLED.store(v, Ordering::Relaxed);
            v == 2
        } else {
            e == 2
        };
        if !on {
            return;
        }
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        if n == THRESHOLD.load(Ordering::Relaxed) {
            capture();
        }
    }

    #[inline(never)]
    fn capture() {
        let marker: usize = 0;
        let sp = &marker as *const usize as usize;
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        let lo = base + 0x1000;
        let hi = base + 0x127000; // .text length ~0x1256c0 from /MAP
        eprintln!("[HANG] alloc budget exceeded; base={:#x} stack code-addrs (rva):", base);
        let mut p = sp;
        let mut found = 0;
        let mut last = 0usize;
        let mut i = 0;
        while i < 16384 {
            let v = unsafe { *(p as *const usize) };
            if v >= lo && v < hi && v != last {
                eprintln!("  rva={:#x}", v - base);
                last = v;
                found += 1;
                if found > 48 {
                    break;
                }
            }
            p += 8;
            i += 1;
        }
        std::process::exit(78);
    }
}

#[cfg(not(windows))]
mod hang_imp {
    #[inline]
    pub fn tick() {}
}

#[inline]
pub fn hang_tick() {
    hang_imp::tick();
}
