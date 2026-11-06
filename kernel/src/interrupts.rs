//! x86_64 IDT, exception diagnostics and emergency exception stacks.
//!
//! CPU exceptions are handled by vector-specific ABI-correct entry points.
//! Fatal exceptions never return. Double faults, NMIs, machine checks and page
//! faults use dedicated TSS interrupt-stack-table (IST) stacks.

use core::arch::asm;
use core::fmt::Write;
use core::mem::size_of;
use core::ptr;

const IDT_ENTRIES: usize = 256;
const EMERGENCY_STACK_SIZE: usize = 16 * 1024;
const GDT_CAPACITY: usize = 16;
const GDT_TSS_TYPE: u64 = 0x89;
const IST_DOUBLE_FAULT: u8 = 1;
const IST_NMI: u8 = 2;
const IST_MACHINE_CHECK: u8 = 3;
const IST_PAGE_FAULT: u8 = 4;

#[derive(Clone, Copy)]
#[repr(C)]
struct InterruptStackFrame {
    instruction_pointer: u64,
    code_segment: u64,
    cpu_flags: u64,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    options: u16,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        options: 0,
        offset_middle: 0,
        offset_high: 0,
        reserved: 0,
    };

    fn new(handler: usize, selector: u16, ist: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            options: 0x8E00 | u16::from(ist & 0x07),
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct TaskStateSegment {
    reserved_1: u32,
    rsp: [u64; 3],
    reserved_2: u64,
    ist: [u64; 7],
    reserved_3: u64,
    reserved_4: u16,
    io_map_base: u16,
}

#[repr(C, align(16))]
struct EmergencyStack([u8; EMERGENCY_STACK_SIZE]);

static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry::MISSING; IDT_ENTRIES];
static mut GDT: [u64; GDT_CAPACITY] = [0; GDT_CAPACITY];
static mut TSS: TaskStateSegment = TaskStateSegment {
    reserved_1: 0,
    rsp: [0; 3],
    reserved_2: 0,
    ist: [0; 7],
    reserved_3: 0,
    reserved_4: 0,
    io_map_base: size_of::<TaskStateSegment>() as u16,
};
static mut DOUBLE_FAULT_STACK: EmergencyStack = EmergencyStack([0; EMERGENCY_STACK_SIZE]);
static mut NMI_STACK: EmergencyStack = EmergencyStack([0; EMERGENCY_STACK_SIZE]);
static mut MACHINE_CHECK_STACK: EmergencyStack = EmergencyStack([0; EMERGENCY_STACK_SIZE]);
static mut PAGE_FAULT_STACK: EmergencyStack = EmergencyStack([0; EMERGENCY_STACK_SIZE]);

