//! Darwin arm64 native-entry recovery. Only owned guard/poll faults are ours.
use super::{ACTIVE_CONTEXT, CallContext, Entry, GuardSupport, TERMINATED_STATUS, trap_status};
use std::sync::OnceLock;

// AAPCS64 preserves x19..x28, fp/lr and the low halves of v8..v15.
const PAIR_BYTES: usize = 2 * size_of::<u64>();
const GENERAL_PAIRS: usize = 5;
const VECTOR_PAIRS: usize = 4;
const GENERAL_BASE: usize = PAIR_BYTES;
const VECTOR_BASE: usize = GENERAL_BASE + GENERAL_PAIRS * PAIR_BYTES;
const FRAME_BYTES: usize = VECTOR_BASE + VECTOR_PAIRS * PAIR_BYTES;

#[unsafe(naked)]
unsafe extern "C" fn trampoline(
    _: *mut CallContext, _: u64, _: u64, _: *const u64, _: *mut u64, _: Entry,
) -> u32 {
    core::arch::naked_asm!(
        "stp x29, x30, [sp, #-{frame}]!",
        "mov x29, sp",
        "stp x19, x20, [sp, #({gpr}+{pair}*0)]",
        "stp x21, x22, [sp, #({gpr}+{pair}*1)]",
        "stp x23, x24, [sp, #({gpr}+{pair}*2)]",
        "stp x25, x26, [sp, #({gpr}+{pair}*3)]",
        "stp x27, x28, [sp, #({gpr}+{pair}*4)]",
        "stp d8, d9, [sp, #({simd}+{pair}*0)]",
        "stp d10, d11, [sp, #({simd}+{pair}*1)]",
        "stp d12, d13, [sp, #({simd}+{pair}*2)]",
        "stp d14, d15, [sp, #({simd}+{pair}*3)]",
        "mov x9, sp",
        "str x9, [x0, #{saved_stack}]",
        "blr x5",
        "b {resume}",
        frame = const FRAME_BYTES, pair = const PAIR_BYTES,
        gpr = const GENERAL_BASE, simd = const VECTOR_BASE,
        saved_stack = const std::mem::offset_of!(CallContext, entry_stack),
        resume = sym resume,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn resume() {
    core::arch::naked_asm!(
        "ldp x19, x20, [sp, #({gpr}+{pair}*0)]",
        "ldp x21, x22, [sp, #({gpr}+{pair}*1)]",
        "ldp x23, x24, [sp, #({gpr}+{pair}*2)]",
        "ldp x25, x26, [sp, #({gpr}+{pair}*3)]",
        "ldp x27, x28, [sp, #({gpr}+{pair}*4)]",
        "ldp d8, d9, [sp, #({simd}+{pair}*0)]",
        "ldp d10, d11, [sp, #({simd}+{pair}*1)]",
        "ldp d12, d13, [sp, #({simd}+{pair}*2)]",
        "ldp d14, d15, [sp, #({simd}+{pair}*3)]",
        "ldp x29, x30, [sp], #{frame}",
        "ret",
        frame = const FRAME_BYTES, pair = const PAIR_BYTES,
        gpr = const GENERAL_BASE, simd = const VECTOR_BASE,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn termination() {
    core::arch::naked_asm!("mov w0, #{status}", "b {resume}",
        status = const TERMINATED_STATUS, resume = sym resume);
}

static PREVIOUS_SEGV: OnceLock<libc::sigaction> = OnceLock::new();
static PREVIOUS_BUS: OnceLock<libc::sigaction> = OnceLock::new();
static SUPPORT: OnceLock<Option<GuardSupport>> = OnceLock::new();

pub(super) fn support() -> Option<&'static GuardSupport> {
    SUPPORT.get_or_init(|| {
        super::install_guard_signals(&[(libc::SIGSEGV, &PREVIOUS_SEGV), (libc::SIGBUS, &PREVIOUS_BUS)],
            fault as *const () as usize, libc::SA_SIGINFO | libc::SA_ONSTACK)?;
        Some(GuardSupport { _code: None, trampoline,
            landing: resume as *const () as usize,
            termination: termination as *const () as usize })
    }).as_ref()
}

extern "C" fn fault(signal: i32, info: *mut libc::siginfo_t, raw_context: *mut libc::c_void) {
    // SAFETY: supplied by the signal ABI.
    if unsafe { resume_fault(info, raw_context) } { return; }
    let saved = if signal == libc::SIGBUS { &PREVIOUS_BUS } else { &PREVIOUS_SEGV };
    let Some(previous) = saved.get() else { std::process::abort(); };
    unsafe { super::forward_guard_fault(signal, info, raw_context, previous); }
}

pub(super) unsafe fn resume_fault(info: *mut libc::siginfo_t, raw_context: *mut libc::c_void) -> bool {
    let active = ACTIVE_CONTEXT.with(std::cell::Cell::get);
    // SAFETY: the signal ABI supplies these contexts. ACTIVE_CONTEXT is lent only
    // while its native invocation and trampoline's saved stack are live.
    unsafe {
        let address = (*info).si_addr() as usize;
        if !active.is_null() && let Some(Some(support)) = SUPPORT.get() {
            let landing = if (*active).poll_start <= address && address < (*active).poll_end {
                Some((support.termination, None))
            } else if (*active).guard_start <= address && address < (*active).guard_end {
                Some((support.landing, Some(trap_status(crate::WasmTrap::OutOfBoundsMemory) as u64)))
            } else { None };
            if let Some((landing, status)) = landing {
                let context = &mut *raw_context.cast::<libc::ucontext_t>();
                let registers = &mut (*context.uc_mcontext).__ss;
                registers.__sp = (*active).entry_stack as u64;
                registers.__pc = landing as u64;
                if let Some(status) = status { registers.__x[0] = status; }
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[unsafe(naked)]
    unsafe extern "C" fn clobber_and_poll(_: *mut CallContext, _: u64, _: u64, _: *const u64, _: *mut u64) -> u32 {
        core::arch::naked_asm!(
            "mov x19, #0", "mov x20, #0", "mov x21, #0", "mov x22, #0", "mov x23, #0",
            "mov x24, #0", "mov x25, #0", "mov x26, #0", "mov x27, #0", "mov x28, #0",
            "movi v8.16b, #0", "movi v9.16b, #0", "movi v10.16b, #0", "movi v11.16b, #0",
            "movi v12.16b, #0", "movi v13.16b, #0", "movi v14.16b, #0", "movi v15.16b, #0",
            "ldarb w9, [x1]",
            // This deliberately non-returning probe must fault before reaching here.
            "brk #0",
        );
    }

    #[test]
    fn fault_recovery_preserves_all_callee_saved_words_and_vectors() {
        support().expect("Darwin arm64 fault recovery");
        let interrupt = crate::ExecutionInterrupt::default();
        let (page, bytes) = interrupt.poll_page().expect("owned host-sized page");
        assert_eq!(bytes, unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize);
        interrupt.request_termination();
        for guard in [false, true] {
            let mut context = CallContext::native_adapter(std::ptr::null_mut(), interrupt.atomic_flag(),
                (!guard).then_some((page, bytes)));
            if guard { context.guard_start = page; context.guard_end = page + bytes; }
            let outer = ACTIVE_CONTEXT.with(|active| active.replace(&mut context));
            let mut integers = std::array::from_fn::<_, 10, _>(|i| 0x1234_5678_9abc_0000_u64 + i as u64);
            let mut vectors = std::array::from_fn::<_, 8, _>(|i| 1.25_f64 + i as f64);
            let expected_integers = integers;
            let expected_vectors = vectors;
            let status: u64;
            // SAFETY: the trampoline owns the saved ABI frame, the page is protected,
            // and the active context supplies the matching recovery reservation.
            unsafe { core::arch::asm!(
                // LLVM reserves x19 internally. Preserve its own value while
                // probing the trampoline with a separately supplied sentinel.
                "str x19, [sp, #-{pair}]!", "mov x19, x6",
                "bl {trampoline}",
                "mov x6, x19", "ldr x19, [sp], #{pair}",
                trampoline = sym trampoline, pair = const PAIR_BYTES,
                inlateout("x0") &mut context as *mut CallContext => status,
                in("x1") page, in("x2") 0_u64, in("x3") std::ptr::null::<u64>(),
                in("x4") std::ptr::null_mut::<u64>(), in("x5") clobber_and_poll as *const (),
                inout("x6") integers[0], inout("x20") integers[1], inout("x21") integers[2],
                inout("x22") integers[3], inout("x23") integers[4], inout("x24") integers[5],
                inout("x25") integers[6], inout("x26") integers[7], inout("x27") integers[8], inout("x28") integers[9],
                inout("v8") vectors[0], inout("v9") vectors[1], inout("v10") vectors[2], inout("v11") vectors[3],
                inout("v12") vectors[4], inout("v13") vectors[5], inout("v14") vectors[6], inout("v15") vectors[7],
                clobber_abi("C"),
            ); }
            ACTIVE_CONTEXT.with(|active| active.set(outer));
            assert_eq!(status, if guard { trap_status(crate::WasmTrap::OutOfBoundsMemory) } else { TERMINATED_STATUS } as u64);
            assert_eq!(integers, expected_integers);
            assert_eq!(vectors, expected_vectors);
        }
    }

    #[test]
    fn nested_fault_returns_to_its_nearest_native_entry() {
        unsafe extern "C" fn nested(context: *mut CallContext, page: u64, bytes: u64,
            _: *const u64, out: *mut u64) -> u32 {
            assert_eq!(ACTIVE_CONTEXT.with(std::cell::Cell::get), context);
            // SAFETY: the outer entry lends its interrupt owner and protected page.
            let mut child = CallContext::native_adapter(std::ptr::null_mut(), unsafe { (*context).interrupt },
                Some((page as usize, bytes as usize)));
            let status = unsafe { super::super::enter_native(&mut child, page, bytes,
                std::ptr::null(), std::ptr::null_mut(), super::super::NativeEntryMode::Resumable, clobber_and_poll) };
            assert_eq!(i64::from(status), TERMINATED_STATUS);
            assert_eq!(ACTIVE_CONTEXT.with(std::cell::Cell::get), context);
            unsafe { out.write(42); }
            0
        }
        let interrupt = crate::ExecutionInterrupt::default();
        let (page, bytes) = interrupt.poll_page().unwrap();
        interrupt.request_termination();
        let mut context = CallContext::native_adapter(std::ptr::null_mut(), interrupt.atomic_flag(), None);
        let before = ACTIVE_CONTEXT.with(std::cell::Cell::get);
        let mut result = 0;
        let status = unsafe { super::super::enter_native(&mut context, page as u64, bytes as u64,
            std::ptr::null(), &mut result, super::super::NativeEntryMode::Resumable, nested) };
        assert_eq!(status, 0);
        assert_eq!(result, 42);
        assert_eq!(ACTIVE_CONTEXT.with(std::cell::Cell::get), before);
    }

    #[test]
    fn unrelated_signals_reach_the_previous_handlers() {
        const CHILD: &str = "QUENCH_TEST_DARWIN_FAULT_FORWARDING";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "vm::compiled::guard_aarch64::tests::unrelated_signals_reach_the_previous_handlers", "--test-threads=1"])
                .env(CHILD, "1").status().unwrap();
            assert!(status.success(), "signal-forwarding child: {status}");
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        static FORWARDED: AtomicUsize = AtomicUsize::new(0);
        extern "C" fn previous(_: i32) { FORWARDED.fetch_add(1, Ordering::Relaxed); }
        extern "C" fn previous_info(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
            assert_eq!(signal, libc::SIGSEGV);
            assert!(!info.is_null() && !context.is_null());
            FORWARDED.fetch_add(1, Ordering::Relaxed);
        }
        for signal in [libc::SIGSEGV, libc::SIGBUS, libc::SIGUSR2] {
            // SAFETY: this isolated child installs its own test handler.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = previous as *const () as usize;
                if signal == libc::SIGSEGV {
                    action.sa_sigaction = previous_info as *const () as usize;
                    action.sa_flags = libc::SA_SIGINFO;
                }
                libc::sigemptyset(&mut action.sa_mask);
                assert_eq!(libc::sigaction(signal, &action, std::ptr::null_mut()), 0);
            }
        }
        let first = OnceLock::new();
        let invalid = OnceLock::new();
        assert!(super::super::install_guard_signals(&[(libc::SIGUSR2, &first), (-1, &invalid)],
            fault as *const () as usize, libc::SA_SIGINFO).is_none());
        assert_eq!(unsafe { libc::raise(libc::SIGUSR2) }, 0, "partial install must restore its previous handler");
        support().unwrap();
        for signal in [libc::SIGSEGV, libc::SIGBUS, libc::SIGSEGV] {
            assert_eq!(unsafe { libc::raise(signal) }, 0);
        }
        assert_eq!(FORWARDED.load(Ordering::Relaxed), 4);
        extern "C" fn observer(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
            if unsafe { super::super::NativeFaultHandler::resume(signal, info, context) } { return; }
            FORWARDED.fetch_add(1, Ordering::Relaxed);
        }
        // Model the Node observer: prepare first, consume owned faults before
        // host delivery, then restore the native predecessor on unsubscribe.
        super::super::NativeFaultHandler::prepare(libc::SIGSEGV);
        let mut predecessor: libc::sigaction = unsafe { std::mem::zeroed() };
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = observer as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO;
            libc::sigemptyset(&mut action.sa_mask);
            assert_eq!(libc::sigaction(libc::SIGSEGV, &action, &mut predecessor), 0);
        }
        fault_recovery_preserves_all_callee_saved_words_and_vectors();
        nested_fault_returns_to_its_nearest_native_entry();
        assert_eq!(unsafe { libc::raise(libc::SIGSEGV) }, 0);
        assert_eq!(FORWARDED.load(Ordering::Relaxed), 5);
        assert_eq!(unsafe { libc::sigaction(libc::SIGSEGV, &predecessor, std::ptr::null_mut()) }, 0);
        fault_recovery_preserves_all_callee_saved_words_and_vectors();
        assert_eq!(unsafe { libc::raise(libc::SIGSEGV) }, 0);
        assert_eq!(FORWARDED.load(Ordering::Relaxed), 6);
    }
}
