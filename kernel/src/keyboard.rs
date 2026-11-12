//! Interrupt-driven PS/2 keyboard input (set-1 scancodes).
//!
//! The IRQ handler only queues raw bytes. Scancode translation and shell
//! editing happen later on the bootstrap thread.

use core::arch::asm;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const QUEUE_CAPACITY: usize = 256;

static QUEUE: [AtomicU8; QUEUE_CAPACITY] =
    [const { AtomicU8::new(0) }; QUEUE_CAPACITY];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

struct Translator {
    shift: bool,
    control: bool,
    caps_lock: bool,
    extended: bool,
    pending: [u8; 3],
    pending_index: usize,
    pending_length: usize,
}

static mut TRANSLATOR: Translator = Translator {
    shift: false,
    control: false,
    caps_lock: false,
    extended: false,
    pending: [0; 3],
    pending_index: 0,
    pending_length: 0,
};

/// Drain stale controller bytes before enabling IRQ1.
pub fn initialise() {
    for _ in 0..32 {
        // SAFETY: Reading the PS/2 status and data ports is standard controller I/O.
        let status = unsafe { inb(STATUS_PORT) };
        if status & 1 == 0 {
            break;
        }
        let _ = unsafe { inb(DATA_PORT) };
    }
}

/// Called from the IRQ1 gate. It does no allocation and never formats output.
pub fn handle_interrupt() {
    // SAFETY: IRQ1 indicates that the controller has a byte available.
    let scancode = unsafe { inb(DATA_PORT) };
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % QUEUE_CAPACITY;
    if next != TAIL.load(Ordering::Acquire) {
        QUEUE[head].store(scancode, Ordering::Relaxed);
        HEAD.store(next, Ordering::Release);
    }
    crate::pic::acknowledge_master_irq();
}

/// Return the next translated ASCII byte, including ANSI arrow sequences.
pub fn read_byte() -> Option<u8> {
    // SAFETY: Only the bootstrap shell thread calls this translator.
    unsafe { (*core::ptr::addr_of_mut!(TRANSLATOR)).read_byte() }
}

impl Translator {
    fn read_byte(&mut self) -> Option<u8> {
        if self.pending_index < self.pending_length {
            let byte = self.pending[self.pending_index];
            self.pending_index += 1;
            return Some(byte);
        }
        self.pending_index = 0;
        self.pending_length = 0;

        while let Some(scancode) = pop_scancode() {
            if scancode == 0xE0 {
                self.extended = true;
                continue;
            }
            let extended = core::mem::replace(&mut self.extended, false);
            let released = scancode & 0x80 != 0;
            let code = scancode & 0x7F;

            if !extended {
                match code {
                    0x2A | 0x36 => { self.shift = !released; continue; }
                    0x1D => { self.control = !released; continue; }
                    0x3A if !released => { self.caps_lock = !self.caps_lock; continue; }
                    _ => {}
                }
            }
            if released {
                continue;
            }

            if extended {
                let arrow = match code {
                    0x48 => b'A',
                    0x50 => b'B',
                    0x4D => b'C',
                    0x4B => b'D',
                    _ => continue,
                };
                self.pending = [0x1B, b'[', arrow];
                self.pending_index = 1;
                self.pending_length = 3;
                return Some(0x1B);
            }

            let Some((normal, shifted, letter)) = key_character(code) else {
                continue;
            };
            let use_shift = if letter {
                self.shift ^ self.caps_lock
            } else {
                self.shift
            };
            let mut byte = if use_shift { shifted } else { normal };
            if self.control && byte.is_ascii_alphabetic() {
                byte = byte.to_ascii_uppercase() - b'@';
            }
            return Some(byte);
        }
        None
    }
}

fn pop_scancode() -> Option<u8> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let byte = QUEUE[tail].load(Ordering::Relaxed);
    TAIL.store((tail + 1) % QUEUE_CAPACITY, Ordering::Release);
    Some(byte)
}

fn key_character(code: u8) -> Option<(u8, u8, bool)> {
    let pair = match code {
        0x02 => (b'1', b'!', false), 0x03 => (b'2', b'@', false),
        0x04 => (b'3', b'#', false), 0x05 => (b'4', b'$', false),
        0x06 => (b'5', b'%', false), 0x07 => (b'6', b'^', false),
        0x08 => (b'7', b'&', false), 0x09 => (b'8', b'*', false),
        0x0A => (b'9', b'(', false), 0x0B => (b'0', b')', false),
        0x0C => (b'-', b'_', false), 0x0D => (b'=', b'+', false),
        0x0E => return Some((0x08, 0x08, false)),
        0x0F => return Some((b'\t', b'\t', false)),
        0x10 => (b'q', b'Q', true), 0x11 => (b'w', b'W', true),
        0x12 => (b'e', b'E', true), 0x13 => (b'r', b'R', true),
        0x14 => (b't', b'T', true), 0x15 => (b'y', b'Y', true),
        0x16 => (b'u', b'U', true), 0x17 => (b'i', b'I', true),
        0x18 => (b'o', b'O', true), 0x19 => (b'p', b'P', true),
        0x1A => (b'[', b'{', false), 0x1B => (b']', b'}', false),
        0x1C => return Some((b'\r', b'\r', false)),
        0x1E => (b'a', b'A', true), 0x1F => (b's', b'S', true),
        0x20 => (b'd', b'D', true), 0x21 => (b'f', b'F', true),
        0x22 => (b'g', b'G', true), 0x23 => (b'h', b'H', true),
        0x24 => (b'j', b'J', true), 0x25 => (b'k', b'K', true),
        0x26 => (b'l', b'L', true), 0x27 => (b';', b':', false),
        0x28 => (b'\'', b'"', false), 0x29 => (b'`', b'~', false),
        0x2B => (b'\\', b'|', false),
        0x2C => (b'z', b'Z', true), 0x2D => (b'x', b'X', true),
        0x2E => (b'c', b'C', true), 0x2F => (b'v', b'V', true),
        0x30 => (b'b', b'B', true), 0x31 => (b'n', b'N', true),
        0x32 => (b'm', b'M', true), 0x33 => (b',', b'<', false),
        0x34 => (b'.', b'>', false), 0x35 => (b'/', b'?', false),
        0x39 => (b' ', b' ', false),
        _ => return None,
    };
    Some(pair)
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: The caller selects one of the standard PS/2 controller ports.
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
