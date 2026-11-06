use limine::framebuffer::{Framebuffer, MemoryModel};

const BLACK: (u8, u8, u8) = (0, 0, 0);
const DEFAULT_FOREGROUND: (u8, u8, u8) = (192, 192, 192);

const ANSI_COLOURS: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (170, 0, 0),
    (0, 170, 0),
    (170, 85, 0),
    (0, 0, 170),
    (170, 0, 170),
    (0, 170, 170),
    (170, 170, 170),
    (85, 85, 85),
    (255, 85, 85),
    (85, 255, 85),
    (255, 255, 85),
    (85, 85, 255),
    (255, 85, 255),
    (85, 255, 255),
    (255, 255, 255),
];

#[derive(Clone, Copy)]
enum EscapeState {
    Ground,
    Escape,
    Csi,
}

#[derive(Clone, Copy)]
struct AnsiParser {
    state: EscapeState,
    parameters: [u16; 8],
    parameter_count: usize,
    current: u16,
    has_digits: bool,
    sequence_len: u8,
    foreground: (u8, u8, u8),
    background: (u8, u8, u8),
}

impl AnsiParser {
    const fn new() -> Self {
        Self {
            state: EscapeState::Ground,
            parameters: [0; 8],
            parameter_count: 0,
            current: 0,
            has_digits: false,
            sequence_len: 0,
            foreground: DEFAULT_FOREGROUND,
            background: BLACK,
        }
    }

    fn reset_sequence(&mut self) {
        self.parameters = [0; 8];
        self.parameter_count = 0;
        self.current = 0;
        self.has_digits = false;
        self.sequence_len = 0;
    }

    fn finish_parameter(&mut self) -> bool {
        if self.parameter_count >= self.parameters.len() {
            return false;
        }
        self.parameters[self.parameter_count] = self.current;
        self.parameter_count += 1;
        self.current = 0;
        self.has_digits = false;
        true
    }

    fn apply_sgr(&mut self) {
        if !self.finish_parameter() {
            self.state = EscapeState::Ground;
            self.reset_sequence();
            return;
        }
        for index in 0..self.parameter_count {
            let code = self.parameters[index];
            match code {
                0 => {
                    self.foreground = DEFAULT_FOREGROUND;
                    self.background = BLACK;
                }
                30..=37 => self.foreground = ANSI_COLOURS[(code - 30) as usize],
                40..=47 => self.background = ANSI_COLOURS[(code - 40) as usize],
                90..=97 => self.foreground = ANSI_COLOURS[(code - 90 + 8) as usize],
                100..=107 => self.background = ANSI_COLOURS[(code - 100 + 8) as usize],
                39 => self.foreground = DEFAULT_FOREGROUND,
                49 => self.background = BLACK,
                _ => {}
            }
        }
        self.state = EscapeState::Ground;
        self.reset_sequence();
    }

    fn feed(&mut self, byte: u8) -> Option<u8> {
        match self.state {
            EscapeState::Ground => {
                if byte == 0x1b {
                    self.state = EscapeState::Escape;
                    None
                } else {
                    Some(byte)
                }
            }
            EscapeState::Escape => {
                if byte == b'[' {
                    self.state = EscapeState::Csi;
                    self.reset_sequence();
                } else {
                    self.state = EscapeState::Ground;
                }
                None
            }
            EscapeState::Csi => {
                self.sequence_len = self.sequence_len.saturating_add(1);
                if self.sequence_len > 32 {
                    self.state = EscapeState::Ground;
                    self.reset_sequence();
                    return None;
                }
                match byte {
                    b'0'..=b'9' => {
                        self.current = self
                            .current
                            .saturating_mul(10)
                            .saturating_add(u16::from(byte - b'0'));
                        self.has_digits = true;
                        None
                    }
                    b';' => {
                        if !self.finish_parameter() {
                            self.state = EscapeState::Ground;
                            self.reset_sequence();
                        }
                        None
                    }
                    b'm' => {
                        self.apply_sgr();
                        None
                    }
                    0x40..=0x7e => {
                        self.state = EscapeState::Ground;
                        self.reset_sequence();
                        None
                    }
                    _ => None,
                }
            }
        }
    }
}

