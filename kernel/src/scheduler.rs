//! Single-CPU pre-emptive kernel-thread scheduler.
//!
//! The timer entry saves the interrupted general-purpose and FPU/SIMD state,
//! then asks this module to select the next runnable context. Scheduling never
//! allocates and never formats output in interrupt context.

use core::alloc::Layout;
use core::fmt::Write as _;
use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::{heap, pic, pit};

const MAX_TASKS: usize = 3;
const KERNEL_STACK_SIZE: usize = 16 * 1024;
const FXSAVE_SIZE: usize = 512;
const CONTEXT_POINTER_OFFSET: usize = FXSAVE_SIZE;
const REGISTER_FRAME_OFFSET: usize = FXSAVE_SIZE + 8;
const REGISTER_FRAME_CODE_SEGMENT_OFFSET: usize = 15 * size_of::<u64>() + size_of::<u64>();

const RING3_TEST_CODE: u64 = 0x0000_4000_0000_0000;
const RING3_TEST_DATA: u64 = RING3_TEST_CODE + 0x1000;
const RING3_TEST_STACK: u64 = RING3_TEST_CODE + 0x2000;
const RING3_TEST_DONE: u32 = 0x4B41_4E47;
const RING3_TEST_READY: u32 = 0x5245_4144;
static RING3_TEST_DATA_POINTER: AtomicUsize = AtomicUsize::new(0);
static RING3_TEST_ACTIVE: AtomicBool = AtomicBool::new(false);
static RING3_TEST_PASSED: AtomicBool = AtomicBool::new(false);
static RING3_TEST_FAILED: AtomicBool = AtomicBool::new(false);
static RING3_TEST_INTERRUPTS: AtomicU64 = AtomicU64::new(0);
static RING3_TEST_SYSCALLS: AtomicU64 = AtomicU64::new(0);
static RING3_TEST_KERNEL_CS: AtomicUsize = AtomicUsize::new(0);
static RING3_TEST_KERNEL_SS: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
static mut KANGA_RING3_USER_RIP: u64 = 0;
#[no_mangle]
static mut KANGA_RING3_USER_CS: u64 = 0;
#[no_mangle]
static mut KANGA_RING3_USER_RSP: u64 = 0;
#[no_mangle]
static mut KANGA_RING3_USER_SS: u64 = 0;


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum TaskState {
    Empty,
    Runnable,
    Running,
}

#[derive(Clone, Copy)]
struct Task {
    name: &'static str,
    priority: u8,
    state: TaskState,
    context: usize,
}

impl Task {
    const EMPTY: Self = Self {
        name: "",
        priority: 1,
        state: TaskState::Empty,
        context: 0,
    };
}

struct TaskTable(UnsafeCell<[Task; MAX_TASKS]>);

// SAFETY: The table is initialised with interrupts disabled and thereafter
// accessed only by the single-CPU timer interrupt scheduler.
unsafe impl Sync for TaskTable {}

static TASKS: TaskTable = TaskTable(UnsafeCell::new([Task::EMPTY; MAX_TASKS]));
static ENABLED: AtomicBool = AtomicBool::new(false);
static CURRENT_TASK: AtomicUsize = AtomicUsize::new(0);
static CONTEXT_SWITCHES: AtomicU64 = AtomicU64::new(0);
static WORKER_A_TICKS: AtomicU64 = AtomicU64::new(0);
static WORKER_B_TICKS: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct RegisterFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
    instruction_pointer: u64,
    code_segment: u64,
    cpu_flags: u64,
    stack_pointer: u64,
    stack_segment: u64,
}

global_asm!(
    ".global kanga_timer_interrupt_entry",
    "kanga_timer_interrupt_entry:",
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov rdi, rsp",
    "sub rsp, 528",
    "and rsp, -16",
    "mov qword ptr [rsp + 512], rdi",
    "fxsave64 [rsp]",
    "mov rdi, rsp",
    "call {scheduler_tick}",
    "mov rsp, rax",
    "fxrstor64 [rsp]",
    "mov rsp, qword ptr [rsp + 512]",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",
    "iretq",
    scheduler_tick = sym scheduler_tick,
);

