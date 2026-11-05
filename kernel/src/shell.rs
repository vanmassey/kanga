//! Minimal interactive kernel shell, initially backed by polled PS/2 input.

use core::fmt::Write;

use crate::keyboard::{self, Key};

const LINE_CAPACITY: usize = 80;

pub fn run() -> ! {
    let _ = writeln!(crate::serial::Writer, "\nKanga interactive shell");
    let _ = writeln!(crate::serial::Writer, "Input: PS/2 keyboard (polling)");
    let _ = writeln!(crate::serial::Writer, "Type 'help' for available commands.\n");
    prompt();

    let mut line = [0_u8; LINE_CAPACITY];
    let mut length = 0_usize;

    loop {
        if let Some(key) = keyboard::poll_key() {
            match key {
                Key::Character(character) if length < LINE_CAPACITY => {
                    line[length] = character;
                    length += 1;
                    let mut writer = crate::serial::Writer;
                    let _ = writer.write_char(character as char);
                }
                Key::Character(_) => {}
                Key::Backspace if length > 0 => {
                    length -= 1;
                    let _ = write!(crate::serial::Writer, "\x08 \x08");
                }
                Key::Backspace => {}
                Key::Enter => {
                    let _ = writeln!(crate::serial::Writer, "");
                    execute(&line[..length]);
                    length = 0;
                    prompt();
                }
            }
        } else {
            core::hint::spin_loop();
        }
    }
}

fn prompt() {
    let _ = write!(crate::serial::Writer, "kanga> ");
}

fn execute(command: &[u8]) {
    let command = trim(command);
    match command {
        b"" => {}
        b"help" => {
            let _ = writeln!(crate::serial::Writer, "help     - show this command list");
            let _ = writeln!(crate::serial::Writer, "clear    - print a fresh terminal page");
            let _ = writeln!(crate::serial::Writer, "info     - show kernel and platform details");
            let _ = writeln!(crate::serial::Writer, "version  - show the Kanga version");
            let _ = writeln!(crate::serial::Writer, "mem      - explain current memory support");
            let _ = writeln!(crate::serial::Writer, "halt     - halt the processor");
        }
        b"clear" => {
            // ANSI clear-screen is useful in serial terminals; graphical
            // terminal rendering will replace this once a text console exists.
            let _ = write!(crate::serial::Writer, "\x1b[2J\x1b[H");
        }
        b"info" => {
            let _ = writeln!(crate::serial::Writer, "Kanga {} / x86_64", env!("CARGO_PKG_VERSION"));
            let _ = writeln!(crate::serial::Writer, "Bootloader: Limine");
            let _ = writeln!(crate::serial::Writer, "Kernel mode: freestanding, no_std");
            let _ = writeln!(crate::serial::Writer, "Input: legacy PS/2 keyboard, polling");
        }
        b"version" => {
            let _ = writeln!(crate::serial::Writer, "Kanga OS {}", env!("CARGO_PKG_VERSION"));
        }
        b"mem" => {
            let _ = writeln!(crate::serial::Writer, "Physical frame allocation: initial 4 KiB allocator");
            let _ = writeln!(crate::serial::Writer, "Frame reclamation, virtual memory and heap: not implemented");
        }
        b"halt" => {
            let _ = writeln!(crate::serial::Writer, "Halting Kanga.");
            loop {
                // SAFETY: The user explicitly requested a processor halt.
                unsafe { core::arch::asm!("cli; hlt", options(nomem, nostack)); }
            }
        }
        _ => {
            let _ = writeln!(crate::serial::Writer, "Unknown command. Type 'help'.");
        }
    }
}

fn trim(mut input: &[u8]) -> &[u8] {
    while input.first().is_some_and(u8::is_ascii_whitespace) {
        input = &input[1..];
    }
    while input.last().is_some_and(u8::is_ascii_whitespace) {
        input = &input[..input.len() - 1];
    }
    input
}
