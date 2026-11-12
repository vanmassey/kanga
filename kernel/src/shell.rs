//! kash: the Kanga serial shell.
//!
//! The shell runs on the bootstrap thread from the idle loop. It reads bytes
//! from COM1, supports line editing and command history over ANSI terminals,
//! and drives KangaFS plus the kernel's diagnostic services. All state lives in
//! the `Shell` value owned by the idle loop; nothing here allocates.

use core::fmt::Write;

use crate::filesystem::{self, Error, FileSystem};
use crate::serial::Writer;
use crate::{console, heap, mascot, memory, paging, pit, power, rtc, scheduler};

const LINE_CAPACITY: usize = 128;
const HISTORY_DEPTH: usize = 8;
const MAX_COMMAND_LENGTH: usize = 16;
const TASK_SCAN_LIMIT: usize = 8;
const PROMPT: &str = "kash> ";
const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";

const HELP_TEXT: &str = "\
kash commands (case-insensitive):
  HELP                  show this list
  VER, INFO, UPTIME     version, system summary, time since boot
  DATE (TIME)           read the CMOS real-time clock
  MEM, TASKS            physical memory and scheduler state
  DIR (LS)              list KangaFS files
  TYPE (CAT) <file>     print a file
  WRITE <file> <text>   create or replace a file
  APPEND <file> <text>  append text to a file
  COPY (CP) <src> <dst> copy a file
  DEL (RM) <file>       delete a file
  FSINFO (DF)           filesystem usage
  HEXDUMP <file>        hex and ASCII dump of a file
  ECHO <text>           print text
  HISTORY               recent commands (Up and Down arrows recall them)
  CLS                   clear the terminal
  KANGA                 redraw the framebuffer mascot
  SELFTEST              rerun the paging and heap self-tests
  REBOOT, HALT, POWEROFF
Line editing: Backspace, Ctrl-U erase line, Ctrl-C cancel, Ctrl-L redraw.
";

macro_rules! say {
    ($($arg:tt)*) => {{
        let _ = write!(Writer, $($arg)*);
    }};
}

