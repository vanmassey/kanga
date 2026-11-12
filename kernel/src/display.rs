//! Minimal RGB framebuffer text console for the graphical QEMU display.
//!
//! The console occupies the lower two-fifths of the display so the Kanga
//! mascot can remain visible above it. It uses fixed 8x16 character cells,
//! a compact built-in 5x7 ASCII font, and no heap allocation.

use core::fmt;
use core::ptr;

use limine::framebuffer::MemoryModel;
use limine::request::FramebufferRequest;

#[used]
#[link_section = ".limine_requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

const CELL_WIDTH: usize = 8;
const CELL_HEIGHT: usize = 16;
const CONSOLE_TOP_NUMERATOR: usize = 2;
const CONSOLE_TOP_DENOMINATOR: usize = 3;
const DEFAULT_FG: u32 = 0xD8DEE9;
const DEFAULT_BG: u32 = 0x101018;

struct Display {
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    top: usize,
    columns: usize,
    rows: usize,
    column: usize,
    row: usize,
    red_shift: u8,
    green_shift: u8,
    blue_shift: u8,
    foreground: u32,
    background: u32,
    escape: u8,
    parameter: usize,
}

static mut DISPLAY: Option<Display> = None;

/// Initialise the text area below the mascot. Returns false if no supported
/// 32-bit RGB framebuffer was supplied by Limine.
pub fn initialise() -> bool {
    // SAFETY: Limine owns the response and framebuffer mapping for the kernel lifetime.
    let Some(response) = (unsafe { FRAMEBUFFER_REQUEST.get_response() }) else {
        return false;
    };
    let Some(framebuffer) = response.framebuffers().next() else {
        return false;
    };
    if framebuffer.bpp() != 32 || framebuffer.memory_model() != MemoryModel::RGB {
        return false;
    }

    let width = framebuffer.width().min(usize::MAX as u64) as usize;
    let height = framebuffer.height().min(usize::MAX as u64) as usize;
    let pitch = framebuffer.pitch() as usize;
    let top = (height * CONSOLE_TOP_NUMERATOR / CONSOLE_TOP_DENOMINATOR / CELL_HEIGHT)
        * CELL_HEIGHT;
    if width < CELL_WIDTH || height.saturating_sub(top) < CELL_HEIGHT * 3
        || pitch < width.saturating_mul(4)
    {
        return false;
    }

    let display = Display {
        address: framebuffer.addr(),
        width,
        height,
        pitch,
        top,
        columns: width / CELL_WIDTH,
        rows: (height - top) / CELL_HEIGHT,
        column: 0,
        row: 0,
        red_shift: framebuffer.red_mask_shift(),
        green_shift: framebuffer.green_mask_shift(),
        blue_shift: framebuffer.blue_mask_shift(),
        foreground: DEFAULT_FG,
        background: DEFAULT_BG,
        escape: 0,
        parameter: 0,
    };
    // SAFETY: Initialisation happens once on the bootstrap CPU before the shell starts.
    unsafe {
        ptr::addr_of_mut!(DISPLAY).write(Some(display));
        if let Some(display) = (*ptr::addr_of_mut!(DISPLAY)).as_mut() {
            display.clear();
        }
    }
    true
}

/// A formatting writer that mirrors shell output to COM1 and the framebuffer.
pub(crate) struct Writer;

impl fmt::Write for Writer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let mut serial = crate::serial::Writer;
        let _ = fmt::Write::write_str(&mut serial, text);
        write_framebuffer(text);
        Ok(())
    }
}

fn write_framebuffer(text: &str) {
    // SAFETY: The display is written only by the bootstrap shell thread.
    unsafe {
        if let Some(display) = (*ptr::addr_of_mut!(DISPLAY)).as_mut() {
            for byte in text.bytes() {
                display.write_byte(byte);
            }
        }
    }
}

impl Display {
    fn colour(&self, rgb: u32) -> u32 {
        ((rgb >> 16) & 0xff) << self.red_shift
            | ((rgb >> 8) & 0xff) << self.green_shift
            | (rgb & 0xff) << self.blue_shift
    }

