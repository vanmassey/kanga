//! A small, self-contained Kanga boot mascot rendered directly to the Limine framebuffer.
//! The artwork is drawn with integer-scaled primitives, so it needs no heap or image decoder.

use limine::framebuffer::MemoryModel;
use limine::request::FramebufferRequest;

#[used]
#[link_section = ".limine_requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

const DESIGN_W: i32 = 160;
const DESIGN_H: i32 = 200;

struct Canvas {
    address: *mut u8,
    width: i32,
    height: i32,
    pitch: usize,
    scale: i32,
    left: i32,
    top: i32,
    red_shift: u8,
    green_shift: u8,
    blue_shift: u8,
}

impl Canvas {
    fn pixel(&mut self, x: i32, y: i32, colour: u32) {
        if x < 0 || y < 0 || x >= DESIGN_W || y >= DESIGN_H {
            return;
        }
        let base_x = self.left + x * self.scale;
        let base_y = self.top + y * self.scale;
        let pixel = ((colour >> 16) & 0xff) << self.red_shift
            | ((colour >> 8) & 0xff) << self.green_shift
            | (colour & 0xff) << self.blue_shift;
        for sy in 0..self.scale {
            let py = base_y + sy;
            if py < 0 || py >= self.height {
                continue;
            }
            for sx in 0..self.scale {
                let px = base_x + sx;
                if px < 0 || px >= self.width {
                    continue;
                }
                let offset = py as usize * self.pitch + px as usize * 4;
                // SAFETY: Coordinates are clipped to the reported framebuffer dimensions.
                unsafe {
                    self.address.add(offset).cast::<u32>().write_volatile(pixel);
                }
            }
        }
    }

    fn ellipse(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, colour: u32) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let rx2 = (rx * rx) as i64;
        let ry2 = (ry * ry) as i64;
        let limit = rx2 * ry2;
        for y in -ry..=ry {
            for x in -rx..=rx {
                let xx = x as i64;
                let yy = y as i64;
                if xx * xx * ry2 + yy * yy * rx2 <= limit {
                    self.pixel(cx + x, cy + y, colour);
                }
            }
        }
    }

    fn outlined_ellipse(
        &mut self,
        cx: i32,
        cy: i32,
        rx: i32,
        ry: i32,
        outline: u32,
        fill: u32,
    ) {
        self.ellipse(cx, cy, rx, ry, outline);
        self.ellipse(cx, cy, rx - 2, ry - 2, fill);
    }

    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, colour: u32) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.pixel(x, y, colour);
            if x == x1 && y == y1 {
                break;
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
    }
}

/// Draw the Kanga mascot in the centre of the first RGB 32-bit Limine framebuffer.
/// Returns false when the bootloader did not provide a compatible framebuffer.
pub fn draw() -> bool {
    // SAFETY: Limine owns the response and framebuffer mapping for the kernel lifetime.
    let Some(response) = (unsafe { FRAMEBUFFER_REQUEST.get_response() }) else {
        return false;
    };
    // SAFETY: The response was supplied by Limine and remains mapped.
    let Some(framebuffers) = (unsafe { response.framebuffers() }) else {
        return false;
    };
    let Some(framebuffer) = framebuffers.first() else {
        return false;
    };
    if framebuffer.bpp() != 32 || framebuffer.memory_model() != MemoryModel::RGB {
        return false;
    }

    let width = framebuffer.width().min(i32::MAX as u64) as i32;
    let height = framebuffer.height().min(i32::MAX as u64) as i32;
    let pitch = framebuffer.pitch() as usize;
    let scale = (width / DESIGN_W).min(height / DESIGN_H).min(4);
    if scale < 1 || pitch < width as usize * 4 {
        return false;
    }

    let mut canvas = Canvas {
        address: framebuffer.addr(),
        width,
        height,
        pitch,
        scale,
        left: (width - DESIGN_W * scale) / 2,
        top: (height - DESIGN_H * scale) / 2,
        red_shift: framebuffer.red_mask_shift(),
        green_shift: framebuffer.green_mask_shift(),
        blue_shift: framebuffer.blue_mask_shift(),
    };

    const BLACK: u32 = 0x101018;
    const ORANGE: u32 = 0xf07818;
    const ORANGE_LIGHT: u32 = 0xffa13a;
    const CREAM: u32 = 0xffdfaa;
    const PINK: u32 = 0xf4a18e;
    const NAVY: u32 = 0x173c82;
    const NAVY_LIGHT: u32 = 0x2859a6;
    const BROWN: u32 = 0x38251f;
    const WHITE: u32 = 0xffffff;

    // Ears, with dark outlines and warm inner colour.
    canvas.outlined_ellipse(48, 31, 16, 37, BLACK, ORANGE);
    canvas.ellipse(49, 32, 9, 29, CREAM);
    canvas.ellipse(50, 31, 6, 25, PINK);
    canvas.outlined_ellipse(112, 31, 16, 37, BLACK, ORANGE);
    canvas.ellipse(111, 32, 9, 29, CREAM);
    canvas.ellipse(110, 31, 6, 25, PINK);

    // Tail behind the body.
    canvas.outlined_ellipse(137, 157, 25, 13, BLACK, ORANGE);
    canvas.ellipse(139, 155, 18, 6, ORANGE_LIGHT);

    // Body and hoodie.
    canvas.outlined_ellipse(80, 148, 54, 48, BLACK, NAVY);
    canvas.ellipse(80, 128, 44, 21, NAVY_LIGHT);
    canvas.outlined_ellipse(80, 163, 29, 30, BLACK, CREAM);
    canvas.outlined_ellipse(43, 151, 18, 35, BLACK, NAVY_LIGHT);
    canvas.outlined_ellipse(117, 151, 18, 35, BLACK, NAVY_LIGHT);
    canvas.outlined_ellipse(48, 181, 20, 15, BLACK, BROWN);
    canvas.outlined_ellipse(112, 181, 20, 15, BLACK, BROWN);

    // Head and muzzle.
    canvas.outlined_ellipse(80, 77, 44, 43, BLACK, ORANGE);
    canvas.ellipse(63, 52, 17, 16, ORANGE_LIGHT);
    canvas.outlined_ellipse(80, 91, 35, 20, BLACK, CREAM);

    // Eyes and pupils.
    canvas.outlined_ellipse(64, 72, 11, 17, BLACK, WHITE);
    canvas.outlined_ellipse(96, 72, 11, 17, BLACK, WHITE);
    canvas.ellipse(65, 73, 5, 10, BLACK);
    canvas.ellipse(95, 73, 5, 10, BLACK);
    canvas.ellipse(67, 68, 2, 3, WHITE);
    canvas.ellipse(97, 68, 2, 3, WHITE);

    // Nose and a simple friendly smile.
    canvas.outlined_ellipse(80, 91, 12, 8, BLACK, BROWN);
    canvas.line(80, 99, 80, 104, BLACK);
    canvas.line(80, 104, 88, 108, BLACK);
    canvas.line(88, 108, 98, 106, BLACK);

    // Hoodie collar, drawstrings and the K mark.
    canvas.line(59, 119, 80, 133, BLACK);
    canvas.line(80, 133, 101, 119, BLACK);
    canvas.line(64, 124, 64, 145, WHITE);
    canvas.line(96, 124, 96, 145, WHITE);
    canvas.line(74, 145, 74, 158, WHITE);
    canvas.line(74, 145, 88, 145, WHITE);
    canvas.line(88, 145, 77, 158, WHITE);
    canvas.line(77, 158, 91, 158, WHITE);

    true
}