global_asm!(
    ".global kanga_syscall_interrupt_entry",
    "kanga_syscall_interrupt_entry:",
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov rdi, rsp",
    "sub rsp, 16",
    "and rsp, -16",
    "mov qword ptr [rsp + 8], rdi",
    "call {syscall_dispatch}",
    "mov rsp, qword ptr [rsp + 8]",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",
    "iretq",
    syscall_dispatch = sym syscall_dispatch,
);

global_asm!(
    ".section .rodata",
    ".balign 16",
    ".global kanga_ring3_user_code_start",
    "kanga_ring3_user_code_start:",
    "movabs rax, 0x0000400000001000",
    "movabs rbx, 0x1111222233334444",
    "movabs rbp, 0x2222333344445555",
    "movabs rdi, 0x3333444455556666",
    "movabs rsi, 0x4444555566667777",
    "movabs rdx, 0x5555666677778888",
    "movabs rcx, 0x6666777788889999",
    "movabs r8,  0x777788889999aaaa",
    "movabs r9,  0x88889999aaaabbbb",
    "movabs r10, 0x9999aaaabbbbcccc",
    "movabs r11, 0xaaaabbbbccccdddd",
    "movabs r12, 0xbbbbccccddddeeee",
    "movabs r13, 0xccccddddeeeeffff",
    "movabs r14, 0xddddeeeeffff0001",
    "movabs r15, 0xeeeeffff00011112",
    "mov dword ptr [rax + 8], 0x52454144",
    "int 0x80",
    "2:",
    "cmp dword ptr [rax], 2",
    "jb 2b",
    "mov dword ptr [rax + 4], 0x4b414e47",
    "3:",
    "pause",
    "jmp 3b",
    ".global kanga_ring3_user_code_end",
    "kanga_ring3_user_code_end:",
    ".text",
    ".global kanga_enter_ring3_test",
    "kanga_enter_ring3_test:",
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov qword ptr [rip + kanga_ring3_kernel_rsp], rsp",
    "mov rax, qword ptr [rip + KANGA_RING3_USER_SS]",
    "push rax",
    "mov rax, qword ptr [rip + KANGA_RING3_USER_RSP]",
    "push rax",
    "push 0x202",
    "mov rax, qword ptr [rip + KANGA_RING3_USER_CS]",
    "push rax",
    "mov rax, qword ptr [rip + KANGA_RING3_USER_RIP]",
    "push rax",
    "iretq",
    ".global kanga_ring3_test_return",
    "kanga_ring3_test_return:",
    "cli",
    "mov rsp, qword ptr [rip + kanga_ring3_kernel_rsp]",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "ret",
    ".section .bss",
    ".balign 8",
    ".global kanga_ring3_kernel_rsp",
    "kanga_ring3_kernel_rsp:",
    ".zero 8",
    ".text",
);



extern "C" {
    #[link_name = "kanga_timer_interrupt_entry"]
    pub(crate) fn timer_interrupt_entry();
    pub(crate) fn syscall_interrupt_entry();
    fn kanga_enter_ring3_test();
    fn kanga_ring3_test_return();
    static kanga_ring3_kernel_rsp: u64;
    static kanga_ring3_user_code_start: u8;
    static kanga_ring3_user_code_end: u8;
}