/// Installs the GDT TSS descriptor, exception gates, PIC and PIT.
///
/// The TSS is installed before the IDT is loaded so every IST-backed gate is
/// valid before the processor can deliver an exception through it.
pub fn initialise() {
    if !install_tss() {
        let _ = writeln!(
            crate::serial::Writer,
            "KANGA FATAL: unable to install the exception TSS"
        );
        halt_forever();
    }

    let selector: u16;
    // SAFETY: Reading CS is a non-mutating architectural register operation.
    unsafe {
        asm!("mov {0:x}, cs", out(reg) selector, options(nomem, nostack, preserves_flags));
    }

    // SAFETY: This is the single boot-time IDT writer. Maskable interrupts
    // remain disabled until the IDT, PIC and PIT have all been configured.
    unsafe {
        let idt = ptr::addr_of_mut!(IDT).cast::<IdtEntry>();
        for vector in 0..IDT_ENTRIES {
            idt.add(vector).write(IdtEntry::new(
                unexpected_interrupt as *const () as usize,
                selector,
                0,
            ));
        }

        set_gate(idt, 0, exception_0 as *const () as usize, selector, 0);
        set_gate(idt, 1, exception_1 as *const () as usize, selector, 0);
        set_gate(idt, 2, exception_2 as *const () as usize, selector, IST_NMI);
        set_gate(idt, 3, exception_3 as *const () as usize, selector, 0);
        set_gate(idt, 4, exception_4 as *const () as usize, selector, 0);
        set_gate(idt, 5, exception_5 as *const () as usize, selector, 0);
        set_gate(idt, 6, exception_6 as *const () as usize, selector, 0);
        set_gate(idt, 7, exception_7 as *const () as usize, selector, 0);
        set_gate(
            idt,
            8,
            exception_8 as *const () as usize,
            selector,
            IST_DOUBLE_FAULT,
        );
        set_gate(idt, 9, exception_9 as *const () as usize, selector, 0);
        set_gate(idt, 10, exception_10 as *const () as usize, selector, 0);
        set_gate(idt, 11, exception_11 as *const () as usize, selector, 0);
        set_gate(idt, 12, exception_12 as *const () as usize, selector, 0);
        set_gate(idt, 13, exception_13 as *const () as usize, selector, 0);
        set_gate(
            idt,
            14,
            exception_14 as *const () as usize,
            selector,
            IST_PAGE_FAULT,
        );
        set_gate(idt, 15, exception_15 as *const () as usize, selector, 0);
        set_gate(idt, 16, exception_16 as *const () as usize, selector, 0);
        set_gate(idt, 17, exception_17 as *const () as usize, selector, 0);
        set_gate(
            idt,
            18,
            exception_18 as *const () as usize,
            selector,
            IST_MACHINE_CHECK,
        );
        set_gate(idt, 19, exception_19 as *const () as usize, selector, 0);
        set_gate(idt, 20, exception_20 as *const () as usize, selector, 0);
        set_gate(idt, 21, exception_21 as *const () as usize, selector, 0);
        set_gate(idt, 22, exception_22 as *const () as usize, selector, 0);
        set_gate(idt, 23, exception_23 as *const () as usize, selector, 0);
        set_gate(idt, 24, exception_24 as *const () as usize, selector, 0);
        set_gate(idt, 25, exception_25 as *const () as usize, selector, 0);
        set_gate(idt, 26, exception_26 as *const () as usize, selector, 0);
        set_gate(idt, 27, exception_27 as *const () as usize, selector, 0);
        set_gate(idt, 28, exception_28 as *const () as usize, selector, 0);
        set_gate(idt, 29, exception_29 as *const () as usize, selector, 0);
        set_gate(idt, 30, exception_30 as *const () as usize, selector, 0);
        set_gate(idt, 31, exception_31 as *const () as usize, selector, 0);
        set_gate(idt, 0x20, timer_irq as *const () as usize, selector, 0);
        set_gate(idt, 0x21, keyboard_irq as *const () as usize, selector, 0);

        let pointer = IdtPointer {
            limit: (size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
            base: ptr::addr_of!(IDT) as u64,
        };
        asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags));
    }

    let _ = writeln!(crate::serial::Writer, "Interrupts: 256-entry IDT installed");
    let _ = writeln!(
        crate::serial::Writer,
        "Interrupts: 32 vector-specific CPU exception handlers installed"
    );
    let _ = writeln!(
        crate::serial::Writer,
        "Interrupts: CPU exception gates 0-31 configured"
    );
    let _ = writeln!(
        crate::serial::Writer,
        "Interrupts: double-fault, NMI, machine-check and page-fault IST stacks ready"
    );
    crate::pic::initialise();
    let _ = writeln!(
        crate::serial::Writer,
        "Interrupts: PIC remapped; timer IRQ0 and keyboard IRQ1 unmasked"
    );
    crate::pit::initialise();
    let _ = writeln!(crate::serial::Writer, "Timer: PIT configured at 100 Hz");
    // SAFETY: The IDT, TSS, PIC and PIT are ready, and IRQ0/IRQ1 have handlers.
    unsafe {
        asm!("sti", options(nomem, nostack));
    }
    let _ = writeln!(crate::serial::Writer, "Interrupts: keyboard IRQ1 enabled");
}

unsafe fn set_gate(idt: *mut IdtEntry, vector: usize, handler: usize, selector: u16, ist: u8) {
    // SAFETY: The caller supplies a valid vector within the 256-entry IDT.
    unsafe {
        idt.add(vector).write(IdtEntry::new(handler, selector, ist));
    }
}