    fn pixel(&mut self, x: usize, y: usize, rgb: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = y * self.pitch + x * 4;
        let pixel = self.colour(rgb);
        // SAFETY: The framebuffer dimensions and pitch were validated at init.
        unsafe {
            self.address.add(offset).cast::<u32>().write_volatile(pixel);
        }
    }

    fn clear(&mut self) {
        self.column = 0;
        self.row = 0;
        self.foreground = DEFAULT_FG;
        self.background = DEFAULT_BG;
        for y in self.top..self.height {
            for x in 0..self.width {
                self.pixel(x, y, self.background);
            }
        }
    }

    fn draw_character(&mut self, column: usize, row: usize, byte: u8) {
        let left = column * CELL_WIDTH;
        let top = self.top + row * CELL_HEIGHT;
        if left + CELL_WIDTH > self.width || top + CELL_HEIGHT > self.height {
            return;
        }
        let character = if byte.is_ascii_lowercase() {
            byte.to_ascii_uppercase()
        } else {
            byte
        };
        for y in 0..CELL_HEIGHT {
            let bits = glyph_row(character, y / 2);
            for x in 0..CELL_WIDTH {
                let on = x > 0 && x < 6 && bits & (1 << (5 - x)) != 0;
                self.pixel(
                    left + x,
                    top + y,
                    if on { self.foreground } else { self.background },
                );
            }
        }
    }

    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.scroll();
        }
    }

    fn scroll(&mut self) {
        if self.rows == 0 {
            return;
        }
        let scroll_bytes = (self.height - self.top - CELL_HEIGHT) * self.pitch;
        // SAFETY: Source and destination are valid framebuffer ranges. ptr::copy
        // permits overlap, which is required when scrolling the display upward.
        unsafe {
            ptr::copy(
                self.address.add(self.top * self.pitch + CELL_HEIGHT * self.pitch),
                self.address.add(self.top * self.pitch),
                scroll_bytes,
            );
        }
        self.row = self.rows - 1;
        self.column = 0;
        self.clear_row(self.row, 0);
    }

    fn clear_row(&mut self, row: usize, from_column: usize) {
        for column in from_column..self.columns {
            self.draw_character(column, row, b' ');
        }
    }

    fn write_byte(&mut self, byte: u8) {
        match self.escape {
            1 => {
                self.escape = if byte == b'[' { 2 } else { 0 };
                self.parameter = 0;
                return;
            }
            2 => {
                if byte.is_ascii_digit() {
                    self.parameter = self.parameter.saturating_mul(10)
                        .saturating_add((byte - b'0') as usize);
                    return;
                }
                match byte {
                    b'J' if self.parameter == 0 || self.parameter == 2 => self.clear(),
                    b'H' => {
                        self.row = 0;
                        self.column = 0;
                    }
                    b'K' => self.clear_row(self.row, self.column),
                    b'm' => self.set_sgr(self.parameter),
                    _ => {}
                }
                self.escape = 0;
                self.parameter = 0;
                return;
            }
            _ => {}
        }

        match byte {
            0x1B => self.escape = 1,
            b'\r' => self.column = 0,
            b'\n' => self.newline(),
            0x08 | 0x7F => {
                if self.column > 0 {
                    self.column -= 1;
                    self.draw_character(self.column, self.row, b' ');
                }
            }
            b'\t' => {
                for _ in 0..4 {
                    self.write_byte(b' ');
                }
            }
            0x20..=0x7E => {
                self.draw_character(self.column, self.row, byte);
                self.column += 1;
                if self.column >= self.columns {
                    self.newline();
                }
            }
            _ => self.draw_character_and_advance(b'?'),
        }
    }

    fn draw_character_and_advance(&mut self, byte: u8) {
        self.draw_character(self.column, self.row, byte);
        self.column += 1;
        if self.column >= self.columns {
            self.newline();
        }
    }

    fn set_sgr(&mut self, parameter: usize) {
        match parameter {
            0 => {
                self.foreground = DEFAULT_FG;
                self.background = DEFAULT_BG;
            }
            30..=37 => self.foreground = ansi_colour(parameter - 30),
            90..=97 => self.foreground = ansi_colour(parameter - 90 + 8),
            40..=47 => self.background = ansi_colour(parameter - 40),
            100..=107 => self.background = ansi_colour(parameter - 100 + 8),
            _ => {}
        }
    }
}

