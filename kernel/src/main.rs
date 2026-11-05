#![no_std]
#![no_main]

use core::fmt::Write;
use core::panic::PanicInfo;
use limine::memory_map::EntryType;
use limine::request::{MemoryMapRequest, RequestsEndMarker, RequestsStartMarker};
use limine::BaseRevision;

#[used]
#[link_section = ".limine_requests_start"]
static REQUESTS_START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".limine_requests"]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[no_mangle]
pub extern "C" fn kanga_entry() -> ! {
    serial::init();
    let _ = writeln!(serial::Writer, "Kanga kernel entry reached");

    if !BASE_REVISION.is_supported() {
        let _ = writeln!(serial::Writer, "ERROR: Limine base revision is not supported");
        stop();
    }

    let _ = writeln!(serial::Writer, "Kanga kernel {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(serial::Writer, "Architecture: x86_64");
    let _ = writeln!(serial::Writer, "Boot protocol: Limine");

    report_memory_map();
    let _ = writeln!(serial::Writer, "Initialisation complete; kernel idle");
    stop()
}

fn report_memory_map() {
    let Some(memory_map) = MEMORY_MAP_REQUEST.get_response() else {
        let _ = writeln!(serial::Writer, "Memory map: no response from bootloader");
        return;
    };

    let entries = memory_map.entries();
    let mut usable_bytes = 0_u64;
    let mut usable_regions = 0_usize;

    for entry in entries {
        if entry.entry_type == EntryType::USABLE {
            usable_bytes = usable_bytes.saturating_add(entry.length);
            usable_regions += 1;
        }
    }

    let _ = writeln!(serial::Writer, "Memory map entries: {}", entries.len());
    let _ = writeln!(serial::Writer, "Usable memory regions: {}", usable_regions);
    let _ = writeln!(
        serial::Writer,
        "Usable memory: {} MiB",
        usable_bytes / (1024 * 1024)
    );
}

fn stop() -> ! {
    loop {
        // SAFETY: Kanga runs in the privileged kernel context.
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
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
                    unsafe {
                        out(COM1, byte);
                    }
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
