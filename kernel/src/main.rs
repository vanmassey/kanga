#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::fmt::Write;
use core::panic::PanicInfo;
use limine::memory_map::EntryType;
use limine::request::{HhdmRequest, MemoryMapRequest, RequestsEndMarker, RequestsStartMarker};
use limine::BaseRevision;

mod console;
mod filesystem;
mod heap;
mod interrupts;
mod memory;
mod mascot;
/// Public x86-64 virtual-memory interface for the kernel.
pub mod paging;
mod pic;
mod pit;
mod power;
mod rtc;
mod scheduler;
mod shell;

static mut FRAME_BITMAP: [u64; memory::BITMAP_WORDS] = [u64::MAX; memory::BITMAP_WORDS];

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
#[link_section = ".limine_requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[no_mangle]
pub extern "C" fn kanga_entry() -> ! {
    serial::init();
    let _ = writeln!(serial::Writer, "Kanga kernel entry reached");

    if !BASE_REVISION.is_supported() {
        let _ = writeln!(
            serial::Writer,
            "ERROR: Limine base revision is not supported"
        );
        stop();
    }

    let _ = writeln!(serial::Writer, "Kanga kernel {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(serial::Writer, "Architecture: x86_64");
    let _ = writeln!(serial::Writer, "Boot protocol: Limine");

    if mascot::draw() {
        let _ = writeln!(serial::Writer, "Framebuffer mascot: Kanga rendered and centred");
    } else {
        let _ = writeln!(serial::Writer, "Framebuffer mascot: unavailable (no compatible RGB 32-bit framebuffer)");
    }

    interrupts::initialise();
    match rtc::read() {
        Some(clock) => {
            let _ = writeln!(
                serial::Writer,
                "RTC: {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                clock.year,
                clock.month,
                clock.day,
                clock.hour,
                clock.minute,
                clock.second
            );
        }
        None => {
            let _ = writeln!(serial::Writer, "RTC: unavailable or invalid CMOS time");
        }
    }
    report_memory_map();
    if scheduler::initialise() {
        let _ = writeln!(
            serial::Writer,
            "Scheduler: pre-emptive round-robin enabled (3 kernel threads)"
        );
    } else {
        let _ = writeln!(
            serial::Writer,
            "Scheduler: unavailable; continuing on bootstrap thread"
        );
    }
    let _ = writeln!(serial::Writer, "Kernel initialisation complete");
    let _ = writeln!(serial::Writer, "Kernel idle loop reached");
    idle()
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

    let Some(hhdm) = HHDM_REQUEST.get_response() else {
        let _ = writeln!(serial::Writer, "HHDM: no response from bootloader");
        return;
    };
    let hhdm_offset = hhdm.offset();
    let _ = writeln!(serial::Writer, "HHDM offset: 0x{:016x}", hhdm_offset);

    // The bitmap is static kernel storage, separate from managed frames.
    // SAFETY: This is the sole initialisation site; the global allocator retains
    // the static bitmap and Limine memory-map entries for the kernel lifetime.
    let bitmap = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(FRAME_BITMAP).cast::<u64>(),
            memory::BITMAP_WORDS,
        )
    };
    if !memory::initialise_global(entries, bitmap) {
        let _ = writeln!(
            serial::Writer,
            "Physical frame allocator: global initialisation failed"
        );
        return;
    }

    if !paging::initialise(hhdm_offset) {
        let _ = writeln!(
            serial::Writer,
            "Virtual memory manager: initialisation failed"
        );
        return;
    }
    let _ = writeln!(
        serial::Writer,
        "Virtual memory manager: x86-64 four-level page tables ready"
    );

    let Some(()) = memory::with_global_allocator(|allocator| {
        let initial_free_frames = allocator.free_frame_count();
        let _ = writeln!(
            serial::Writer,
            "Physical bitmap allocator: {} frames ready",
            initial_free_frames
        );
        let _ = writeln!(
            serial::Writer,
            "Physical bitmap managed frames: {}",
            allocator.managed_frame_count()
        );
        let _ = writeln!(
            serial::Writer,
            "Physical bitmap capacity: {} frames ({} GiB)",
            allocator.capacity_frames(),
            (allocator.capacity_frames() as u64 * memory::PAGE_SIZE) / (1024 * 1024 * 1024)
        );
        let _ = writeln!(
            serial::Writer,
            "Usable frames above bitmap limit: {}",
            allocator.ignored_usable_frame_count()
        );

        let Some(frame_one) = allocator.allocate_frame() else {
            let _ = writeln!(serial::Writer, "Physical frame allocator: no usable frames");
            return;
        };
        let Some(frame_two) = allocator.allocate_frame() else {
            let _ = writeln!(
                serial::Writer,
                "Physical frame allocator: second allocation failed"
            );
            return;
        };
        let Some(frame_three) = allocator.allocate_frame() else {
            let _ = writeln!(
                serial::Writer,
                "Physical frame allocator: third allocation failed"
            );
            return;
        };

        let _ = writeln!(
            serial::Writer,
            "Physical frame allocator: allocated frame at 0x{:016x}",
            frame_one
        );
        let unique = frame_one != frame_two && frame_one != frame_three && frame_two != frame_three;
        let _ = writeln!(
            serial::Writer,
            "Frame allocation uniqueness test: {}",
            if unique { "passed" } else { "FAILED" }
        );

        match memory::physical_to_virtual(frame_one, hhdm_offset) {
            Some(virtual_address) => {
                let _ = writeln!(
                    serial::Writer,
                    "Allocated frame virtual address: 0x{:016x}",
                    virtual_address
                );
                // SAFETY: frame_one is exclusively owned by this diagnostic.
                let accessible = unsafe { memory::zero_and_verify_frame(virtual_address) };
                let _ = writeln!(
                    serial::Writer,
                    "Frame memory test: {}",
                    if accessible { "passed" } else { "FAILED" }
                );
            }
            None => {
                let _ = writeln!(serial::Writer, "HHDM address conversion overflowed");
            }
        }

        // SAFETY: frame_two is allocated and unused elsewhere.
        let returned = unsafe { allocator.deallocate_frame(frame_two) };
        // SAFETY: This deliberately verifies double-free rejection.
        let double_free_rejected = !unsafe { allocator.deallocate_frame(frame_two) };
        let reused = allocator.allocate_frame();
        let reuse_passed = returned && double_free_rejected && reused == Some(frame_two);
        let _ = writeln!(
            serial::Writer,
            "Frame deallocation, double-free guard and reuse test: {}",
            if reuse_passed { "passed" } else { "FAILED" }
        );

        let contiguous = allocator.allocate_contiguous(3);
        let contiguous_passed = contiguous.is_some_and(|first| {
            let second = first.checked_add(memory::PAGE_SIZE);
            let third = first.checked_add(memory::PAGE_SIZE * 2);
            second.is_some()
                && third.is_some()
                && second.unwrap() - first == memory::PAGE_SIZE
                && third.unwrap() - second.unwrap() == memory::PAGE_SIZE
        });
        let _ = writeln!(
            serial::Writer,
            "Bitmap contiguous allocation test: {}",
            if contiguous_passed {
                "passed"
            } else {
                "FAILED"
            }
        );

        let returned_run = if let Some(first) = contiguous {
            // SAFETY: These three frames belong to the allocated contiguous run.
            let a = unsafe { allocator.deallocate_frame(first) };
            // SAFETY: The second frame belongs to that run.
            let b = unsafe { allocator.deallocate_frame(first + memory::PAGE_SIZE) };
            // SAFETY: The third frame belongs to that run.
            let c = unsafe { allocator.deallocate_frame(first + memory::PAGE_SIZE * 2) };
            a && b && c
        } else {
            false
        };
        // SAFETY: frame_one and frame_three remain allocated by this diagnostic.
        let returned_one = unsafe { allocator.deallocate_frame(frame_one) };
        // SAFETY: frame_three remains allocated by this diagnostic.
        let returned_three = unsafe { allocator.deallocate_frame(frame_three) };
        let returned_reused = reused.is_some_and(|frame| {
            // SAFETY: The reused frame was allocated above.
            unsafe { allocator.deallocate_frame(frame) }
        });
        let integrity =
            returned_one && returned_three && returned_reused && returned_run && contiguous_passed;
        let _ = writeln!(
            serial::Writer,
            "Frame allocator integrity test: {}",
            if integrity { "passed" } else { "FAILED" }
        );
        let _ = writeln!(
            serial::Writer,
            "Physical bitmap allocator: {} frames available",
            allocator.free_frame_count()
        );
    }) else {
        let _ = writeln!(
            serial::Writer,
            "Physical frame allocator: global service unavailable"
        );
        return;
    };

    let paging_ok = paging::self_test();
    let _ = writeln!(
        serial::Writer,
        "Virtual mapping, translation and unmapping test: {}",
        if paging_ok { "passed" } else { "FAILED" }
    );
    let heap_ok = heap::initialise();
    let _ = writeln!(
        serial::Writer,
        "Kernel heap initialisation: {}",
        if heap_ok { "passed" } else { "FAILED" }
    );
    if heap_ok {
        let heap_test = heap::self_test();
        let _ = writeln!(
            serial::Writer,
            "Kernel heap allocation and reclamation test: {}",
            if heap_test { "passed" } else { "FAILED" }
        );
    }
}