macro_rules! sayln {
    () => {{
        let _ = writeln!(Writer);
    }};
    ($($arg:tt)*) => {{
        let _ = writeln!(Writer, $($arg)*);
    }};
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Escape {
    None,
    Esc,
    Csi,
}

/// Interactive shell state: the line being edited, history and KangaFS.
pub struct Shell {
    line: [u8; LINE_CAPACITY],
    length: usize,
    escape: Escape,
    skip_line_feed: bool,
    history: [[u8; LINE_CAPACITY]; HISTORY_DEPTH],
    history_lengths: [usize; HISTORY_DEPTH],
    history_head: usize,
    history_count: usize,
    browsing: Option<usize>,
    files: FileSystem,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            line: [0; LINE_CAPACITY],
            length: 0,
            escape: Escape::None,
            skip_line_feed: false,
            history: [[0; LINE_CAPACITY]; HISTORY_DEPTH],
            history_lengths: [0; HISTORY_DEPTH],
            history_head: 0,
            history_count: 0,
            browsing: None,
            files: FileSystem::new(),
        }
    }

    /// Print the banner and first prompt.
    pub fn start(&mut self) {
        sayln!();
        sayln!("Welcome to kash, the Kanga shell. Type HELP for commands.");
        prompt();
    }

    /// Process every byte currently waiting in the UART receive FIFO.
    pub fn poll(&mut self) {
        while let Some(byte) = console::read_byte() {
            self.handle_byte(byte);
        }
    }

    fn handle_byte(&mut self, byte: u8) {
        if self.consume_escape(byte) {
            return;
        }
        let after_carriage_return = core::mem::replace(&mut self.skip_line_feed, false);
        match byte {
            0x1B => self.escape = Escape::Esc,
            b'\r' => {
                self.skip_line_feed = true;
                self.submit();
            }
            b'\n' if after_carriage_return => {}
            b'\n' => self.submit(),
            0x08 | 0x7F => self.backspace(),
            0x03 => self.cancel(),
            0x15 => {
                self.browsing = None;
                self.erase_display();
            }
            0x0C => self.redraw(),
            0x20..=0x7E => self.insert(byte),
            _ => {}
        }
    }

    /// Swallow ANSI escape sequences, acting on Up and Down arrows.
    fn consume_escape(&mut self, byte: u8) -> bool {
        match self.escape {
            Escape::None => false,
            Escape::Esc => {
                self.escape = if byte == b'[' {
                    Escape::Csi
                } else {
                    Escape::None
                };
                true
            }
            Escape::Csi => {
                if (0x40..=0x7E).contains(&byte) {
                    self.escape = Escape::None;
                    match byte {
                        b'A' => self.history_step(true),
                        b'B' => self.history_step(false),
                        _ => {}
                    }
                }
                true
            }
        }
    }

    fn insert(&mut self, byte: u8) {
        if self.length < LINE_CAPACITY {
            self.line[self.length] = byte;
            self.length += 1;
            self.browsing = None;
            echo(byte);
        }
    }

    fn backspace(&mut self) {
        if self.length > 0 {
            self.length -= 1;
            self.browsing = None;
            say!("\x08 \x08");
        }
    }

    fn cancel(&mut self) {
        sayln!("^C");
        self.length = 0;
        self.browsing = None;
        prompt();
    }

    fn redraw(&self) {
        say!("{}", CLEAR_SCREEN);
        prompt();
        for &byte in &self.line[..self.length] {
            echo(byte);
        }
    }

    /// Visually erase the current line and empty the edit buffer.
    fn erase_display(&mut self) {
        for _ in 0..self.length {
            say!("\x08 \x08");
        }
        self.length = 0;
    }

    fn replace_line(&mut self, text: &[u8]) {
        self.erase_display();
        self.line[..text.len()].copy_from_slice(text);
        self.length = text.len();
        for &byte in text {
            echo(byte);
        }
    }

    fn submit(&mut self) {
        sayln!();
        let length = self.length;
        self.length = 0;
        self.browsing = None;
        let mut buffer = [0_u8; LINE_CAPACITY];
        buffer[..length].copy_from_slice(&self.line[..length]);
        if let Ok(text) = core::str::from_utf8(&buffer[..length]) {
            let text = text.trim();
            if !text.is_empty() {
                self.remember(text.as_bytes());
                self.execute(text);
            }
        }
        prompt();
    }

    fn remember(&mut self, text: &[u8]) {
        if self.history_count > 0 {
            let last = (self.history_head + HISTORY_DEPTH - 1) % HISTORY_DEPTH;
            if &self.history[last][..self.history_lengths[last]] == text {
                return;
            }
        }
        let slot = self.history_head;
        self.history[slot][..text.len()].copy_from_slice(text);
        self.history_lengths[slot] = text.len();
        self.history_head = (slot + 1) % HISTORY_DEPTH;
        if self.history_count < HISTORY_DEPTH {
            self.history_count += 1;
        }
    }

    /// Move through history; `older` is Up Arrow, otherwise Down Arrow.
    fn history_step(&mut self, older: bool) {
        let target = match (self.browsing, older) {
            (None, true) if self.history_count > 0 => 1,
            (Some(age), true) if age < self.history_count => age + 1,
            (Some(1), false) => 0,
            (Some(age), false) if age > 1 => age - 1,
            _ => return,
        };
        if target == 0 {
            self.browsing = None;
            self.replace_line(&[]);
            return;
        }
        self.browsing = Some(target);
        let slot = (self.history_head + HISTORY_DEPTH - target) % HISTORY_DEPTH;
        let length = self.history_lengths[slot];
        let mut copy = [0_u8; LINE_CAPACITY];
        copy[..length].copy_from_slice(&self.history[slot][..length]);
        self.replace_line(&copy[..length]);
    }

    fn execute(&mut self, line: &str) {
        let (word, rest) = split_word(line);
        let mut lowered = [0_u8; MAX_COMMAND_LENGTH];
        match lowercase(word, &mut lowered) {
            "help" | "?" => say!("{}", HELP_TEXT),
            "ver" | "version" => sayln!("Kanga kernel {}", env!("CARGO_PKG_VERSION")),
            "info" | "uname" => info(),
            "uptime" => uptime(),
            "date" | "time" | "clock" | "rtc" => date(),
            "mem" => memory_report(),
            "tasks" => tasks(),
            "cls" | "clear" => say!("{}", CLEAR_SCREEN),
            "echo" => sayln!("{}", rest),
            "history" => self.show_history(),
            "dir" | "ls" => self.list_files(),
            "type" | "cat" => self.type_file(rest),
            "write" => self.write_file(rest),
            "append" => self.append_file(rest),
            "copy" | "cp" => self.copy_file(rest),
            "del" | "rm" => self.delete_file(rest),
            "fsinfo" | "df" => self.file_system_info(),
            "hexdump" => self.hexdump(rest),
            "kanga" => kanga(),
            "selftest" => selftest(),
            "reboot" => power::reboot(),
            "poweroff" => power::power_off(),
            "halt" => {
                sayln!("System halted.");
                power::halt()
            }
            _ => sayln!("kash: unknown command '{}' (type HELP)", word),
        }
    }

    fn show_history(&self) {
        if self.history_count == 0 {
            sayln!("(no history)");
            return;
        }
        for age in (1..=self.history_count).rev() {
            let slot = (self.history_head + HISTORY_DEPTH - age) % HISTORY_DEPTH;
            let bytes = &self.history[slot][..self.history_lengths[slot]];
            let text = core::str::from_utf8(bytes).unwrap_or("");
            let number = self.history_count - age + 1;
            sayln!("{:>3}  {}", number, text);
        }
    }

    fn list_files(&self) {
        let capacity = self.files.capacity();
        let mut count = 0;
        for index in 0..capacity {
            if let Some((name, size)) = self.files.entry(index) {
                sayln!("{:<24} {:>4} bytes", name, size);
                count += 1;
            }
        }
        let free = capacity - count;
        sayln!("{} file(s), {} slot(s) free", count, free);
    }

    fn type_file(&self, args: &str) {
        let (name, _) = split_word(args);
        if name.is_empty() {
            sayln!("usage: TYPE <file>");
            return;
        }
        let Some(data) = self.files.read(name.as_bytes()) else {
            sayln!("type: {}: no such file", name);
            return;
        };
        match core::str::from_utf8(data) {
            Ok(text) => {
                say!("{}", text);
                if !text.ends_with('\n') {
                    sayln!();
                }
            }
            Err(_) => sayln!("type: {}: not a text file", name),
        }
    }

    fn write_file(&mut self, args: &str) {
        let (name, text) = split_word(args);
        if name.is_empty() {
            sayln!("usage: WRITE <file> <text>");
            return;
        }
        match self.files.write(name.as_bytes(), text.as_bytes()) {
            Ok(()) => sayln!("{} bytes written to {}", text.len(), name),
            Err(error) => report("write", error),
        }
    }

    fn append_file(&mut self, args: &str) {
        let (name, text) = split_word(args);
        if name.is_empty() {
            sayln!("usage: APPEND <file> <text>");
            return;
        }
        let mut buffer = [0_u8; filesystem::FILE_CAPACITY];
        let mut length = 0;
        if let Some(existing) = self.files.read(name.as_bytes()) {
            buffer[..existing.len()].copy_from_slice(existing);
            length = existing.len();
        }
        let addition = text.as_bytes();
        if length + addition.len() > buffer.len() {
            report("append", Error::FileTooLarge);
            return;
        }
        buffer[length..length + addition.len()].copy_from_slice(addition);
        length += addition.len();
        match self.files.write(name.as_bytes(), &buffer[..length]) {
            Ok(()) => sayln!("{} bytes in {}", length, name),
            Err(error) => report("append", error),
        }
    }

    fn copy_file(&mut self, args: &str) {
        let (source, rest) = split_word(args);
        let (destination, _) = split_word(rest);
        if source.is_empty() || destination.is_empty() {
            sayln!("usage: COPY <source> <destination>");
            return;
        }
        match self.files.copy(source.as_bytes(), destination.as_bytes()) {
            Ok(()) => sayln!("copied {} to {}", source, destination),
            Err(error) => report("copy", error),
        }
    }

    fn delete_file(&mut self, args: &str) {
        let (name, _) = split_word(args);
        if name.is_empty() {
            sayln!("usage: DEL <file>");
        } else if self.files.delete(name.as_bytes()) {
            sayln!("deleted {}", name);
        } else {
            sayln!("del: {}: no such file", name);
        }
    }

    fn file_system_info(&self) {
        let used = self.files.used();
        let capacity = self.files.capacity();
        let mut bytes = 0;
        for index in 0..capacity {
            if let Some((_, size)) = self.files.entry(index) {
                bytes += size;
            }
        }
        sayln!("Files: {} of {} used", used, capacity);
        sayln!("Stored: {} bytes", bytes);
        sayln!("Maximum file size: {} bytes", self.files.file_capacity());
    }

    fn hexdump(&self, args: &str) {
        let (name, _) = split_word(args);
        if name.is_empty() {
            sayln!("usage: HEXDUMP <file>");
            return;
        }
        let Some(data) = self.files.read(name.as_bytes()) else {
            sayln!("hexdump: {}: no such file", name);
            return;
        };
        if data.is_empty() {
            sayln!("(empty file)");
            return;
        }
        for (row, chunk) in data.chunks(16).enumerate() {
            say!("{:04x}  ", row * 16);
            for column in 0..16 {
                match chunk.get(column) {
                    Some(byte) => say!("{:02x} ", byte),
                    None => say!("   "),
                }
            }
            say!(" |");
            for &byte in chunk {
                let shown = if byte.is_ascii_graphic() || byte == b' ' {
                    byte as char
                } else {
                    '.'
                };
                say!("{}", shown);
            }
            sayln!("|");
        }
    }
}