struct Surface {
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    red_size: u8,
    red_shift: u8,
    green_size: u8,
    green_shift: u8,
    blue_size: u8,
    blue_shift: u8,
}

impl Surface {
    unsafe fn fill(&self, colour: (u8, u8, u8)) {
        // SAFETY: Coordinates are bounded by the framebuffer dimensions.
        unsafe { self.rect(0, 0, self.width, self.height, colour) };
    }

    unsafe fn rect(&self, x: usize, y: usize, width: usize, height: usize, colour: (u8, u8, u8)) {
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        for row in y.min(self.height)..bottom {
            for column in x.min(self.width)..right {
                // SAFETY: rect clamps coordinates to the framebuffer bounds.
                unsafe { self.pixel(column, row, colour) };
            }
        }
    }

    unsafe fn pixel(&self, x: usize, y: usize, colour: (u8, u8, u8)) {
        let packed = channel(colour.0, self.red_size, self.red_shift)
            | channel(colour.1, self.green_size, self.green_shift)
            | channel(colour.2, self.blue_size, self.blue_shift);
        let offset = y * self.pitch + x * 4;
        // SAFETY: x/y are bounded and pitch was validated against width.
        unsafe {
            self.address
                .add(offset)
                .cast::<u32>()
                .write_volatile(packed);
        }
    }
}

fn channel(value: u8, size: u8, shift: u8) -> u32 {
    if size == 0 || size > 8 || shift >= 32 || size as u16 + shift as u16 > 32 {
        return 0;
    }
    let maximum = (1_u32 << size) - 1;
    ((u32::from(value) * maximum / 255) & maximum) << shift
}

use crate::font;

const CELL_WIDTH: usize = 8;
const CELL_HEIGHT: usize = 16;

/// A small framebuffer text console. The framebuffer mapping must remain valid
/// and exclusively writable for the lifetime of this value.
pub struct Console {
    surface: Surface,
    columns: usize,
    rows: usize,
    column: usize,
    row: usize,
    ansi: AnsiParser,
}

impl Console {
    /// # Safety
    /// The supplied Limine framebuffer must remain mapped and writable.
    pub unsafe fn new(framebuffer: &Framebuffer<'_>) -> Option<Self> {
        if framebuffer.memory_model() != MemoryModel::RGB || framebuffer.bpp() != 32 {
            return None;
        }
        let width = usize::try_from(framebuffer.width()).ok()?;
        let height = usize::try_from(framebuffer.height()).ok()?;
        let pitch = usize::try_from(framebuffer.pitch()).ok()?;
        let minimum_pitch = width.checked_mul(4)?;
        let total_bytes = pitch.checked_mul(height)?;
        if width < CELL_WIDTH
            || height < CELL_HEIGHT
            || pitch < minimum_pitch
            || total_bytes == 0
            || framebuffer.addr().is_null()
        {
            return None;
        }
        let surface = Surface {
            address: framebuffer.addr(),
            width,
            height,
            pitch,
            red_size: framebuffer.red_mask_size(),
            red_shift: framebuffer.red_mask_shift(),
            green_size: framebuffer.green_mask_size(),
            green_shift: framebuffer.green_mask_shift(),
            blue_size: framebuffer.blue_mask_size(),
            blue_shift: framebuffer.blue_mask_shift(),
        };
        let mut console = Self {
            surface,
            columns: width / CELL_WIDTH,
            rows: height / CELL_HEIGHT,
            column: 0,
            row: 0,
            ansi: AnsiParser::new(),
        };
        console.clear();
        Some(console)
    }

    pub fn clear(&mut self) {
        // SAFETY: Console owns the validated live framebuffer mapping.
        unsafe { self.surface.fill(self.ansi.background) };
        self.column = 0;
        self.row = 0;
        self.draw_prompt_cursor();
    }