/// Install two kernel worker threads and enable pre-emptive round-robin
/// scheduling. The boot thread remains task zero and continues running the
/// idle loop whenever it is selected by the timer.
pub fn initialise() -> bool {
    let interrupt_flags: u64;
    // SAFETY: This is privileged x86-64 code; mask IRQ0 while publishing tasks.
    unsafe {
        asm!("pushfq", "pop {}", "cli", out(reg) interrupt_flags);
    }

    if ENABLED.load(Ordering::Acquire) {
        if interrupt_flags & (1 << 9) != 0 {
            // SAFETY: Restore the caller's interrupt state.
            unsafe { asm!("sti", options(nomem, nostack)) };
        }
        return false;
    }

    let stack_layout = match Layout::from_size_align(KERNEL_STACK_SIZE, 16) {
        Ok(layout) => layout,
        Err(_) => {
            restore_interrupts(interrupt_flags);
            return false;
        }
    };

    let worker_a_stack = heap::allocate(stack_layout);
    if worker_a_stack.is_null() {
        restore_interrupts(interrupt_flags);
        return false;
    }

    let worker_b_stack = heap::allocate(stack_layout);
    if worker_b_stack.is_null() {
        // SAFETY: This stack was allocated above and has not been published.
        let _ = unsafe { heap::deallocate(worker_a_stack, stack_layout) };
        restore_interrupts(interrupt_flags);
        return false;
    }

    let code_segment: u16;
    let stack_segment: u16;
    // SAFETY: Reading CS and SS are non-mutating architectural register operations.
    unsafe {
        asm!(
            "mov {0:x}, cs",
            out(reg) code_segment,
            options(nomem, nostack, preserves_flags)
        );
        asm!(
            "mov {0:x}, ss",
            out(reg) stack_segment,
            options(nomem, nostack, preserves_flags)
        );
    }

    let worker_a_context =
        unsafe { build_initial_context(worker_a_stack, worker_a, code_segment, stack_segment) };
    let worker_b_context =
        unsafe { build_initial_context(worker_b_stack, worker_b, code_segment, stack_segment) };

    let tasks = unsafe { &mut *TASKS.0.get() };
    tasks[0] = Task {
        name: "bootstrap",
        priority: 1,
        state: TaskState::Running,
        context: 0,
    };
    tasks[1] = Task {
        name: "worker-a",
        priority: 1,
        state: TaskState::Runnable,
        context: worker_a_context,
    };
    tasks[2] = Task {
        name: "worker-b",
        priority: 1,
        state: TaskState::Runnable,
        context: worker_b_context,
    };

    CURRENT_TASK.store(0, Ordering::Relaxed);
    CONTEXT_SWITCHES.store(0, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Release);
    restore_interrupts(interrupt_flags);
    true
}

/// Return a task's stable name and whether it is currently running.
pub fn task_info(index: usize) -> Option<(&'static str, bool)> {
    if index >= MAX_TASKS || !ENABLED.load(Ordering::Acquire) {
        return None;
    }
    // Names are immutable after initialisation; only the current index changes.
    let name = unsafe { (*TASKS.0.get())[index].name };
    if name.is_empty() {
        None
    } else {
        Some((name, CURRENT_TASK.load(Ordering::Acquire) == index))
    }
}

/// Return a lock-free snapshot for the foreground TASKS command.
pub fn statistics() -> (usize, u64, u64, u64) {
    (
        CURRENT_TASK.load(Ordering::Acquire),
        CONTEXT_SWITCHES.load(Ordering::Relaxed),
        WORKER_A_TICKS.load(Ordering::Relaxed),
        WORKER_B_TICKS.load(Ordering::Relaxed),
    )
}

/// Confirm that timer-driven pre-emption has run both worker threads.
pub fn runtime_self_test() -> bool {
    pit::ticks() >= 6
        && CONTEXT_SWITCHES.load(Ordering::Relaxed) >= 6
        && WORKER_A_TICKS.load(Ordering::Relaxed) > 0
        && WORKER_B_TICKS.load(Ordering::Relaxed) > 0
}

extern "C" fn syscall_dispatch(context: usize) {
    let frame = context as *const RegisterFrame;
    let code_segment = unsafe { ptr::read_unaligned(ptr::addr_of!((*frame).code_segment)) };
    if code_segment & 0x3 == 3 && RING3_TEST_ACTIVE.load(Ordering::Acquire) {
        RING3_TEST_SYSCALLS.fetch_add(1, Ordering::Relaxed);
    }
}

