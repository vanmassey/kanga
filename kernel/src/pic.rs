//! Legacy dual 8259 programmable interrupt controller support.
use core::arch::asm;

const MASTER_COMMAND: u16 = 0x20;
const MASTER_DATA: u16 = 0x21;
const SLAVE_COMMAND: u16 = 0xA0;
const SLAVE_DATA: u16 = 0xA1;

pub fn initialise() {
    // SAFETY: These are the standard 8259 PIC initialisation ports.
    unsafe {
        write(MASTER_COMMAND, 0x11);
        wait();
        write(SLAVE_COMMAND, 0x11);
        wait();
        write(MASTER_DATA, 0x20);
        wait();
        write(SLAVE_DATA, 0x28);
        wait();
        write(MASTER_DATA, 0x04);
        wait();
        write(SLAVE_DATA, 0x02);
        wait();
        write(MASTER_DATA, 0x01);
        wait();
        write(SLAVE_DATA, 0x01);
        wait();
        // Keep all lines masked except IRQ1 (the PS/2 keyboard).
        write(MASTER_DATA, 0xFD);
        write(SLAVE_DATA, 0xFF);
    }
}

pub fn acknowledge_keyboard() {
    // SAFETY: Send end-of-interrupt to the master PIC for IRQ1.
    unsafe { write(MASTER_COMMAND, 0x20); }
}

unsafe fn wait() {
    unsafe { write(0x80, 0); }
}

unsafe fn write(port: u16, value: u8) {
    // SAFETY: The caller selects a standard PIC or I/O delay port.
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}