fn prompt() {
    say!("{}", PROMPT);
}

fn echo(byte: u8) {
    say!("{}", byte as char);
}

fn info() {
    sayln!("Kanga kernel {}", env!("CARGO_PKG_VERSION"));
    sayln!("Architecture: x86_64, boot protocol: Limine");
    sayln!("Timer: PIT at 100 Hz, pre-emptive round-robin scheduler");
    uptime();
    memory_report();
}

fn uptime() {
    let seconds = pit::elapsed_tenths() / 10;
    let hours = seconds / 3600;
    let minutes = (seconds / 60) % 60;
    sayln!("up {}h {:02}m {:02}s", hours, minutes, seconds % 60);
    sayln!("{} timer ticks", pit::ticks());
}

fn date() {
    let Some(clock) = rtc::read() else {
        sayln!("date: RTC unavailable or invalid CMOS time");
        return;
    };
    sayln!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        clock.year,
        clock.month,
        clock.day,
        clock.hour,
        clock.minute,
        clock.second
    );
}

fn memory_report() {
    let reported = memory::with_global_allocator(|allocator| {
        let managed = allocator.managed_frame_count() as u64;
        let free = allocator.free_frame_count() as u64;
        let managed_mib = mib(managed);
        let free_mib = mib(free);
        sayln!("Physical frames: {} managed, {} free", managed, free);
        sayln!("Physical memory: {} of {} MiB free", free_mib, managed_mib);
    });
    if reported.is_none() {
        sayln!("mem: frame allocator unavailable");
    }
}

