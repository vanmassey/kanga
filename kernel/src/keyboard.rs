//! Polling PS/2 keyboard driver for the legacy Set 1 scancode set.
//!
//! This deliberately uses polling rather than interrupts: it gives the first
//! shell a small, testable input path while the IDT and interrupt controllers
//! are still future work.

use core::sync::atomic::{AtomicBool, Ordering};

const STATUS_PORT: u16 = 0x64;
const DATA_PORT: u16 = 0x60;
static SHIFT: AtomicBool = AtomicBool::new(false);
static CAPS_LOCK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Character(u8),
    Backspace,
    Enter,
}

pub fn poll_key() -> Option<Key> {
    // SAFETY: Reading the PS/2 status port is the standard controller polling
    // operation on x86 PCs. No memory is accessed by this instruction.
    let status = unsafe { inb(STATUS_PORT) };
    if status & 1 == 0 {
        return None;
    }

    // SAFETY: The output-buffer-full status bit was set immediately above.
    let scancode = unsafe { inb(DATA_PORT) };
    match scancode {
        0x2A | 0x36 => {
            SHIFT.store(true, Ordering::Relaxed);
            return None;
        }
        0xAA | 0xB6 => {
            SHIFT.store(false, Ordering::Relaxed);
            return None;
        }
        0x3A => {
            CAPS_LOCK.fetch_xor(true, Ordering::Relaxed);
            return None;
        }
        0xE0 | 0xE1 => return None,
        code if code & 0x80 != 0 => return None,
        _ => {}
    }

    let shifted = SHIFT.load(Ordering::Relaxed);
    let caps = CAPS_LOCK.load(Ordering::Relaxed);
    let character = match scancode {
        0x0E => return Some(Key::Backspace),
        0x1C => return Some(Key::Enter),
        0x02..=0x0B => {
            const NORMAL: &[u8] = b"1234567890";
            const UPPER: &[u8] = b"!@#$%^&*()";
            let index = (scancode - 0x02) as usize;
            shifted_char(shifted, UPPER[index], NORMAL[index])
        }
        0x10..=0x19 => letter_or_symbol(scancode, shifted, caps, b"qwertyuiop", b"QWERTYUIOP"),
        0x1E..=0x26 => letter_or_symbol(scancode, shifted, caps, b"asdfghjkl", b"ASDFGHJKL"),
        0x2C..=0x32 => letter_or_symbol(scancode, shifted, caps, b"zxcvbnm", b"ZXCVBNM"),
        0x39 => b' ',
        0x0C => shifted_char(shifted, b'_', b'-'),
        0x0D => shifted_char(shifted, b'+', b'='),
        0x1A => shifted_char(shifted, b'{', b'['),
        0x1B => shifted_char(shifted, b'}', b']'),
        0x27 => shifted_char(shifted, b':', b';'),
        0x28 => if shifted { b'"' } else { b'\'' },
        0x29 => shifted_char(shifted, b'~', b'`'),
        0x2B => shifted_char(shifted, b'|', b'\\'),
        0x33 => shifted_char(shifted, b'<', b','),
        0x34 => shifted_char(shifted, b'>', b'.'),
        0x35 => shifted_char(shifted, b'?', b'/'),
        _ => return None,
    };
    Some(Key::Character(character))
}

fn shifted_char(shifted: bool, shifted_value: u8, normal_value: u8) -> u8 {
    if shifted {
        shifted_value
    } else {
        normal_value
    }
}

fn letter_or_symbol(code: u8, shifted: bool, caps: bool, normal: &[u8], upper: &[u8]) -> u8 {
    let start = match code {
        0x10..=0x19 => 0x10,
        0x1E..=0x26 => 0x1E,
        _ => 0x2C,
    };
    let index = (code - start) as usize;
    if shifted ^ caps {
        upper[index]
    } else {
        normal[index]
    }
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: Caller selects one of the legacy PS/2 controller I/O ports.
    unsafe {
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