extern "C" fn scheduler_tick(context: usize) -> usize {
    pit::handle_interrupt();
    pic::acknowledge_master_irq();

    if !ENABLED.load(Ordering::Acquire) {
        return context;
    }

    // A same-CPL interrupt frame contains only RIP, CS and RFLAGS. A ring-3
    // interrupt also contains the interrupted RSP and SS. Until tasks own
    // separate kernel-entry stacks and privilege-aware saved contexts, never
    // hand a user-originated frame to the kernel-thread context switcher.
    let register_frame = unsafe {
        ptr::read_unaligned((context + CONTEXT_POINTER_OFFSET) as *const usize)
    };
    let code_segment = unsafe {
        ptr::read_unaligned(
            (register_frame + REGISTER_FRAME_CODE_SEGMENT_OFFSET) as *const u64,
        )
    };
    if code_segment & 0x3 != 0 {
        if RING3_TEST_ACTIVE.load(Ordering::Acquire) {
            handle_ring3_test_interrupt(register_frame as *mut RegisterFrame);
        }
        return context;
    }

    // The timer interrupt gate has cleared IF. This single-CPU scheduler
    // therefore needs no allocator, heap lock or additional scheduler lock.
    let tasks = unsafe { &mut *TASKS.0.get() };
    let current = CURRENT_TASK.load(Ordering::Relaxed);
    tasks[current].context = context;
    tasks[current].state = TaskState::Runnable;

    let mut selected = current;
    let mut selected_priority = 0_u8;
    for offset in 1..=MAX_TASKS {
        let candidate = (current + offset) % MAX_TASKS;
        let task = tasks[candidate];
        if task.state != TaskState::Empty && task.priority >= selected_priority {
            if task.priority > selected_priority || selected == current {
                selected = candidate;
                selected_priority = task.priority;
            }
        }
    }

    tasks[selected].state = TaskState::Running;
    CURRENT_TASK.store(selected, Ordering::Release);
    CONTEXT_SWITCHES.fetch_add(1, Ordering::Relaxed);
    tasks[selected].context
}

