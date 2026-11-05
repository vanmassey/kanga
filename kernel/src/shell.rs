//! Interactive kernel shell with DOS-style commands and serial/framebuffer output.

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
        let _ = writeln!(output, "Kanga OS 0.1.0");
        let _ = writeln!(output, "(C) 2026 Kanga Development Team");
        let _ = writeln!(output);
        let _ = writeln!(output, "Initialising kernel........ [ OK ]");
        let _ = writeln!(output, "Setting up interrupts...... [ OK ]");
        let _ = writeln!(output, "Initialising keyboard...... [ OK ]");
        let _ = writeln!(output, "Starting command shell..... [ OK ]");
        let _ = writeln!(output);
        let _ = writeln!(output, "Type HELP for a list of commands.");
        let _ = writeln!(output);
        let _ = write!(output, "Kanga> ");
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
                    let _ = write!(output, "Kanga> ");
                }
            }
        } else {
            // SAFETY: STI followed by HLT avoids a lost-wakeup race: pending
            // IRQs are recognised immediately after the halt instruction.
            unsafe {
                core::arch::asm!("sti; hlt", options(nomem, nostack));
            }
        }
    }
}

fn execute(command_line: &[u8], console: Option<&mut Console>) {
    let command_line = trim(command_line);
    let split = command_line
        .iter()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(command_line.len());
    let command = &command_line[..split];
    let arguments = trim(&command_line[split..]);
    let mut output = Output(console);

    if command.is_empty() {
        return;
    }

    if command.eq_ignore_ascii_case(b"help") || command == b"/?" {
        let _ = writeln!(output, "Kanga DOS-style command shell");
        let _ = writeln!(
            output,
            "DIR       list files (requires a mounted filesystem)"
        );
        let _ = writeln!(output, "CLS       clear the screen");
        let _ = writeln!(output, "VER       display the Kanga version");
        let _ = writeln!(output, "ECHO      display a message");
        let _ = writeln!(output, "INFO      show kernel and platform details");
        let _ = writeln!(output, "MEM       show memory support status");
        let _ = writeln!(output, "TIME      show system uptime and timer ticks");
        let _ = writeln!(output, "DATE      show the CMOS real-time clock");
        let _ = writeln!(output, "BEEP      play a short PC speaker tone");
        let _ = writeln!(output, "UPTIME    show system uptime");
        let _ = writeln!(output, "ABOUT     show project information");
        let _ = writeln!(output, "HELP      display this help");
        let _ = writeln!(output, "HALT      stop the processor");
    } else if command.eq_ignore_ascii_case(b"dir") {
        let _ = writeln!(output, " Directory of KANGA:\\");
        let _ = writeln!(output);
        let _ = writeln!(output, "No filesystem is mounted.");
        let _ = writeln!(
            output,
            "Disk and directory support are not implemented yet."
        );
    } else if command.eq_ignore_ascii_case(b"cls") || command.eq_ignore_ascii_case(b"clear") {
        if let Some(console) = output.0.as_deref_mut() {
            console.clear();
        }
    } else if command.eq_ignore_ascii_case(b"ver") || command.eq_ignore_ascii_case(b"version") {
        let _ = writeln!(output, "Kanga OS [Version {}]", env!("CARGO_PKG_VERSION"));
    } else if command.eq_ignore_ascii_case(b"echo") {
        if arguments.is_empty() {
            let _ = writeln!(output);
        } else {
            let message = core::str::from_utf8(arguments).unwrap_or("[invalid text]");
            let _ = writeln!(output, "{message}");
        }
    } else if command.eq_ignore_ascii_case(b"info") {
        let _ = writeln!(output, "Kanga {} / x86_64", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(output, "Bootloader: Limine");
        let _ = writeln!(output, "Kernel mode: freestanding, no_std");
        let _ = writeln!(output, "Input: legacy PS/2 keyboard, interrupt-driven IRQ1");
        let _ = writeln!(output, "Display: monochrome framebuffer text console");
    } else if command.eq_ignore_ascii_case(b"time") {
        let ticks = crate::pit::ticks();
        let tenths = crate::pit::elapsed_tenths();
        let _ = writeln!(
            output,
            "System uptime: {}.{:01} seconds",
            tenths / 10,
            tenths % 10
        );
        let _ = writeln!(output, "PIT timer ticks: {ticks} (100 Hz)");
    } else if command.eq_ignore_ascii_case(b"date") || command.eq_ignore_ascii_case(b"clock") || command.eq_ignore_ascii_case(b"rtc") {
        match crate::rtc::read() {
            Some(time) => {
                let _ = writeln!(
                    output,
                    "RTC date/time: {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    time.year, time.month, time.day, time.hour, time.minute, time.second
                );
            }
            None => {
                let _ = writeln!(output, "RTC unavailable or returned an invalid date/time.");
            }
        }
    } else if command.eq_ignore_ascii_case(b"uptime") {
        let tenths = crate::pit::elapsed_tenths();
        let _ = writeln!(output, "Uptime: {}.{:01} seconds", tenths / 10, tenths % 10);
    } else if command.eq_ignore_ascii_case(b"beep") {
        crate::speaker::beep();
        let _ = writeln!(output, "PC speaker: 880 Hz tone played.");
    } else if command.eq_ignore_ascii_case(b"about") {
        let _ = writeln!(output, "Kanga is an independent x86-64 operating system.");
        let _ = writeln!(output, "Written in Rust; booted with Limine.");
        let _ = writeln!(output, "Project version: {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(output, "License: GPL-2.0-only");
    } else if command.eq_ignore_ascii_case(b"mem") {
        let _ = writeln!(output, "Physical frame allocation: initial 4 KiB allocator");
        let _ = writeln!(
            output,
            "Frame reclamation, virtual memory and heap: not implemented"
        );
    } else if command.eq_ignore_ascii_case(b"halt") {
        let _ = writeln!(output, "Halting Kanga.");
        loop {
            // SAFETY: The user explicitly requested a processor halt.
            unsafe {
                core::arch::asm!("cli; hlt", options(nomem, nostack));
            }
        }
    } else {
        let text = core::str::from_utf8(command).unwrap_or("[invalid command]");
        let _ = writeln!(output, "Bad command or file name: {text}");
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
