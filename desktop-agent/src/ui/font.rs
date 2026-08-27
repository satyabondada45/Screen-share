// High-legibility 8x13 and scalable bitmap font renderer for Minifb UI

pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 13;

// Basic ASCII font bitmaps 32..127 (8x13)
include!("font_data.rs");

pub fn draw_char(buffer: &mut [u32], buf_w: usize, buf_h: usize, x: usize, y: usize, c: char, color: u32, scale: usize) {
    let ascii = c as usize;
    if ascii < 32 || ascii > 126 {
        return;
    }
    let glyph_idx = ascii - 32;
    let glyph = &FONT_GLYPHS[glyph_idx];

    for row in 0..13 {
        let bits = glyph[row];
        for col in 0..8 {
            if (bits & (0x80 >> col)) != 0 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + col * scale + sx;
                        let py = y + row * scale + sy;
                        if px < buf_w && py < buf_h {
                            buffer[py * buf_w + px] = color;
                        }
                    }
                }
            }
        }
    }
}

pub fn draw_text(buffer: &mut [u32], buf_w: usize, buf_h: usize, x: usize, y: usize, text: &str, color: u32, scale: usize) {
    let char_w = FONT_WIDTH * scale;
    let mut cur_x = x;
    let mut cur_y = y;

    for c in text.chars() {
        if c == '\n' {
            cur_x = x;
            cur_y += (FONT_HEIGHT + 2) * scale;
            continue;
        }
        draw_char(buffer, buf_w, buf_h, cur_x, cur_y, c, color, scale);
        cur_x += char_w;
    }
}

pub fn text_width(text: &str, scale: usize) -> usize {
    text.len() * FONT_WIDTH * scale
}