/// Run a real ring-3 payload under QEMU and verify timer IRQ return.
pub fn ring3_timer_self_test(hhdm_offset: u64) -> bool {
    if !ENABLED.load(Ordering::Acquire) {
        return false;
    }

    let interrupt_flags: u64;
    unsafe {
        asm!("pushfq", "pop {}", "cli", out(reg) interrupt_flags);
    }

    let Some(first_frame) = crate::memory::with_global_allocator(|allocator| {
        allocator.allocate_contiguous(3)
    }).flatten() else {
        restore_interrupts(interrupt_flags);
        return false;
    };
    let frames = [
        first_frame,
        first_frame + crate::memory::PAGE_SIZE,
        first_frame + crate::memory::PAGE_SIZE * 2,
    ];

    let mut address_space = match crate::paging::create_address_space() {
        Ok(space) => space,
        Err(_) => {
            let _ = crate::memory::with_global_allocator(|allocator| {
                for frame in frames {
                    // SAFETY: These frames were allocated above and remain unmapped.
                    let _ = unsafe { allocator.deallocate_frame(frame) };
                }
            });
            restore_interrupts(interrupt_flags);
            return false;
        }
    };
    let mappings = [
        (
            RING3_TEST_CODE,
            crate::paging::PageFlags::PRESENT | crate::paging::PageFlags::USER,
        ),
        (
            RING3_TEST_DATA,
            crate::paging::PageFlags::PRESENT
                | crate::paging::PageFlags::WRITABLE
                | crate::paging::PageFlags::USER
                | crate::paging::PageFlags::NO_EXECUTE,
        ),
        (
            RING3_TEST_STACK,
            crate::paging::PageFlags::PRESENT
                | crate::paging::PageFlags::WRITABLE
                | crate::paging::PageFlags::USER
                | crate::paging::PageFlags::NO_EXECUTE,
        ),
    ];

    for (index, (virtual_address, flags)) in mappings.iter().enumerate() {
        if address_space.map_page(*virtual_address, frames[index], *flags).is_err() {
            for (mapped_address, _) in mappings.iter().take(index) {
                let _ = address_space.unmap_page(*mapped_address);
            }
            let _ = crate::memory::with_global_allocator(|allocator| {
                for frame in frames {
                    // SAFETY: Every frame belongs to this test allocation and
                    // all successfully installed mappings were removed above.
                    let _ = unsafe { allocator.deallocate_frame(frame) };
                }
            });
            restore_interrupts(interrupt_flags);
            return false;
        }
    }

    let code_start = core::ptr::addr_of!(kanga_ring3_user_code_start) as usize;
    let code_end = core::ptr::addr_of!(kanga_ring3_user_code_end) as usize;
    let code_length = code_end.saturating_sub(code_start);
    if code_length == 0 || code_length > crate::memory::PAGE_SIZE as usize {
        restore_interrupts(interrupt_flags);
        return false;
    }

    let Some(code_pointer) = crate::memory::physical_to_virtual(frames[0], hhdm_offset)
        .map(|address| address as *mut u8)
    else {
        restore_interrupts(interrupt_flags);
        return false;
    };
    let Some(data_pointer) = crate::memory::physical_to_virtual(frames[1], hhdm_offset)
        .map(|address| address as *mut u8)
    else {
        restore_interrupts(interrupt_flags);
        return false;
    };
    let Some(stack_pointer) = crate::memory::physical_to_virtual(frames[2], hhdm_offset)
        .map(|address| address as *mut u8)
    else {
        restore_interrupts(interrupt_flags);
        return false;
    };

    unsafe {
        ptr::copy_nonoverlapping(code_start as *const u8, code_pointer, code_length);
        ptr::write_bytes(data_pointer, 0, crate::memory::PAGE_SIZE as usize);
        ptr::write_bytes(stack_pointer, 0, crate::memory::PAGE_SIZE as usize);
    }

    let code_selector = crate::interrupts::user_code_selector();
    let data_selector = crate::interrupts::user_data_selector();
    let (Some(code_selector), Some(data_selector)) = (code_selector, data_selector) else {
        restore_interrupts(interrupt_flags);
        return false;
    };
    let kernel_code_selector: u16;
    unsafe {
        asm!(
            "mov {0:x}, cs",
            out(reg) kernel_code_selector,
            options(nomem, nostack, preserves_flags)
        );
    }

    let kernel_stack_selector: u16;
    unsafe {
        asm!(
            "mov {0:x}, ss",
            out(reg) kernel_stack_selector,
            options(nomem, nostack, preserves_flags)
        );
    }

    RING3_TEST_DATA_POINTER.store(data_pointer as usize, Ordering::Release);
    RING3_TEST_KERNEL_CS.store(usize::from(kernel_code_selector), Ordering::Release);
    RING3_TEST_KERNEL_SS.store(usize::from(kernel_stack_selector), Ordering::Release);
    RING3_TEST_INTERRUPTS.store(0, Ordering::Relaxed);
    RING3_TEST_SYSCALLS.store(0, Ordering::Relaxed);
    RING3_TEST_PASSED.store(false, Ordering::Relaxed);
    RING3_TEST_FAILED.store(false, Ordering::Relaxed);
    RING3_TEST_ACTIVE.store(true, Ordering::Release);

    let user_stack = RING3_TEST_STACK + crate::memory::PAGE_SIZE - 16;
    unsafe {
        ptr::write_volatile(core::ptr::addr_of_mut!(KANGA_RING3_USER_RIP), RING3_TEST_CODE);
        ptr::write_volatile(
            core::ptr::addr_of_mut!(KANGA_RING3_USER_CS),
            u64::from(code_selector),
        );
        ptr::write_volatile(core::ptr::addr_of_mut!(KANGA_RING3_USER_RSP), user_stack);
        ptr::write_volatile(
            core::ptr::addr_of_mut!(KANGA_RING3_USER_SS),
            u64::from(data_selector),
        );
        let entry_cs = ptr::read_volatile(core::ptr::addr_of!(KANGA_RING3_USER_CS));
        let entry_ss = ptr::read_volatile(core::ptr::addr_of!(KANGA_RING3_USER_SS));
        let entry_rsp = ptr::read_volatile(core::ptr::addr_of!(KANGA_RING3_USER_RSP));
        let _ = writeln!(
            crate::serial::Writer,
            "Ring-3 IRET frame: CS=0x{:04x}, SS=0x{:04x}, RSP=0x{:016x}",
            entry_cs,
            entry_ss,
            entry_rsp
        );
        let previous_root = match address_space.activate() {
            Ok(root) => root,
            Err(_) => {
                RING3_TEST_ACTIVE.store(false, Ordering::Release);
                restore_interrupts(interrupt_flags);
                return false;
            }
        };
        kanga_enter_ring3_test();
        let restored = crate::paging::AddressSpace::restore_root(previous_root).is_ok();
        RING3_TEST_ACTIVE.store(false, Ordering::Release);
        if !restored {
            restore_interrupts(interrupt_flags);
            return false;
        }
    }

    let passed = RING3_TEST_PASSED.load(Ordering::Acquire)
        && !RING3_TEST_FAILED.load(Ordering::Acquire)
        && RING3_TEST_INTERRUPTS.load(Ordering::Relaxed) >= 3
        && RING3_TEST_SYSCALLS.load(Ordering::Relaxed) == 1;
    let mut unmapped = true;
    for (virtual_address, _) in mappings {
        unmapped &= address_space.unmap_page(virtual_address).is_ok();
    }
    let released = if unmapped {
        let frames_returned = crate::memory::with_global_allocator(|allocator| {
            let mut all_returned = true;
            for frame in frames {
                // SAFETY: The user mappings were removed and the test owns these frames.
                all_returned &= unsafe { allocator.deallocate_frame(frame) };
            }
            all_returned
        }).unwrap_or(false);
        frames_returned && address_space.destroy()
    } else {
        false
    };
    restore_interrupts(interrupt_flags);
    passed && released
}

