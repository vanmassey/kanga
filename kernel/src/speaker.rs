//! PC speaker tone generation using PIT channel 2.

use core::arch::asm;

/// Plays a short 880 Hz tone. The PIT channel 0 timer provides the duration.
pub fn beep() {
    const PIT_CHANNEL_2: u16 = 0x42;
    const PIT_COMMAND: u16 = 0x43;
    const SPEAKER_CONTROL: u16 = 0x61;
    // PIT input clock is 1,193,182 Hz; calculate in u32 before narrowing.
    const DIVISOR: u16 = (1_193_182_u32 / 880) as u16;

    let start = crate::pit::ticks();
    // SAFETY: Configure PIT channel 2 and gate the conventional PC speaker.
    unsafe {
        outb(PIT_COMMAND, 0xB6);
        outb(PIT_CHANNEL_2, DIVISOR as u8);
        outb(PIT_CHANNEL_2, (DIVISOR >> 8) as u8);
        let control = inb(SPEAKER_CONTROL);
        if control & 0x03 != 0x03 {
            outb(SPEAKER_CONTROL, control | 0x03);
        }
    }

    // Wait for 25 IRQ0 ticks (approximately 250 ms). Interrupts remain enabled.
    while crate::pit::ticks().wrapping_sub(start) < 25 {
        // SAFETY: IRQ0 wakes the processor and advances the monotonic tick count.
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }

    // SAFETY: Disable the speaker gate without changing unrelated port bits.
    unsafe {
        let control = inb(SPEAKER_CONTROL);
        outb(SPEAKER_CONTROL, control & !0x03);
    }
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
}
