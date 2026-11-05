#![no_std]
#![no_main]

use core::fmt::Write;
use core::panic::PanicInfo;
use limine::request::{RequestsEndMarker, RequestsStartMarker};
use limine::BaseRevision;

#[used]
#[link_section = ".limine_requests_start"]
static REQUESTS_START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[no_mangle]
pub extern "C" fn kanga_entry() -> ! {
    if !BASE_REVISION.is_supported() {
        stop();
    }

    serial::init();
    let _ = writeln!(serial::Writer, "");
    let _ = writeln!(serial::Writer, "Kanga kernel {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(serial::Writer, "Architecture: x86_64");
    let _ = writeln!(serial::Writer, "Boot protocol: Limine");
    let _ = writeln!(serial::Writer, "Kernel entry reached successfully");
    let _ = writeln!(serial::Writer, "System halted: next milestone is boot information and memory map");

    stop()
}

fn stop() -> ! {
    loop {
        // SAFETY: Kanga runs in the privileged kernel context.
        unsafe { core::arch::asm!("cli; hlt", options(nomem, nostack)); }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    let _ = writeln!(serial::Writer, "\nKANGA PANIC: {info}");
    stop()
}

mod serial {
    use super::*;

    const COM1: u16 = 0x3F8;

    pub fn init() {
        // SAFETY: These writes configure the conventional COM1 UART.
        unsafe {
            out(COM1 + 1, 0x00);
            out(COM1 + 3, 0x80);
            out(COM1, 0x03);
            out(COM1 + 1, 0x00);
            out(COM1 + 3, 0x03);
            out(COM1 + 2, 0xC7);
            out(COM1 + 4, 0x0B);
        }
    }

    unsafe fn out(port: u16, value: u8) {
        // SAFETY: The caller selects an intended x86 I/O port.
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    unsafe fn input(port: u16) -> u8 {
        let value: u8;
        // SAFETY: The caller selects an intended x86 I/O port.
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

    pub struct Writer;

    impl Writer {
        fn byte(&mut self, byte: u8) {
            // A bounded wait prevents a missing UART from hanging the kernel.
            for _ in 0..100_000 {
                // SAFETY: COM1 line-status register is readable after UART setup.
                if unsafe { input(COM1 + 5) } & 0x20 != 0 {
                    // SAFETY: COM1 data register is the UART transmit port.
                    unsafe { out(COM1, byte); }
                    return;
                }
                core::hint::spin_loop();
            }
        }
    }

    impl Write for Writer {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            for byte in text.bytes() {
                if byte == b'\n' {
                    self.byte(b'\r');
                }
                self.byte(byte);
            }
            Ok(())
        }
    }
}
