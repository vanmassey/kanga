//! x86_64 interrupt descriptor table and fatal CPU exception handlers.
//!
//! The table, legacy PIC and keyboard IRQ gate are installed before enabling IRQ1.

use core::arch::asm;
use core::fmt::Write;

#[derive(Clone, Copy)]
#[repr(C)]
struct InterruptStackFrame {
    instruction_pointer: u64,
    code_segment: u64,
    cpu_flags: u64,
    stack_pointer: u64,
    stack_segment: u64,
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

    fn new(handler: usize, selector: u16) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            options: 0x8E00,
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

static mut IDT: [IdtEntry; 256] = [IdtEntry::MISSING; 256];

/// Installs exception gates and loads the IDTR. IF remains disabled.
pub fn initialise() {
    let selector: u16;
    // SAFETY: Reading CS is a non-mutating architectural register operation.
    unsafe {
        asm!("mov {0:x}, cs", out(reg) selector, options(nomem, nostack, preserves_flags));
    }

    // SAFETY: This is the single boot-time writer of the static IDT, and the
    // processor has not enabled maskable interrupts at this point. Raw pointer
    // writes avoid creating references to mutable static storage.
    unsafe {
        let idt = core::ptr::addr_of_mut!(IDT).cast::<IdtEntry>();
        let default_handler = exception_without_error as *const () as usize;
        for index in 0..256 {
            idt.add(index).write(IdtEntry::new(default_handler, selector));
        }

        let error_handler = exception_with_error as *const () as usize;
        for index in [8, 10, 11, 12, 13, 14, 17, 21, 29, 30] {
            idt.add(index).write(IdtEntry::new(error_handler, selector));
        }
        idt.add(0x20).write(IdtEntry::new(timer_irq as *const () as usize, selector));
        idt.add(0x21).write(IdtEntry::new(keyboard_irq as *const () as usize, selector));

        let pointer = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: idt as u64,
        };
        asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags));
    }

    let _ = writeln!(crate::serial::Writer, "Interrupts: 256-entry IDT installed");
    let _ = writeln!(
        crate::serial::Writer,
        "Interrupts: CPU exception gates 0-31 configured"
    );
    crate::pic::initialise();
    let _ = writeln!(
        crate::serial::Writer,
        "Interrupts: PIC remapped; timer IRQ0 and keyboard IRQ1 unmasked"
    );
    crate::pit::initialise();
    let _ = writeln!(crate::serial::Writer, "Timer: PIT configured at 100 Hz");
    // SAFETY: The IDT, PIC and PIT are ready, and IRQ0/IRQ1 have valid handlers.
    unsafe {
        asm!("sti", options(nomem, nostack));
    }
    let _ = writeln!(crate::serial::Writer, "Interrupts: keyboard IRQ1 enabled");
}

extern "x86-interrupt" fn timer_irq(_frame: InterruptStackFrame) {
    crate::pit::handle_interrupt();
    crate::pic::acknowledge_master_irq();
}

extern "x86-interrupt" fn keyboard_irq(_frame: InterruptStackFrame) {
    crate::keyboard::handle_interrupt();
    crate::pic::acknowledge_master_irq();
}

extern "x86-interrupt" fn exception_without_error(_frame: InterruptStackFrame) {
    fatal_exception("CPU exception without error code", None)
}

extern "x86-interrupt" fn exception_with_error(_frame: InterruptStackFrame, error_code: u64) {
    fatal_exception("CPU exception with error code", Some(error_code))
}

fn fatal_exception(message: &str, error_code: Option<u64>) -> ! {
    let _ = writeln!(crate::serial::Writer, "\\nKANGA FATAL: {message}");
    if let Some(code) = error_code {
        let _ = writeln!(
            crate::serial::Writer,
            "CPU exception error code: 0x{code:016x}"
        );
    }

    loop {
        // SAFETY: A fatal CPU exception cannot safely return to the interrupted
        // context. Disable interrupts and halt the processor permanently.
        unsafe {
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}
