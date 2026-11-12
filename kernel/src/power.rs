//! Machine reset, halt and power-off helpers for the shell.

use core::arch::asm;

const KEYBOARD_CONTROLLER: u16 = 0x64;
const CONTROLLER_RESET: u8 = 0xFE;
const INPUT_BUFFER_FULL: u8 = 0x02;
const QEMU_ACPI_PM1A: u16 = 0x604;
const BOCHS_SHUTDOWN: u16 = 0xB004;
const ACPI_SLEEP_S5: u16 = 0x2000;

/// Stop the CPU permanently with interrupts disabled.
pub fn halt() -> ! {
    loop {
        // SAFETY: Kanga runs in the privileged kernel context.
        unsafe {
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

/// Pulse the CPU reset line through the 8042 keyboard controller.
///
/// Under `scripts/run-qemu.sh` (`-no-reboot`) this makes QEMU exit.
pub fn reboot() -> ! {
    // SAFETY: 0xFE on the 8042 command port is the standard legacy reset.
    unsafe {
        for _ in 0..100_000 {
            if inb(KEYBOARD_CONTROLLER) & INPUT_BUFFER_FULL == 0 {
                break;
            }
        }
        outb(KEYBOARD_CONTROLLER, CONTROLLER_RESET);
    }
    halt()
}

/// Request an ACPI S5 shutdown using the well-known emulator PM1a ports.
///
/// Real hardware needs ACPI table parsing, which Kanga does not have yet; if
/// the ports are ignored this falls back to halting the CPU.
pub fn power_off() -> ! {
    // SAFETY: These ports are the QEMU/Bochs shutdown registers. Writes to
    // them are ignored by hardware that does not implement them.
    unsafe {
        outw(QEMU_ACPI_PM1A, ACPI_SLEEP_S5);
        outw(BOCHS_SHUTDOWN, ACPI_SLEEP_S5);
    }
    halt()
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

unsafe fn outb(port: u16, value: u8) {
    // SAFETY: The caller selects an intended x86 I/O port.
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn outw(port: u16, value: u16) {
    // SAFETY: The caller selects an intended x86 I/O port.
    unsafe {
        asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}