    pub fn write_text(&mut self, text: &str) {
        self.erase_prompt_cursor();
        for byte in text.bytes() {
            let Some(byte) = self.ansi.feed(byte) else {
                continue;
            };
            match byte {
                b'\n' => self.newline(),
                b'\r' => self.column = 0,
                b'\x08' => self.backspace(),
                0x20..=0x7e => self.put_character(byte as char),
                _ => self.put_character('?'),
            }
        }
        self.draw_prompt_cursor();
    }

    fn put_character(&mut self, character: char) {
        if self.column >= self.columns {
            self.newline();
        }
        let x = self.column * CELL_WIDTH;
        let y = self.row * CELL_HEIGHT;
        let rows = font::glyph(character as u8);
        // Render a genuine 8x16 VGA glyph: eight pixels wide and sixteen scanlines.
        // SAFETY: The cell is inside the validated console grid.
        unsafe {
            self.surface
                .rect(x, y, CELL_WIDTH, CELL_HEIGHT, self.ansi.background);
            for (row, bits) in rows.iter().enumerate() {
                for column in 0..8 {
                    if bits & (1 << (7 - column)) != 0 {
                        self.surface
                            .rect(x + column, y + row, 1, 1, self.ansi.foreground);
                    }
                }
            }
        }
        self.column += 1;
    }

    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.scroll();
            self.row = self.rows - 1;
        }
    }

    fn backspace(&mut self) {
        if self.column == 0 {
            return;
        }
        self.column -= 1;
        let x = self.column * CELL_WIDTH;
        let y = self.row * CELL_HEIGHT;
        // SAFETY: The cursor remains within the console grid.
        unsafe {
            self.surface
                .rect(x, y, CELL_WIDTH, CELL_HEIGHT, self.ansi.background)
        };
    }

    fn erase_prompt_cursor(&mut self) {
        let x = self.column * CELL_WIDTH;
        let y = self.row * CELL_HEIGHT + CELL_HEIGHT - 2;
        // The cursor is drawn only in the otherwise-empty next character cell.
        // Restore that scanline to the active background before moving it.
        // SAFETY: Cursor coordinates are within the clipped console surface.
        unsafe { self.surface.rect(x, y, 7, 1, self.ansi.background) };
    }

    fn scroll(&mut self) {
        let pitch = self.surface.pitch;
        let first_source_row = CELL_HEIGHT;
        let rows_to_move = self.surface.height.saturating_sub(CELL_HEIGHT);
        // Move one complete physical framebuffer row at a time. Every offset
        // is derived from Limine's pitch, including any alignment padding.
        // Forward order is safe because each source row is below its target.
        // SAFETY: Console::new validated the full pitch * height mapping.
        unsafe {
            for destination_row in 0..rows_to_move {
                let source_offset = (destination_row + first_source_row) * pitch;
                let destination_offset = destination_row * pitch;
                core::ptr::copy_nonoverlapping(
                    self.surface.address.add(source_offset),
                    self.surface.address.add(destination_offset),
                    pitch,
                );
            }
        }
        // Clear the newly exposed bottom rows.
        // SAFETY: The rectangle is clipped to the framebuffer bounds.
        unsafe {
            self.surface.rect(
                0,
                self.surface.height.saturating_sub(CELL_HEIGHT),
                self.surface.width,
                CELL_HEIGHT,
                self.ansi.background,
            );
        }
    }

    fn draw_prompt_cursor(&mut self) {
        let x = self.column * CELL_WIDTH;
        let y = self.row * CELL_HEIGHT + CELL_HEIGHT - 2;
        // SAFETY: Cursor coordinates are within the console cell.
        unsafe { self.surface.rect(x, y, 7, 1, self.ansi.foreground) };
    }
}

impl core::fmt::Write for Console {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        self.write_text(text);
        Ok(())
    }
}