/// Copies the active GDT, appends a 64-bit TSS descriptor and loads TR.
///
/// Keeping all existing descriptors byte-for-byte preserves the code/data
/// selectors supplied by Limine while giving the kernel a private TSS.
fn install_tss() -> bool {
    // SAFETY: The boot CPU is the only CPU and interrupts are still disabled.
    unsafe {
        let mut old_gdtr = DescriptorTablePointer { limit: 0, base: 0 };
        asm!("sgdt [{}]", in(reg) &mut old_gdtr, options(nostack, preserves_flags));
        let old_limit = ptr::addr_of!(old_gdtr.limit).read_unaligned() as usize;
        let old_base = ptr::addr_of!(old_gdtr.base).read_unaligned() as usize;
        let old_length = old_limit + 1;
        if old_length == 0 || old_length % size_of::<u64>() != 0 {
            return false;
        }

        let descriptor_index = old_length / size_of::<u64>();
        if descriptor_index + 2 > GDT_CAPACITY {
            return false;
        }

        let gdt = ptr::addr_of_mut!(GDT).cast::<u64>();
        ptr::copy_nonoverlapping(old_base as *const u8, gdt.cast::<u8>(), old_length);

        let double_fault_top = stack_top(ptr::addr_of_mut!(DOUBLE_FAULT_STACK));
        let nmi_top = stack_top(ptr::addr_of_mut!(NMI_STACK));
        let machine_check_top = stack_top(ptr::addr_of_mut!(MACHINE_CHECK_STACK));
        let page_fault_top = stack_top(ptr::addr_of_mut!(PAGE_FAULT_STACK));

        ptr::addr_of_mut!(TSS).write(TaskStateSegment {
            reserved_1: 0,
            rsp: [0; 3],
            reserved_2: 0,
            ist: [
                double_fault_top,
                nmi_top,
                machine_check_top,
                page_fault_top,
                0,
                0,
                0,
            ],
            reserved_3: 0,
            reserved_4: 0,
            io_map_base: size_of::<TaskStateSegment>() as u16,
        });

        let tss_base = ptr::addr_of!(TSS) as u64;
        let tss_limit = (size_of::<TaskStateSegment>() - 1) as u64;
        let low = (tss_limit & 0xFFFF)
            | ((tss_base & 0x00FF_FFFF) << 16)
            | (GDT_TSS_TYPE << 40)
            | ((tss_limit & 0xF000) << 32)
            | (((tss_base >> 24) & 0xFF) << 56);
        let high = tss_base >> 32;
        gdt.add(descriptor_index).write(low);
        gdt.add(descriptor_index + 1).write(high);

        let new_gdtr = DescriptorTablePointer {
            limit: ((descriptor_index + 2) * size_of::<u64>() - 1) as u16,
            base: gdt as u64,
        };
        asm!("lgdt [{}]", in(reg) &new_gdtr, options(readonly, nostack, preserves_flags));

        let tss_selector = (descriptor_index * size_of::<u64>()) as u16;
        asm!("ltr {0:x}", in(reg) tss_selector, options(nostack, preserves_flags));
    }
    true
}

unsafe fn stack_top(stack: *mut EmergencyStack) -> u64 {
    // SAFETY: The stack is a static, correctly aligned object of fixed size.
    unsafe { (*stack).0.as_mut_ptr().add(EMERGENCY_STACK_SIZE) as u64 }
}

extern "x86-interrupt" fn timer_irq(_frame: InterruptStackFrame) {
    crate::pit::handle_interrupt();
    crate::pic::acknowledge_master_irq();
}

extern "x86-interrupt" fn keyboard_irq(_frame: InterruptStackFrame) {
    crate::keyboard::handle_interrupt();
    crate::pic::acknowledge_master_irq();
}

macro_rules! no_error_exception {
    ($handler:ident, $vector:expr, $name:literal) => {
        extern "x86-interrupt" fn $handler(frame: InterruptStackFrame) {
            fatal_exception($vector, $name, &frame, None, None)
        }
    };
}

macro_rules! error_exception {
    ($handler:ident, $vector:expr, $name:literal) => {
        extern "x86-interrupt" fn $handler(frame: InterruptStackFrame, error_code: u64) {
            fatal_exception($vector, $name, &frame, Some(error_code), None)
        }
    };
}

