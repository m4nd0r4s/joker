//! The cycle path of a component without heap allocation (ref/04 §4.13,
//! ADR 0056).
//!
//! `init` reserves the image buffer and the proposal buffer with
//! [`reserve`]. From then on:
//!
//! - `cabi_realloc` lends the reserved image buffer to the host for the image
//!   of the next `on-cycle`, and, during `on-cycle`, serves the values the
//!   host returns from `cycle-io` out of a 4 KiB arena reset at each call;
//! - `on-cycle` is exported by [`export_guest!`](crate::export_guest), not by
//!   `wit-bindgen`: the task writes its proposals in place into the reserved
//!   buffer ([`CycleOut`]), `cycle-result` points at that buffer and at
//!   static error buffers, and the export has no post-return, so the result
//!   stays intact until the next call and the host reads it after the call
//!   returns;
//! - the global allocator of the component, [`CycleAllocator`], traps when a
//!   cycle task allocates, after writing [`ALLOC_TRAP`] to the log, from
//!   which the host reports `J-E0903`.
//!
//! Outside `on-cycle` (`init`, `on-params`, `export-state`, `migrate`) the
//! allocator is the system one. The reserved buffers are never handed to it:
//! freeing memory inside them is a no-op, and growing it copies it out.

/// Bytes of the arena for the values `cycle-io` returns in one call.
pub const READ_ARENA: usize = 4096;
/// Bytes kept of `cycle-result.error.code`, as the host does.
pub const ERROR_CODE: usize = 16;
/// Bytes kept of `cycle-result.error.message`, as the host does.
pub const ERROR_MESSAGE: usize = 256;
/// The log record a component writes before it traps on an allocation in a
/// cycle task. The host reports the code before the colon (ADR 0056).
pub const ALLOC_TRAP: &str = "J-E0903: heap allocation during a cycle task";

fn floor_char_boundary(s: &str, limit: usize) -> usize {
    if s.len() <= limit {
        return s.len();
    }
    let mut i = limit;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Reserves the image buffer (`image_len` bytes, fixed for the generation)
/// and the proposal buffer (`proposal_capacity` bytes), both from
/// `init-context`. Called from `init`, where allocation is allowed; until it
/// is, `on-cycle` gets an empty proposal buffer. Does nothing outside a
/// component.
pub fn reserve(image_len: usize, proposal_capacity: usize) {
    #[cfg(target_arch = "wasm32")]
    wasm::reserve(image_len, proposal_capacity);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (image_len, proposal_capacity);
}

/// What `on-cycle` hands to a task: the reserved proposal buffer and room
/// for `cycle-result.error`, kept on the stack.
pub struct CycleOut<'a> {
    proposals: &'a mut [u8],
    len: usize,
    failed: bool,
    code: [u8; ERROR_CODE],
    code_len: usize,
    message: [u8; ERROR_MESSAGE],
    message_len: usize,
}

impl core::fmt::Debug for CycleOut<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CycleOut")
            .field("capacity", &self.proposals.len())
            .field("len", &self.len)
            .field("error", &self.error())
            .finish()
    }
}

impl<'a> CycleOut<'a> {
    /// A result over `proposals`, empty and without error.
    pub fn new(proposals: &'a mut [u8]) -> Self {
        Self {
            proposals,
            len: 0,
            failed: false,
            code: [0; ERROR_CODE],
            code_len: 0,
            message: [0; ERROR_MESSAGE],
            message_len: 0,
        }
    }

    /// The proposal buffer, `init-context.proposal-capacity` bytes long
    /// (empty before `init`). The task writes the buffer of ref/04 §3.3 from
    /// its start and says how much with [`CycleOut::set_len`].
    pub fn proposals(&mut self) -> &mut [u8] {
        self.proposals
    }

    /// Sets the length of the proposal buffer handed to the host, at most
    /// its capacity.
    pub fn set_len(&mut self, len: usize) {
        self.len = len.min(self.proposals.len());
    }

    /// Sets `cycle-result.error`, truncating the code and the message at a
    /// character boundary.
    pub fn fail(&mut self, code: &str, message: &str) {
        let c = floor_char_boundary(code, ERROR_CODE);
        let m = floor_char_boundary(message, ERROR_MESSAGE);
        self.code[..c].copy_from_slice(&code.as_bytes()[..c]);
        self.message[..m].copy_from_slice(&message.as_bytes()[..m]);
        self.code_len = c;
        self.message_len = m;
        self.failed = true;
    }

    /// The proposal buffer as it will be handed to the host.
    pub fn buffer(&self) -> &[u8] {
        &self.proposals[..self.len]
    }

    /// The error set with [`CycleOut::fail`], if any.
    pub fn error(&self) -> Option<(&str, &str)> {
        self.failed.then(|| {
            (
                core::str::from_utf8(&self.code[..self.code_len]).unwrap_or(""),
                core::str::from_utf8(&self.message[..self.message_len]).unwrap_or(""),
            )
        })
    }
}

