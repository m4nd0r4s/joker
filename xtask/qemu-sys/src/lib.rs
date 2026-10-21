//! The `critical-section` implementation of the QEMU conformance firmware
//! (`joker-qemu-conformance`): a single core, so a critical section masks
//! the interrupts and restores them as they were, the `cs-single-core` of
//! ref/08 §2.4 rule 8. `portable-atomic` takes it for the 64-bit atomics of
//! the core on Cortex-M33 and RISC-V `imac` (AGENTS.md §2 rule 2).
//!
//! This is the only crate of the firmware with `unsafe` code (AGENTS.md §2
//! rule 10): the `Impl` trait is unsafe, and masking the interrupts takes
//! one instruction of inline assembly. On any other target it is empty.

#![no_std]

#[cfg(all(target_os = "none", any(target_arch = "arm", target_arch = "riscv32")))]
mod single_core {
    use core::arch::asm;

    use critical_section::RawRestoreState;

    /// Masks the interrupts of the only core.
    ///
    /// The implementation is sound because the firmware runs on one core:
    /// masking its interrupts excludes every other context, and
    /// `critical-section` hands `release` the state `acquire` returned, in
    /// nesting order. The assembly touches no memory and is a compiler
    /// barrier, so accesses stay inside the section.
    #[derive(Debug)]
    struct SingleCore;

    critical_section::set_impl!(SingleCore);

    unsafe impl critical_section::Impl for SingleCore {
        /// Reads PRIMASK and masks the interrupts; returns whether they
        /// were unmasked.
        #[cfg(target_arch = "arm")]
        unsafe fn acquire() -> RawRestoreState {
            let primask: u32;
            unsafe { asm!("mrs {}, PRIMASK", "cpsid i", out(reg) primask, options(nostack)) };
            primask & 1 == 0
        }

        /// Unmasks the interrupts if they were unmasked when the outermost
        /// section began.
        #[cfg(target_arch = "arm")]
        unsafe fn release(was_enabled: RawRestoreState) {
            if was_enabled {
                unsafe { asm!("cpsie i", options(nostack)) };
            }
        }

        /// Clears `mstatus.MIE`; returns whether it was set.
        #[cfg(target_arch = "riscv32")]
        unsafe fn acquire() -> RawRestoreState {
            let mstatus: usize;
            unsafe { asm!("csrrci {}, mstatus, 8", out(reg) mstatus, options(nostack)) };
            mstatus & 8 != 0
        }

        /// Sets `mstatus.MIE` back if it was set when the outermost section
        /// began.
        #[cfg(target_arch = "riscv32")]
        unsafe fn release(was_enabled: RawRestoreState) {
            if was_enabled {
                unsafe { asm!("csrsi mstatus, 8", options(nostack)) };
            }
        }
    }
}