fn handle_ring3_test_interrupt(frame: *mut RegisterFrame) {
    let frame = unsafe { &mut *frame };
    let data_pointer = RING3_TEST_DATA_POINTER.load(Ordering::Acquire) as *mut u32;
    let user_code = crate::interrupts::user_code_selector().map(u64::from);
    let user_data = crate::interrupts::user_data_selector().map(u64::from);

    let context_valid = user_code == Some(frame.code_segment)
        && user_data == Some(frame.stack_segment)
        && frame.instruction_pointer >= RING3_TEST_CODE
        && frame.instruction_pointer < RING3_TEST_CODE + crate::memory::PAGE_SIZE
        && frame.stack_pointer == RING3_TEST_STACK + crate::memory::PAGE_SIZE - 16
        && frame.cpu_flags & (1 << 9) != 0
        && frame.rax == RING3_TEST_DATA
        && frame.rbx == 0x1111_2222_3333_4444
        && frame.rbp == 0x2222_3333_4444_5555
        && frame.rdi == 0x3333_4444_5555_6666
        && frame.rsi == 0x4444_5555_6666_7777
        && frame.rdx == 0x5555_6666_7777_8888
        && frame.rcx == 0x6666_7777_8888_9999
        && frame.r8 == 0x7777_8888_9999_aaaa
        && frame.r9 == 0x8888_9999_aaaa_bbbb
        && frame.r10 == 0x9999_aaaa_bbbb_cccc
        && frame.r11 == 0xaaaa_bbbb_cccc_dddd
        && frame.r12 == 0xbbbb_cccc_dddd_eeee
        && frame.r13 == 0xcccc_dddd_eeee_ffff
        && frame.r14 == 0xdddd_eeee_ffff_0001
        && frame.r15 == 0xeeee_ffff_0001_1112;

    if data_pointer.is_null() {
        RING3_TEST_FAILED.store(true, Ordering::Release);
        return_to_kernel_test_context(frame);
        return;
    }

    // The first timer interrupt can arrive while ring-3 code is still loading
    // its sentinel registers. Ignore those early frames until the user code
    // publishes READY after all registers have their expected values.
    let ready = unsafe { ptr::read_volatile(data_pointer.add(2)) };
    if ready != RING3_TEST_READY {
        return;
    }

    if !context_valid {
        RING3_TEST_FAILED.store(true, Ordering::Release);
        return_to_kernel_test_context(frame);
        return;
    }

    let tick_count = unsafe { ptr::read_volatile(data_pointer) };
    unsafe {
        ptr::write_volatile(data_pointer, tick_count.saturating_add(1));
    }
    let interrupts = RING3_TEST_INTERRUPTS.fetch_add(1, Ordering::Relaxed) + 1;
    let completion = unsafe { ptr::read_volatile(data_pointer.add(1)) };
    if completion == RING3_TEST_DONE && tick_count >= 2 && interrupts >= 3 {
        RING3_TEST_PASSED.store(true, Ordering::Release);
        return_to_kernel_test_context(frame);
    }
}