no_error_exception!(exception_0, 0, "Divide Error");
no_error_exception!(exception_1, 1, "Debug");
no_error_exception!(exception_2, 2, "Non-Maskable Interrupt");
no_error_exception!(exception_3, 3, "Breakpoint");
no_error_exception!(exception_4, 4, "Overflow");
no_error_exception!(exception_5, 5, "BOUND Range Exceeded");
no_error_exception!(exception_6, 6, "Invalid Opcode");
no_error_exception!(exception_7, 7, "Device Not Available");
error_exception!(exception_8, 8, "Double Fault");
no_error_exception!(exception_9, 9, "Coprocessor Segment Overrun");
error_exception!(exception_10, 10, "Invalid TSS");
error_exception!(exception_11, 11, "Segment Not Present");
error_exception!(exception_12, 12, "Stack-Segment Fault");
error_exception!(exception_13, 13, "General Protection Fault");
no_error_exception!(exception_15, 15, "Reserved Exception 15");
no_error_exception!(exception_16, 16, "x87 Floating-Point Exception");
error_exception!(exception_17, 17, "Alignment Check");
no_error_exception!(exception_18, 18, "Machine Check");
no_error_exception!(exception_19, 19, "SIMD Floating-Point Exception");
no_error_exception!(exception_20, 20, "Virtualisation Exception");
error_exception!(exception_21, 21, "Control Protection Exception");
no_error_exception!(exception_22, 22, "Reserved Exception 22");
no_error_exception!(exception_23, 23, "Reserved Exception 23");
no_error_exception!(exception_24, 24, "Reserved Exception 24");
no_error_exception!(exception_25, 25, "Reserved Exception 25");
no_error_exception!(exception_26, 26, "Reserved Exception 26");
no_error_exception!(exception_27, 27, "Reserved Exception 27");
no_error_exception!(exception_28, 28, "Hypervisor Injection Exception");
error_exception!(exception_29, 29, "VMM Communication Exception");
error_exception!(exception_30, 30, "Security Exception");
no_error_exception!(exception_31, 31, "Reserved Exception 31");

extern "x86-interrupt" fn exception_14(frame: InterruptStackFrame, error_code: u64) {
    let fault_address: u64;
    // SAFETY: CR2 contains the linear address that caused the page fault and
    // must be captured before any other code can itself fault.
    unsafe {
        asm!("mov {}, cr2", out(reg) fault_address, options(nomem, nostack, preserves_flags));
    }
    fatal_exception(
        14,
        "Page Fault",
        &frame,
        Some(error_code),
        Some(fault_address),
    )
}

extern "x86-interrupt" fn unexpected_interrupt(_frame: InterruptStackFrame) {
    fatal_exception(0xFFFF, "Unexpected interrupt vector", &_frame, None, None)
}

fn fatal_exception(
    vector: u16,
    name: &str,
    frame: &InterruptStackFrame,
    error_code: Option<u64>,
    fault_address: Option<u64>,
) -> ! {
    // Interrupt gates clear IF. CLI also documents and enforces the fatal path.
    unsafe {
        asm!("cli", options(nomem, nostack));
    }

    let _ = writeln!(crate::serial::Writer, "\nKANGA FATAL EXCEPTION");
    let _ = writeln!(crate::serial::Writer, "Exception: {name}");
    let _ = writeln!(crate::serial::Writer, "Vector: {}", vector);
    let _ = writeln!(
        crate::serial::Writer,
        "Instruction pointer: 0x{:016x}",
        frame.instruction_pointer
    );
    let _ = writeln!(
        crate::serial::Writer,
        "Code segment: 0x{:04x}",
        frame.code_segment
    );
    let _ = writeln!(
        crate::serial::Writer,
        "CPU flags: 0x{:016x}",
        frame.cpu_flags
    );

    if let Some(code) = error_code {
        let _ = writeln!(crate::serial::Writer, "Error code: 0x{code:016x}");
    }
    if let Some(address) = fault_address {
        let _ = writeln!(
            crate::serial::Writer,
            "Faulting linear address (CR2): 0x{address:016x}"
        );
        let _ = writeln!(
            crate::serial::Writer,
            "Page fault: present={} write={} user={} reserved={} instruction-fetch={}",
            code_bit(error_code, 0),
            code_bit(error_code, 1),
            code_bit(error_code, 2),
            code_bit(error_code, 3),
            code_bit(error_code, 4)
        );
    }

    let _ = writeln!(
        crate::serial::Writer,
        "Kanga halted: fatal CPU exceptions cannot safely return"
    );
    halt_forever()
}

fn code_bit(error_code: Option<u64>, bit: u8) -> bool {
    error_code.is_some_and(|code| code & (1_u64 << bit) != 0)
}

fn halt_forever() -> ! {
    loop {
        // SAFETY: Returning to the faulting context is unsafe. Keep interrupts
        // disabled and halt rather than risk repeated faults or corrupted state.
        unsafe {
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}
