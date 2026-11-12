//! Polled COM1 receive path for the Kanga serial shell.
//!
//! `serial::init` configures the UART during early boot. The foreground idle
//! loop drains the 16550 receive FIFO each time the 100 Hz timer wakes the CPU
//! from HLT, so no extra interrupt vector or PIC change is required.

use core::arch::asm;

const COM1: u16 = 0x3F8;
const LINE_STATUS: u16 = COM1 + 5;
const DATA_READY: u8 = 0x01;
const NO_UART: u8 = 0xFF;

/// Return the next received byte, or `None` when the FIFO is empty.
pub fn read_byte() -> Option<u8> {
    // SAFETY: COM1 is initialised during early boot. Reading the line-status
    // register has no side effects, and the data register is read only when
    // the UART reports a byte is waiting.
    unsafe {
        let status = inb(LINE_STATUS);
        // A floating bus reads as 0xFF; treat that as "no UART present".
        if status == NO_UART || status & DATA_READY == 0 {
            return None;
        }
        Some(inb(COM1))
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: The caller selects an intended x86 I/O port.
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