fn mib(frames: u64) -> u64 {
    frames * memory::PAGE_SIZE / (1024 * 1024)
}

fn tasks() {
    let (current, switches, worker_a, worker_b) = scheduler::statistics();
    sayln!("Current task: {}, context switches: {}", current, switches);
    for index in 0..TASK_SCAN_LIMIT {
        if let Some((name, running)) = scheduler::task_info(index) {
            let marker = if running { " (running)" } else { "" };
            sayln!("  {} {}{}", index, name, marker);
        }
    }
    sayln!("Worker ticks: a={}, b={}", worker_a, worker_b);
}

fn kanga() {
    if mascot::draw() {
        sayln!("Kanga hops onto the framebuffer.");
    } else {
        sayln!("kanga: no compatible RGB framebuffer");
    }
}

fn selftest() {
    sayln!("Paging self-test: {}", verdict(paging::self_test()));
    sayln!("Heap self-test: {}", verdict(heap::self_test()));
}

fn verdict(passed: bool) -> &'static str {
    if passed { "passed" } else { "FAILED" }
}

fn report(command: &str, error: Error) {
    say!("{}: ", command);
    match error {
        Error::InvalidName => sayln!("invalid name (letters, digits, . _ - only)"),
        Error::NameTooLong => sayln!("name longer than {} bytes", filesystem::NAME_CAPACITY),
        Error::FileTooLarge => sayln!("larger than {} bytes", filesystem::FILE_CAPACITY),
        Error::DirectoryFull => sayln!("directory full ({} files)", filesystem::MAX_FILES),
        Error::NotFound => sayln!("no such file"),
    }
}

/// Split off the first whitespace-delimited word; the rest is trimmed.
fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(char::is_whitespace) {
        Some(index) => (&text[..index], text[index..].trim_start()),
        None => (text, ""),
    }
}

/// ASCII-lowercase `text` into `buffer`; over-long words become an empty string.
fn lowercase<'a>(text: &str, buffer: &'a mut [u8; MAX_COMMAND_LENGTH]) -> &'a str {
    if text.len() > MAX_COMMAND_LENGTH {
        return "";
    }
    for (slot, byte) in buffer.iter_mut().zip(text.bytes()) {
        *slot = byte.to_ascii_lowercase();
    }
    core::str::from_utf8(&buffer[..text.len()]).unwrap_or("")
}
