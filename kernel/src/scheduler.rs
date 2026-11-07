//! Single-CPU pre-emptive kernel-thread scheduler.
//!
//! The timer entry saves the interrupted general-purpose and FPU/SIMD state,
//! then asks this module to select the next runnable context. Scheduling never
//! allocates and never formats output in interrupt context.

use core::alloc::Layout;
use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::{heap, pic, pit};

const MAX_TASKS: usize = 3;
const KERNEL_STACK_SIZE: usize = 16 * 1024;
const FXSAVE_SIZE: usize = 512;
const CONTEXT_POINTER_OFFSET: usize = FXSAVE_SIZE;
const REGISTER_FRAME_OFFSET: usize = FXSAVE_SIZE + 8;

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

extern "C" {
    #[link_name = "kanga_timer_interrupt_entry"]
    pub(crate) fn timer_interrupt_entry();
}

/// Install two kernel worker threads and enable pre-emptive round-robin
/// scheduling. The boot thread remains task zero and continues running the
/// shell whenever it is selected by the timer.
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

extern "C" fn scheduler_tick(context: usize) -> usize {
    pit::handle_interrupt();
    pic::acknowledge_master_irq();

    if !ENABLED.load(Ordering::Acquire) {
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
