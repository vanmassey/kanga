//! Interactive kernel shell with DOS-style commands and serial/framebuffer output.

use core::fmt::{self, Write};

use crate::filesystem::{self, FileSystem};
use crate::graphics::Console;
use crate::keyboard::{self, Key};

const LINE_CAPACITY: usize = 256;

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
        let width = console
            .as_ref()
            .map_or(80, |display| display.columns().clamp(64, 160));
        let mut output = Output(console.as_mut());
        draw_rule(&mut output, width);
        draw_centered(&mut output, width, "\x1b[1;36mKANGA OS\x1b[0m");
        draw_centered(&mut output, width, "SYSTEM CONSOLE");
        draw_rule(&mut output, width);
        draw_panel_line(
            &mut output,
            width,
            "Kanga OS  |  x86_64  |  Limine boot protocol",
        );
        draw_panel_line(&mut output, width, "(C) 2026 Kanga Development Team");
        draw_panel_line(
            &mut output,
            width,
            "This program comes with ABSOLUTELY NO WARRANTY.",
        );
        draw_panel_line(
            &mut output,
            width,
            "You may redistribute it under the GNU GPL version 2.",
        );
        draw_panel_line(
            &mut output,
            width,
            "License: https://github.com/vanmassey/kanga/blob/main/LICENSE",
        );
        draw_rule(&mut output, width);
        draw_panel_line(&mut output, width, "KERNEL STARTUP");
        draw_status(&mut output, width, "Kernel core", "READY");
        draw_status(&mut output, width, "Interrupt controller", "READY");
        draw_status(&mut output, width, "PS/2 keyboard", "READY");
        draw_status(&mut output, width, "KangaFS RAM filesystem", "READY");
        draw_status(&mut output, width, "Command interpreter", "READY");
        draw_rule(&mut output, width);
        draw_panel_line(&mut output, width, "COMMAND INTERFACE");
        draw_panel_line(&mut output, width, "Type HELP to browse commands.");
        draw_panel_line(
            &mut output,
            width,
            "Use the Up and Down arrows to navigate scrollback.",
        );
        draw_rule(&mut output, width);
        let _ = write!(output, "\x1b[1;36mKanga> \x1b[0m");
    }

    let mut line = [0_u8; LINE_CAPACITY];
    let mut length = 0_usize;
    let mut filesystem = FileSystem::new();

    loop {
        if let Some(key) = keyboard::poll_key() {
            match key {
                Key::Character(character) if length < LINE_CAPACITY => {
                    if let Some(console) = console.as_mut() {
                        console.follow_live();
                    }
                    line[length] = character;
                    length += 1;
                    let mut output = Output(console.as_mut());
                    let _ = output.write_char(character as char);
                }
                Key::Character(_) => {}
                Key::Backspace if length > 0 => {
                    if let Some(console) = console.as_mut() {
                        console.follow_live();
                    }
                    length -= 1;
                    let mut output = Output(console.as_mut());
                    let _ = output.write_str("\x08 \x08");
                }
                Key::Backspace => {}
                Key::Enter => {
                    if let Some(console) = console.as_mut() {
                        console.follow_live();
                    }
                    {
                        let mut output = Output(console.as_mut());
                        let _ = writeln!(output);
                    }
                    execute(&line[..length], console.as_mut(), &mut filesystem);
                    length = 0;
                    let mut output = Output(console.as_mut());
                    let _ = write!(output, "\x1b[1;36mKanga> \x1b[0m");
                }
                Key::ArrowUp => {
                    if let Some(console) = console.as_mut() {
                        console.scroll_up();
                    }
                }
                Key::ArrowDown => {
                    if let Some(console) = console.as_mut() {
                        console.scroll_down();
                    }
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

fn draw_rule(output: &mut Output<'_>, width: usize) {
    let _ = write!(output, "\x1b[1;36m+");
    for _ in 0..width.saturating_sub(2) {
        let _ = write!(output, "-");
    }
    let _ = writeln!(output, "+\x1b[0m");
}

fn draw_centered(output: &mut Output<'_>, width: usize, text: &str) {
    let visible_width = text
        .strip_prefix("\x1b[1;36m")
        .and_then(|value| value.strip_suffix("\x1b[0m"))
        .map_or(text.len(), str::len);
    let inner_width = width.saturating_sub(4);
    let padding = inner_width.saturating_sub(visible_width);
    let left = padding / 2;
    let right = padding.saturating_sub(left);
    let _ = write!(output, "| ");
    for _ in 0..left {
        let _ = write!(output, " ");
    }
    let _ = write!(output, "{text}");
    for _ in 0..right {
        let _ = write!(output, " ");
    }
    let _ = writeln!(output, " |");
}

fn draw_panel_line(output: &mut Output<'_>, width: usize, text: &str) {
    let inner_width = width.saturating_sub(4);
    let visible = text.len().min(inner_width);
    let _ = write!(output, "| ");
    let _ = output.write_str(&text[..visible]);
    for _ in visible..inner_width {
        let _ = write!(output, " ");
    }
    let _ = writeln!(output, " |");
}

fn draw_status(output: &mut Output<'_>, width: usize, name: &str, status: &str) {
    let inner_width = width.saturating_sub(4);
    let label_width = inner_width.saturating_sub(status.len() + 3);
    let label = if name.len() > label_width {
        &name[..label_width]
    } else {
        name
    };
    let _ = write!(output, "| {label}");
    for _ in label.len()..label_width {
        let _ = write!(output, ".");
    }
    let _ = writeln!(output, " \x1b[32m[{status}]\x1b[0m |");
}

fn execute(command_line: &[u8], console: Option<&mut Console>, filesystem: &mut FileSystem) {
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
        let _ = writeln!(output, "\x1b[1;36mKANGA COMMAND REFERENCE\x1b[0m");
        let _ = writeln!(output);
        let _ = writeln!(output, "\x1b[1;36mFILES AND STORAGE\x1b[0m");
        let _ = writeln!(output, "  DIR       List files in KangaFS");
        let _ = writeln!(
            output,
            "  WRITE     Create or replace a file: WRITE name.txt text"
        );
        let _ = writeln!(output, "  TYPE      Display a file: TYPE name.txt");
        let _ = writeln!(output, "  DEL       Delete a file: DEL name.txt");
        let _ = writeln!(output, "  FSINFO    Show filesystem capacity and status");
        let _ = writeln!(output);
        let _ = writeln!(output, "\x1b[1;36mSYSTEM INFORMATION\x1b[0m");
        let _ = writeln!(output, "  INFO      Show kernel and platform details");
        let _ = writeln!(output, "  MEM       Show kernel heap statistics");
        let _ = writeln!(output, "  TIME      Show uptime and timer ticks");
        let _ = writeln!(output, "  DATE      Show the CMOS real-time clock");
        let _ = writeln!(output, "  UPTIME    Show system uptime");
        let _ = writeln!(output, "  VER       Display the Kanga version");
        let _ = writeln!(output, "  ABOUT     Show project information");
        let _ = writeln!(output, "  CAL       Display the current month calendar");
        let _ = writeln!(output);
        let _ = writeln!(output, "\x1b[1;36mSHELL\x1b[0m");
        let _ = writeln!(output, "  ECHO      Display a message");
        let _ = writeln!(output, "  CLS       Clear the screen");
        let _ = writeln!(output, "  HELP      Display this command reference");
        let _ = writeln!(output, "  HALT      Stop the processor");
    } else if command.eq_ignore_ascii_case(b"dir") {
        let _ = writeln!(output, " Directory of KANGA:\\");
        let _ = writeln!(output);
        let mut found = false;
        for index in 0..filesystem.capacity() {
            if let Some((name, size)) = filesystem.entry(index) {
                let _ = writeln!(output, "{name:<24} {size:>4} bytes");
                found = true;
            }
        }
        if !found {
            let _ = writeln!(output, "No files.");
        }
        let _ = writeln!(output, "{} file(s)", filesystem.used());
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
        let _ = writeln!(output, "Display: ANSI colour framebuffer text console");
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
    } else if command.eq_ignore_ascii_case(b"date")
        || command.eq_ignore_ascii_case(b"clock")
        || command.eq_ignore_ascii_case(b"rtc")
    {
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
    } else if command.eq_ignore_ascii_case(b"cal") {
        match crate::rtc::read() {
            Some(time) => print_calendar(&mut output, time.year, time.month),
            None => {
                let _ = writeln!(output, "Calendar unavailable: CMOS RTC read failed.");
            }
        }
    } else if command.eq_ignore_ascii_case(b"uptime") {
        let tenths = crate::pit::elapsed_tenths();
        let _ = writeln!(output, "Uptime: {}.{:01} seconds", tenths / 10, tenths % 10);
    } else if command.eq_ignore_ascii_case(b"write") {
        let separator = arguments.iter().position(|byte| byte.is_ascii_whitespace());
        let Some(separator) = separator else {
            let _ = writeln!(output, "Usage: WRITE name.txt text");
            return;
        };
        let name = &arguments[..separator];
        let contents = trim(&arguments[separator..]);
        match filesystem.write(name, contents) {
            Ok(()) => {
                let filename = core::str::from_utf8(name).unwrap_or("?");
                let _ = writeln!(output, "Saved {filename} ({} bytes).", contents.len());
            }
            Err(error) => report_fs_error(&mut output, error),
        }
    } else if command.eq_ignore_ascii_case(b"type") {
        match filesystem.read(arguments) {
            Some(contents) => {
                let text = core::str::from_utf8(contents).unwrap_or("[binary data]");
                let _ = writeln!(output, "{text}");
            }
            None => {
                let _ = writeln!(output, "File not found.");
            }
        }
    } else if command.eq_ignore_ascii_case(b"del") || command.eq_ignore_ascii_case(b"delete") {
        if filesystem.delete(arguments) {
            let name = core::str::from_utf8(arguments).unwrap_or("?");
            let _ = writeln!(output, "Deleted {name}.");
        } else {
            let _ = writeln!(output, "File not found or invalid name.");
        }
    } else if command.eq_ignore_ascii_case(b"fsinfo") {
        let _ = writeln!(output, "Filesystem: KangaFS");
        let _ = writeln!(output, "Storage: volatile kernel RAM");
        let _ = writeln!(
            output,
            "Files: {}/{}",
            filesystem.used(),
            filesystem.capacity()
        );
        let _ = writeln!(
            output,
            "Maximum file size: {} bytes",
            filesystem.file_capacity()
        );
        let _ = writeln!(output, "Data is cleared when the system reboots.");
    } else if command.eq_ignore_ascii_case(b"about") {
        let _ = writeln!(output, "Kanga is an independent x86-64 operating system.");
        let _ = writeln!(output, "Written in Rust; booted with Limine.");
        let _ = writeln!(output, "Project version: {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(output, "License: GNU GPL version 2 (GPL-2.0-only).");
        let _ = writeln!(
            output,
            "There is no warranty; see the LICENSE file for terms."
        );
    } else if command.eq_ignore_ascii_case(b"mem") {
        let _ = writeln!(output, "PHYSICAL MEMORY");
        let _ = writeln!(output, "  Frame size: 4 KiB");
        let _ = writeln!(output, "  Allocator: bitmap-backed, up to 64 GiB");
        let _ = writeln!(output, "  Virtual memory: x86-64 four-level page tables");
        if let Some(stats) = crate::heap::statistics() {
            let _ = writeln!(output);
            let _ = writeln!(output, "KERNEL HEAP");
            let _ = writeln!(output, "  Mapped:       {} KiB", stats.mapped_bytes / 1024);
            let _ = writeln!(
                output,
                "  Occupied:     {} KiB",
                stats.occupied_bytes / 1024
            );
            let _ = writeln!(output, "  Free:         {} KiB", stats.free_bytes / 1024);
            let _ = writeln!(
                output,
                "  Largest free: {} KiB",
                stats.largest_free_block / 1024
            );
            let _ = writeln!(output, "  Free blocks:  {}", stats.free_block_count);
            let _ = writeln!(
                output,
                "  Fragmented:   {} bytes",
                stats.fragmented_free_bytes
            );
        } else {
            let _ = writeln!(output, "Kernel heap statistics are not available.");
        }
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

fn report_fs_error(output: &mut Output<'_>, error: filesystem::Error) {
    let message = match error {
        filesystem::Error::InvalidName => {
            "Invalid filename. Use letters, numbers, dot, dash or underscore."
        }
        filesystem::Error::NameTooLong => "Filename is too long (24 characters maximum).",
        filesystem::Error::FileTooLarge => "File is too large (256 bytes maximum).",
        filesystem::Error::DirectoryFull => "KangaFS directory is full (16 files maximum).",
    };
    let _ = writeln!(output, "KangaFS: {message}");
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

fn print_calendar(output: &mut Output<'_>, year: u16, month: u8) {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let Some(name) = MONTHS.get(usize::from(month.saturating_sub(1))) else {
        let _ = writeln!(output, "Invalid RTC month.");
        return;
    };
    let first_weekday = weekday(year, month, 1);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return,
    };
    let _ = writeln!(output, "       {name} {year}");
    let _ = writeln!(output, "Su Mo Tu We Th Fr Sa");
    for _ in 0..first_weekday {
        let _ = write!(output, "   ");
    }
    for day in 1..=days {
        let _ = write!(output, "{day:>2} ");
        if (first_weekday + day) % 7 == 0 {
            let _ = writeln!(output);
        }
    }
    let _ = writeln!(output);
}

fn weekday(year: u16, month: u8, day: u8) -> u8 {
    // Sakamoto's algorithm: Sunday = 0, Monday = 1, ... Saturday = 6.
    const OFFSETS: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = u32::from(year);
    let m = usize::from(month);
    if month < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + OFFSETS[m - 1] as u32 + u32::from(day)) % 7) as u8
}
