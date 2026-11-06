use limine::framebuffer::{Framebuffer, MemoryModel};

const BACKGROUND: (u8, u8, u8) = (0, 0, 0);
const WHITE: (u8, u8, u8) = (192, 192, 192);

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
        };
        console.clear();
        Some(console)
    }

    pub fn clear(&mut self) {
        // SAFETY: Console owns the validated live framebuffer mapping.
        unsafe { self.surface.fill(BACKGROUND) };
        self.column = 0;
        self.row = 0;
        self.draw_prompt_cursor();
    }

    pub fn write_text(&mut self, text: &str) {
        for byte in text.bytes() {
            match byte {
                b'\n' => self.newline(),
                b'\r' => self.column = 0,
                b'\x08' => self.backspace(),
                0x20..=0x7e => self.put_character(byte as char),
                _ => self.put_character('?'),
            }
        }
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
            self.surface.rect(x, y, CELL_WIDTH, CELL_HEIGHT, BACKGROUND);
            for (row, bits) in rows.iter().enumerate() {
                for column in 0..8 {
                    if bits & (1 << (7 - column)) != 0 {
                        self.surface.rect(x + column, y + row, 1, 1, WHITE);
                    }
                }
            }
        }
        self.column += 1;
        self.draw_prompt_cursor();
    }

    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.scroll();
            self.row = self.rows - 1;
        }
        self.draw_prompt_cursor();
    }

    fn backspace(&mut self) {
        if self.column == 0 {
            return;
        }
        self.column -= 1;
        let x = self.column * CELL_WIDTH;
        let y = self.row * CELL_HEIGHT;
        // SAFETY: The cursor remains within the console grid.
        unsafe { self.surface.rect(x, y, CELL_WIDTH, CELL_HEIGHT, BACKGROUND) };
        self.draw_prompt_cursor();
    }

    fn scroll(&mut self) {
        let scroll_bytes = CELL_HEIGHT * self.surface.pitch;
        let remaining = self.surface.height.saturating_sub(CELL_HEIGHT);
        // SAFETY: Both ranges are within the validated framebuffer mapping.
        // The ranges overlap, so ptr::copy (memmove semantics) is required.
        // The framebuffer is ordinary mapped RAM, not an I/O register window;
        // using the optimized bulk copy avoids a slow byte-by-byte scan down
        // the display during every terminal scroll.
        unsafe {
            core::ptr::copy(
                self.surface.address.add(scroll_bytes),
                self.surface.address,
                remaining * self.surface.pitch,
            );
        }
        // Clear the newly exposed bottom rows.
        // SAFETY: The rectangle is clipped to the framebuffer bounds.
        unsafe {
            self.surface.rect(
                0,
                self.surface.height.saturating_sub(CELL_HEIGHT),
                self.surface.width,
                CELL_HEIGHT,
                BACKGROUND,
            );
        }
    }

    fn draw_prompt_cursor(&mut self) {
        let x = self.column * CELL_WIDTH;
        let y = self.row * CELL_HEIGHT + CELL_HEIGHT - 2;
        // SAFETY: Cursor coordinates are within the console cell.
        unsafe { self.surface.rect(x, y, 7, 1, WHITE) };
    }
}

impl core::fmt::Write for Console {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        self.write_text(text);
        Ok(())
    }
}
