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
    // processor has not enabled maskable interrupts at this point.
    unsafe {
        for entry in &mut IDT {
            *entry = IdtEntry::new(exception_without_error as usize, selector);
        }

        IDT[8] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[10] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[11] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[12] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[13] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[14] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[17] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[21] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[29] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[30] = IdtEntry::new(exception_with_error as usize, selector);
        IDT[0x20] = IdtEntry::new(timer_irq as usize, selector);
        IDT[0x21] = IdtEntry::new(keyboard_irq as usize, selector);

        let pointer = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u64,
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
    let _ = writeln!(crate::serial::Writer, "DEBUG: keyboard IRQ1 received");
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
    let _ = writeln!(crate::serial::Writer, "\nKANGA FATAL: {message}");
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