fn idle() -> ! {
    let mut scheduler_reported = false;
    let mut shell_started = false;
    let mut shell = shell::Shell::new();
    loop {
        // Keep interrupts enabled so the timer and scheduler continue running.
        // SAFETY: HLT is valid in the privileged kernel context.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }

        if !scheduler_reported && scheduler::runtime_self_test() {
            let _ = writeln!(
                serial::Writer,
                "Scheduler runtime test: passed (timer pre-emption and both workers observed)"
            );
            let elapsed = pit::elapsed_tenths();
            let (current, switches, worker_a_ticks, worker_b_ticks) = scheduler::statistics();
            let _ = writeln!(
                serial::Writer,
                "Uptime: {}.{:01} seconds",
                elapsed / 10,
                elapsed % 10
            );
            let _ = writeln!(
                serial::Writer,
                "Scheduler: current task {}, {} context switches",
                current,
                switches
            );
            let _ = writeln!(
                serial::Writer,
                "Scheduler workers: A={} ticks, B={} ticks",
                worker_a_ticks,
                worker_b_ticks
            );
            for index in 0..3 {
                if let Some((name, running)) = scheduler::task_info(index) {
                    let _ = writeln!(
                        serial::Writer,
                        "Task {}: {}{}",
                        index,
                        name,
                        if running { " (running)" } else { "" }
                    );
                }
            }
            scheduler_reported = true;
        }

        // Start the shell once the scheduler report is out, or after one
        // second if the scheduler never came up.
        if !shell_started && (scheduler_reported || pit::ticks() >= 100) {
            shell.start();
            shell_started = true;
        }
        if shell_started {
            shell.poll();
        }
    }
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

pub(crate) mod serial {
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

    pub(crate) struct Writer;

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