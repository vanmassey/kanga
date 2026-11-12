//! Read-only PCI configuration-space enumeration for x86-64.
//!
//! This initial implementation uses PCI Configuration Mechanism #1. It scans
//! all buses and discovers functions without probing BARs or changing device
//! configuration. Enumeration runs only in ordinary kernel context, never in
//! an interrupt handler.

use core::arch::asm;

const CONFIG_ADDRESS: u16 = 0x0CF8;
const CONFIG_DATA: u16 = 0x0CFC;
const MAX_BUSES: u16 = 256;
const DEVICES_PER_BUS: u8 = 32;
const FUNCTIONS_PER_DEVICE: u8 = 8;

/// A single PCI function discovered in configuration space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Device {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub programming_interface: u8,
}

/// Enumerate every present PCI function and pass each descriptor to `visit`.
///
/// The scan checks function zero first and checks functions 1–7 only when the
/// multifunction bit is set in function zero's header type. All 256 bus
/// numbers are visited so devices behind PCI-to-PCI bridges are discoverable
/// without needing bridge-driver support first.
///
/// Returns the number of discovered functions. The callback must not perform
/// PCI configuration accesses recursively.
pub fn scan<F>(mut visit: F) -> usize
where
    F: FnMut(Device),
{
    let mut found = 0usize;

    for bus in 0..MAX_BUSES {
        for device in 0..DEVICES_PER_BUS {
            let function_zero_id = read_config(bus as u8, device, 0, 0x00);
            let vendor_zero = function_zero_id as u16;
            if vendor_zero == u16::MAX {
                continue;
            }

            if let Some(function) = read_function(bus as u8, device, 0, function_zero_id) {
                visit(function);
                found += 1;
            }

            let header = read_config(bus as u8, device, 0, 0x0C);
            let multifunction = ((header >> 16) as u8 & 0x80) != 0;
            if multifunction {
                for function_number in 1..FUNCTIONS_PER_DEVICE {
                    let id = read_config(bus as u8, device, function_number, 0x00);
                    if id as u16 == u16::MAX {
                        continue;
                    }
                    if let Some(function) = read_function(bus as u8, device, function_number, id) {
                        visit(function);
                        found += 1;
                    }
                }
            }
        }
    }

    found
}

fn read_function(bus: u8, device: u8, function: u8, id: u32) -> Option<Device> {
    let vendor_id = id as u16;
    if vendor_id == u16::MAX {
        return None;
    }

    let class = read_config(bus, device, function, 0x08);
    
    Some(Device {
        bus,
        device,
        function,
        vendor_id,
        device_id: (id >> 16) as u16,
        class_code: (class >> 24) as u8,
        subclass: (class >> 16) as u8,
        programming_interface: (class >> 8) as u8,
    })
}

/// Return a short human-readable name for the PCI base class.
pub fn class_name(class_code: u8) -> &'static str {
    match class_code {
        0x01 => "mass storage",
        0x02 => "network",
        0x03 => "display",
        0x04 => "multimedia",
        0x05 => "memory",
        0x06 => "bridge",
        0x07 => "communications",
        0x08 => "system peripheral",
        0x09 => "input",
        0x0A => "docking station",
        0x0B => "processor",
        0x0C => "serial bus",
        0x0D => "wireless",
        0x0E => "intelligent I/O",
        0x0F => "satellite",
        0x10 => "encryption",
        0x11 => "signal processing",
        0x12 => "processing accelerator",
        0x13 => "non-essential instrumentation",
        0x40 => "coprocessor",
        0xFF => "unassigned",
        _ => "other",
    }
}

fn read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    // SAFETY: PCI configuration mechanism #1 uses the conventional CF8/CFC
    // ports. The address selects a bounded bus/device/function and aligned
    // register offset; this code is called only from normal kernel context.
    unsafe {
        outl(CONFIG_ADDRESS, address);
        inl(CONFIG_DATA)
    }
}

unsafe fn outl(port: u16, value: u32) {
    // SAFETY: Caller supplies the standard PCI configuration address port.
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: Caller supplies the standard PCI configuration data port.
    unsafe {
        asm!(
            "in eax, dx",
            in("dx") port,
            out("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