fn ansi_colour(index: usize) -> u32 {
    const PALETTE: [u32; 16] = [
        0x202028, 0xCD3131, 0x0DBC79, 0xE5E510,
        0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5,
        0x666666, 0xF14C4C, 0x23D18B, 0xF5F543,
        0x3B8EEA, 0xD670D6, 0x29B8DB, 0xFFFFFF,
    ];
    PALETTE[index.min(15)]
}

// A compact 5x7 font in 8x16 cells. Lowercase input is displayed using the
// corresponding uppercase glyph, keeping the kernel independent of assets.
fn glyph_row(byte: u8, row: usize) -> u8 {
    let glyph: [u8; 7] = match byte {
        b'A' => [14,17,17,31,17,17,17],
        b'B' => [30,17,17,30,17,17,30],
        b'C' => [14,17,16,16,16,17,14],
        b'D' => [30,17,17,17,17,17,30],
        b'E' => [31,16,16,30,16,16,31],
        b'F' => [31,16,16,30,16,16,16],
        b'G' => [14,17,16,23,17,17,15],
        b'H' => [17,17,17,31,17,17,17],
        b'I' => [14,4,4,4,4,4,14],
        b'J' => [7,2,2,2,18,18,12],
        b'K' => [17,18,20,24,20,18,17],
        b'L' => [16,16,16,16,16,16,31],
        b'M' => [17,27,21,21,17,17,17],
        b'N' => [17,25,21,19,17,17,17],
        b'O' => [14,17,17,17,17,17,14],
        b'P' => [30,17,17,30,16,16,16],
        b'Q' => [14,17,17,17,21,18,13],
        b'R' => [30,17,17,30,20,18,17],
        b'S' => [15,16,16,14,1,1,30],
        b'T' => [31,4,4,4,4,4,4],
        b'U' => [17,17,17,17,17,17,14],
        b'V' => [17,17,17,17,17,10,4],
        b'W' => [17,17,17,21,21,21,10],
        b'X' => [17,17,10,4,10,17,17],
        b'Y' => [17,17,10,4,4,4,4],
        b'Z' => [31,1,2,4,8,16,31],
        b'0' => [14,17,19,21,25,17,14],
        b'1' => [4,12,4,4,4,4,14],
        b'2' => [14,17,1,2,4,8,31],
        b'3' => [30,1,1,14,1,1,30],
        b'4' => [2,6,10,18,31,2,2],
        b'5' => [31,16,16,30,1,1,30],
        b'6' => [14,16,16,30,17,17,14],
        b'7' => [31,1,2,4,8,8,8],
        b'8' => [14,17,17,14,17,17,14],
        b'9' => [14,17,17,15,1,1,14],
        b'.' => [0,0,0,0,0,12,12],
        b',' => [0,0,0,0,4,4,8],
        b':' => [0,12,12,0,12,12,0],
        b';' => [0,12,12,0,4,4,8],
        b'!' => [4,4,4,4,4,0,4],
        b'?' => [14,17,1,2,4,0,4],
        b'-' => [0,0,0,31,0,0,0],
        b'_' => [0,0,0,0,0,0,31],
        b'+' => [0,4,4,31,4,4,0],
        b'=' => [0,31,0,31,0,0,0],
        b'/' => [1,2,2,4,8,8,16],
        b'\\' => [16,8,8,4,2,2,1],
        b'|' => [4,4,4,4,4,4,4],
        b'(' => [2,4,8,8,8,4,2],
        b')' => [8,4,2,2,2,4,8],
        b'[' => [14,8,8,8,8,8,14],
        b']' => [14,2,2,2,2,2,14],
        b'<' => [2,4,8,16,8,4,2],
        b'>' => [8,4,2,1,2,4,8],
        b'\'' => [4,4,8,0,0,0,0],
        b'"' => [10,10,0,0,0,0,0],
        b'#' => [10,31,10,10,31,10,0],
        b'$' => [4,15,20,14,5,30,4],
        b'%' => [17,2,4,8,17,0,0],
        b'*' => [0,21,14,31,14,21,0],
        b'@' => [14,17,23,21,23,16,14],
        b' ' => [0; 7],
        _ => [31,17,1,2,4,0,4],
    };
    if row < glyph.len() { glyph[row] } else { 0 }
}