fn return_to_kernel_test_context(frame: &mut RegisterFrame) {
    frame.instruction_pointer = kanga_ring3_test_return as *const () as usize as u64;
    frame.code_segment = RING3_TEST_KERNEL_CS.load(Ordering::Acquire) as u64;
    frame.stack_segment = RING3_TEST_KERNEL_SS.load(Ordering::Acquire) as u64;
    // IRETQ always consumes the five-slot 64-bit frame. Returning to ring 0
    // therefore needs the saved kernel SS:RSP as well as the kernel CS:RIP.
    // The assembly trampoline restores this same stack before popping the
    // callee-saved registers. Keep interrupts disabled until that is complete.
    frame.stack_pointer = unsafe {
        ptr::read_volatile(core::ptr::addr_of!(kanga_ring3_kernel_rsp))
    };
    frame.cpu_flags = (frame.cpu_flags | 2) & !(1 << 9);
}

unsafe fn build_initial_context(
    stack: *mut u8,
    entry: fn() -> !,
    code_segment: u16,
    stack_segment: u16,
) -> usize {
    // SAFETY: The caller owns a live heap allocation of KERNEL_STACK_SIZE bytes.
    // The initial frame is placed at its upper end and remains inside that stack.
    unsafe {
        let stack_end = stack.add(KERNEL_STACK_SIZE) as usize;
        // SysV x86-64 requires RSP == 8 (mod 16) at a function entry point.
        let stack_top = (stack_end & !0xF).saturating_sub(8);
        let frame_address = stack_top - core::mem::size_of::<RegisterFrame>();
        let context_address = frame_address - REGISTER_FRAME_OFFSET;
        let context = context_address as *mut u8;

        // Initialise a clean x87/SSE state (FCW=0x037f, MXCSR=0x1f80).
        ptr::write_bytes(context, 0, FXSAVE_SIZE);
        ptr::write_unaligned(context.cast::<u16>(), 0x037F);
        ptr::write_unaligned(context.add(24).cast::<u32>(), 0x1F80);
        ptr::write(
            context.add(CONTEXT_POINTER_OFFSET).cast::<usize>(),
            frame_address,
        );

        let frame = frame_address as *mut RegisterFrame;
        frame.write(RegisterFrame {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rbp: 0,
            rdi: 0,
            rsi: 0,
            rdx: 0,
            rcx: 0,
            rbx: 0,
            rax: 0,
            instruction_pointer: entry as usize as u64,
            code_segment: u64::from(code_segment),
            cpu_flags: 0x202,
            stack_pointer: stack_top as u64,
            stack_segment: u64::from(stack_segment),
        });

        context_address
    }
}

fn worker_a() -> ! {
    loop {
        WORKER_A_TICKS.fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
}

fn worker_b() -> ! {
    loop {
        WORKER_B_TICKS.fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
}

fn restore_interrupts(flags: u64) {
    if flags & (1 << 9) != 0 {
        // SAFETY: Restore IF only if it was enabled before scheduler setup.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
}