/// The `on-cycle` export of a component (ref/04 §2.6.1), which
/// [`export_guest!`](crate::export_guest) wires to the canonical ABI.
pub trait CycleGuest {
    /// Runs cycle task `task` over the packed `image` (ref/04 §3.2) and
    /// writes its proposal buffer, and its error if it returns one, into
    /// `out`. Heap allocation traps here (`J-E0903`).
    fn on_cycle(task: u32, image: &[u8], out: &mut CycleOut<'_>);
}

#[cfg(target_arch = "wasm32")]
pub use wasm::{CycleAllocator, on_cycle};

#[cfg(target_arch = "wasm32")]
mod wasm {
    use core::alloc::{GlobalAlloc, Layout};
    use core::ptr;

    use core::cell::UnsafeCell;

    use super::{ALLOC_TRAP, CycleGuest, CycleOut, ERROR_CODE, ERROR_MESSAGE, READ_ARENA};
    use crate::joker::runtime::log;

    const PROPOSALS_PTR: usize = 0;
    const PROPOSALS_LEN: usize = 4;
    const ERROR_TAG: usize = 8;
    const CODE_PTR: usize = 12;
    const CODE_LEN: usize = 16;
    const MESSAGE_PTR: usize = 20;
    const MESSAGE_LEN: usize = 24;
    const RESULT_SIZE: usize = 32;

    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    enum Phase {
        Setup,
        Ready,
        Cycle,
        Trapping,
    }

    #[repr(C, align(8))]
    struct Bytes<const N: usize>([u8; N]);

    struct State {
        phase: Phase,
        image: *mut u8,
        image_len: usize,
        image_lent: bool,
        proposals: *mut u8,
        proposals_len: usize,
        read_used: usize,
        read: Bytes<READ_ARENA>,
        code: Bytes<ERROR_CODE>,
        message: Bytes<ERROR_MESSAGE>,
        result: Bytes<RESULT_SIZE>,
    }

    /// The state of the cycle path. A component is single-threaded, and every
    /// access goes through a raw pointer to one field, never through a reference
    /// to the whole state, because `cabi_realloc` and the allocator touch it
    /// while a task holds slices of the reserved buffers.
    struct Shared(UnsafeCell<State>);

    unsafe impl Sync for Shared {}

    static STATE: Shared = Shared(UnsafeCell::new(State {
        phase: Phase::Setup,
        image: ptr::null_mut(),
        image_len: 0,
        image_lent: false,
        proposals: ptr::null_mut(),
        proposals_len: 0,
        read_used: 0,
        read: Bytes([0; READ_ARENA]),
        code: Bytes([0; ERROR_CODE]),
        message: Bytes([0; ERROR_MESSAGE]),
        result: Bytes([0; RESULT_SIZE]),
    }));

    fn state() -> *mut State {
        STATE.0.get()
    }

    fn leak(len: usize) -> *mut u8 {
        alloc::vec![0u8; len].leak().as_mut_ptr()
    }

    fn within(p: *mut u8, start: *mut u8, len: usize) -> bool {
        let (p, start) = (p as usize, start as usize);
        len != 0 && p >= start && p - start < len
    }

    fn reserved(p: *mut u8) -> bool {
        let s = state();
        unsafe {
            within(p, (*s).image, (*s).image_len)
                || within(p, (*s).proposals, (*s).proposals_len)
                || within(p, ptr::addr_of_mut!((*s).read).cast::<u8>(), READ_ARENA)
        }
    }

    /// Reserves the image buffer (`image_len` bytes, fixed for the generation)
    /// and the proposal buffer (`proposal_capacity` bytes), both from
    /// `init-context`. Called from `init`, where allocation is allowed; until it
    /// is, `on-cycle` gets an empty proposal buffer.
    pub(super) fn reserve(image_len: usize, proposal_capacity: usize) {
        let s = state();
        unsafe {
            if (*s).image.is_null() || (*s).image_len != image_len {
                (*s).image = leak(image_len);
                (*s).image_len = image_len;
            }
            if (*s).proposals.is_null() || (*s).proposals_len < proposal_capacity {
                (*s).proposals = leak(proposal_capacity);
                (*s).proposals_len = proposal_capacity;
            }
            (*s).phase = Phase::Ready;
        }
    }

    fn trap() -> ! {
        core::arch::wasm32::unreachable()
    }

    fn alloc_trap() -> ! {
        unsafe { (*state()).phase = Phase::Trapping };
        log::log(log::Level::Error, ALLOC_TRAP, &[]);
        trap()
    }

    fn guard() {
        match unsafe { (*state()).phase } {
            Phase::Cycle => alloc_trap(),
            Phase::Trapping => trap(),
            Phase::Setup | Phase::Ready => {}
        }
    }

    /// The global allocator of a component: the system allocator, except
    /// that it traps with `J-E0903` inside `on-cycle` and never frees the
    /// reserved buffers.
    #[derive(Debug)]
    pub struct CycleAllocator;

