//! Interactive kernel shell with serial and framebuffer output.

use core::fmt::{self, Write};

use crate::graphics::Console;
use crate::keyboard::{self, Key};

const LINE_CAPACITY: usize = 80;

struct Output<'a>(Option<&'a mut Console>);

impl Write for Output<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let _ = crate::serial::Writer.write_str(text);
        if let Some(console) = self.0.as_deref_mut() {
            console.write_str(text)?;
        }
        Ok(())
    }
}

pub fn run(mut console: Option<Console>) -> ! {
    {
        let mut output = Output(console.as_mut());
        let _ = writeln!(output, "\nKanga interactive shell");
        let _ = writeln!(output, "Input: PS/2 keyboard (interrupt-driven IRQ1)");
        let _ = writeln!(output, "Type 'help' for available commands.\n");
        let _ = write!(output, "kanga> ");
    }

    let mut line = [0_u8; LINE_CAPACITY];
    let mut length = 0_usize;

    loop {
        if let Some(key) = keyboard::poll_key() {
            match key {
                Key::Character(character) if length < LINE_CAPACITY => {
                    line[length] = character;
                    length += 1;
                    let mut output = Output(console.as_mut());
                    let _ = output.write_char(character as char);
                }
                Key::Character(_) => {}
                Key::Backspace if length > 0 => {
                    length -= 1;
                    let mut output = Output(console.as_mut());
                    let _ = output.write_str("\x08 \x08");
                }
                Key::Backspace => {}
                Key::Enter => {
                    {
                        let mut output = Output(console.as_mut());
                        let _ = writeln!(output);
                    }
                    execute(&line[..length], console.as_mut());
                    length = 0;
                    let mut output = Output(console.as_mut());
                    let _ = write!(output, "kanga> ");
                }
            }
        } else {
            core::hint::spin_loop();
        }
    }
}

fn execute(command: &[u8], console: Option<&mut Console>) {
    let command = trim(command);
    let mut output = Output(console);
    match command {
        b"" => {}
        b"help" => {
            let _ = writeln!(output, "help     - show this command list");
            let _ = writeln!(output, "clear    - clear the screen");
            let _ = writeln!(output, "info     - show kernel and platform details");
            let _ = writeln!(output, "version  - show the Kanga version");
            let _ = writeln!(output, "mem      - explain current memory support");
            let _ = writeln!(output, "halt     - halt the processor");
        }
        b"clear" => {
            if let Some(console) = output.0.as_deref_mut() {
                console.clear();
            }
        }
        b"info" => {
            let _ = writeln!(output, "Kanga {} / x86_64", env!("CARGO_PKG_VERSION"));
            let _ = writeln!(output, "Bootloader: Limine");
            let _ = writeln!(output, "Kernel mode: freestanding, no_std");
            let _ = writeln!(output, "Input: legacy PS/2 keyboard, interrupt-driven IRQ1");
            let _ = writeln!(output, "Display: framebuffer text console");
        }
        b"version" => {
            let _ = writeln!(output, "Kanga OS {}", env!("CARGO_PKG_VERSION"));
        }
        b"mem" => {
            let _ = writeln!(output, "Physical frame allocation: initial 4 KiB allocator");
            let _ = writeln!(
                output,
                "Frame reclamation, virtual memory and heap: not implemented"
            );
        }
        b"halt" => {
            let _ = writeln!(output, "Halting Kanga.");
            loop {
                // SAFETY: The user explicitly requested a processor halt.
                unsafe {
                    core::arch::asm!("cli; hlt", options(nomem, nostack));
                }
            }
        }
        _ => {
            let _ = writeln!(output, "Unknown command. Type 'help'.");
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
