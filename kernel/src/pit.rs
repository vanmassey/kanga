//! Programmable Interval Timer (PIT) at 100 Hz.
//!
//! IRQ0 only increments an atomic tick counter. Formatting and time queries
//! happen in the foreground, never in interrupt context.

use core::sync::atomic::{AtomicU64, Ordering};

const PIT_INPUT_HZ: u32 = 1_193_182;
const TICK_HZ: u32 = 100;
const COMMAND_PORT: u16 = 0x43;
const CHANNEL_0_PORT: u16 = 0x40;

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn initialise() {
    let divisor = (PIT_INPUT_HZ / TICK_HZ) as u16;
    // Channel 0, low byte then high byte, rate generator, binary mode.
    // SAFETY: These are the standard legacy PIT programming ports.
    unsafe {
        outb(COMMAND_PORT, 0x34);
        outb(CHANNEL_0_PORT, divisor as u8);
        outb(CHANNEL_0_PORT, (divisor >> 8) as u8);
    }
}

pub fn handle_interrupt() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn elapsed_tenths() -> u64 {
    ticks() / (TICK_HZ as u64 / 10)
}

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: Caller selects the standard PIT command/data port.
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}