    unsafe impl GlobalAlloc for CycleAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            guard();
            unsafe { std::alloc::System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            guard();
            unsafe { std::alloc::System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
            if !reserved(p) {
                unsafe { std::alloc::System.dealloc(p, layout) }
            }
        }

        unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            guard();
            if !reserved(p) {
                return unsafe { std::alloc::System.realloc(p, layout, new_size) };
            }
            let Ok(new) = Layout::from_size_align(new_size, layout.align()) else {
                return ptr::null_mut();
            };
            let q = unsafe { std::alloc::System.alloc(new) };
            if !q.is_null() {
                unsafe { ptr::copy_nonoverlapping(p, q, layout.size().min(new_size)) };
            }
            q
        }
    }

    #[global_allocator]
    static ALLOCATOR: CycleAllocator = CycleAllocator;

    unsafe fn heap(old_ptr: *mut u8, old_len: usize, align: usize, new_len: usize) -> *mut u8 {
        if new_len == 0 {
            return align as *mut u8;
        }
        let Ok(new) = Layout::from_size_align(new_len, align) else {
            trap()
        };
        let p = if old_len == 0 {
            unsafe { alloc::alloc::alloc(new) }
        } else {
            let Ok(old) = Layout::from_size_align(old_len, align) else {
                trap()
            };
            unsafe { alloc::alloc::realloc(old_ptr, old, new_len) }
        };
        if p.is_null() {
            trap()
        }
        p
    }

    unsafe fn arena(old_ptr: *mut u8, align: usize, new_len: usize) -> *mut u8 {
        if !old_ptr.is_null() {
            alloc_trap()
        }
        if new_len == 0 {
            return align as *mut u8;
        }
        let s = state();
        let used = unsafe { (*s).read_used };
        let start = used.next_multiple_of(align.max(1));
        match start.checked_add(new_len) {
            Some(end) if end <= READ_ARENA => unsafe {
                (*s).read_used = end;
                ptr::addr_of_mut!((*s).read).cast::<u8>().add(start)
            },
            _ => alloc_trap(),
        }
    }

    /// The canonical ABI `realloc` of the component (ref/04 §4.13).
    ///
    /// # Safety
    ///
    /// Called by the host with the arguments of the canonical ABI.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn cabi_realloc(
        old_ptr: *mut u8,
        old_len: usize,
        align: usize,
        new_len: usize,
    ) -> *mut u8 {
        let s = state();
        unsafe {
            match (*s).phase {
                Phase::Cycle => arena(old_ptr, align, new_len),
                Phase::Trapping => trap(),
                Phase::Ready
                    if old_ptr.is_null()
                        && align == 1
                        && new_len != 0
                        && new_len == (*s).image_len
                        && !(*s).image_lent =>
                {
                    (*s).image_lent = true;
                    (*s).image
                }
                Phase::Setup | Phase::Ready => heap(old_ptr, old_len, align, new_len),
            }
        }
    }

    fn put(result: *mut u8, at: usize, v: usize) {
        unsafe { result.add(at).cast::<u32>().write(v as u32) }
    }

    /// The body of the `on-cycle` export: runs `G` over the image the host
    /// lowered at `image` and returns the address of `cycle-result`.
    ///
    /// # Safety
    ///
    /// Called only from the export that `export_guest!` generates, with the
    /// arguments of the canonical ABI.
    pub unsafe fn on_cycle<G: CycleGuest>(task: i32, image: *mut u8, len: usize) -> *mut u8 {
        wit_bindgen::rt::run_ctors_once();
        let s = state();
        unsafe {
            let before = (*s).phase;
            let lent = len != 0 && image == (*s).image && len == (*s).image_len;
            let bytes: &[u8] = if len == 0 {
                &[]
            } else {
                core::slice::from_raw_parts(image, len)
            };
            let proposals: &mut [u8] = if (*s).proposals_len == 0 {
                &mut []
            } else {
                core::slice::from_raw_parts_mut((*s).proposals, (*s).proposals_len)
            };
            (*s).read_used = 0;
            (*s).phase = Phase::Cycle;
            let mut out = CycleOut::new(proposals);
            G::on_cycle(task as u32, bytes, &mut out);
            (*s).phase = before;
            (*s).image_lent = false;
            let result = ptr::addr_of_mut!((*s).result).cast::<u8>();
            put(result, PROPOSALS_PTR, (*s).proposals as usize);
            put(result, PROPOSALS_LEN, out.len);
            if out.failed {
                let code = ptr::addr_of_mut!((*s).code).cast::<u8>();
                let message = ptr::addr_of_mut!((*s).message).cast::<u8>();
                ptr::copy_nonoverlapping(out.code.as_ptr(), code, out.code_len);
                ptr::copy_nonoverlapping(out.message.as_ptr(), message, out.message_len);
                result.add(ERROR_TAG).write(1);
                put(result, CODE_PTR, code as usize);
                put(result, CODE_LEN, out.code_len);
                put(result, MESSAGE_PTR, message as usize);
                put(result, MESSAGE_LEN, out.message_len);
            } else {
                result.add(ERROR_TAG).write(0);
            }
            if !lent && len != 0 {
                alloc::alloc::dealloc(image, Layout::from_size_align_unchecked(len, 1));
            }
            result
        }
    }
}
