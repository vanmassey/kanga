use limine::framebuffer::{Framebuffer, MemoryModel};

const BACKGROUND: (u8, u8, u8) = (10, 16, 28);
const PANEL: (u8, u8, u8) = (19, 30, 47);
const ACCENT: (u8, u8, u8) = (70, 190, 150);
const WHITE: (u8, u8, u8) = (230, 239, 246);
const MUTED: (u8, u8, u8) = (142, 164, 181);

/// Draw Kanga's first graphical boot screen into a Limine RGB framebuffer.
///
/// # Safety
///
/// The framebuffer must be the live framebuffer supplied by Limine and remain
/// mapped and exclusively writable for the duration of this call.
pub unsafe fn draw_boot_screen(framebuffer: &Framebuffer<'_>) -> bool {
    if framebuffer.memory_model() != MemoryModel::RGB || framebuffer.bpp() != 32 {
        return false;
    }

    let width = match usize::try_from(framebuffer.width()) {
        Ok(value) if value > 0 => value,
        _ => return false,
    };
    let height = match usize::try_from(framebuffer.height()) {
        Ok(value) if value > 0 => value,
        _ => return false,
    };
    let Some(minimum_pitch) = width.checked_mul(4) else {
        return false;
    };
    let pitch = match usize::try_from(framebuffer.pitch()) {
        Ok(value) if value >= minimum_pitch => value,
        _ => return false,
    };
    let Some(total_bytes) = pitch.checked_mul(height) else {
        return false;
    };
    if total_bytes == 0 || framebuffer.addr().is_null() {
        return false;
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

    // SAFETY: The caller guarantees the live Limine framebuffer is writable.
    unsafe {
        surface.fill(BACKGROUND);
        let scale = (width / 240).min(height / 150).clamp(1, 4);
        let margin = 24 * scale;
        let panel_x = margin;
        let panel_y = margin;
        let panel_width = width.saturating_sub(margin.saturating_mul(2));
        let panel_height = height.saturating_sub(margin.saturating_mul(2));

        surface.rect(panel_x, panel_y, panel_width, 5 * scale, ACCENT);
        surface.rect(
            panel_x,
            panel_y + 5 * scale,
            panel_width,
            panel_height.saturating_sub(5 * scale),
            PANEL,
        );

        let text_x = panel_x + 28 * scale;
        let mut text_y = panel_y + 28 * scale;
        surface.text(text_x, text_y, "KANGA", scale * 3, ACCENT);
        text_y += 30 * scale;
        surface.text(text_x, text_y, "SYSTEM READY", scale * 2, WHITE);
        text_y += 28 * scale;
        surface.text(text_x, text_y, "BOOTED VIA LIMINE", scale, MUTED);
        text_y += 24 * scale;
        surface.text(text_x, text_y, "MEMORY MANAGER ONLINE", scale, WHITE);
        text_y += 18 * scale;
        surface.text(text_x, text_y, "FRAMEBUFFER OUTPUT ACTIVE", scale, WHITE);

        let footer_y = panel_y + panel_height.saturating_sub(24 * scale);
        surface.text(text_x, footer_y, "KANGA OS / 0.1.0", scale, MUTED);
    }

    true
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

    unsafe fn text(&self, x: usize, y: usize, value: &str, scale: usize, colour: (u8, u8, u8)) {
        let mut cursor = x;
        for character in value.chars() {
            let glyph = glyph(character);
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..5 {
                    if bits & (1 << (4 - column)) != 0 {
                        // SAFETY: rect clips each glyph pixel to the framebuffer.
                        unsafe {
                            self.rect(
                                cursor + column * scale,
                                y + row * scale,
                                scale,
                                scale,
                                colour,
                            );
                        }
                    }
                }
            }
            cursor = cursor.saturating_add(6 * scale);
            if cursor >= self.width {
                break;
            }
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

fn glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        ' ' => [0; 7],
        _ => [31, 17, 1, 2, 4, 0, 4],
    }
}
