/// Extracts `(width, height)` from PNG, JPEG, GIF, WebP, or BMP header bytes.
pub fn extract_image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 {
        return None;
    }

    // PNG
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        if w > 0 && h > 0 {
            return Some((w, h));
        }
    }

    // GIF (GIF87a / GIF89a)
    if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        if w > 0 && h > 0 {
            return Some((w, h));
        }
    }

    // WebP (VP8, VP8L, VP8X)
    if bytes.starts_with(b"RIFF") && bytes.len() >= 16 && &bytes[8..12] == b"WEBP" {
        let tag = &bytes[12..16];
        if tag == b"VP8 " && bytes.len() >= 30 {
            let w = (u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF) as u32;
            let h = (u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF) as u32;
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        } else if tag == b"VP8L" && bytes.len() >= 25 {
            let b0 = bytes[21] as u32;
            let b1 = bytes[22] as u32;
            let b2 = bytes[23] as u32;
            let b3 = bytes[24] as u32;
            let w = 1 + (b0 | ((b1 & 0x3F) << 8));
            let h = 1 + ((b1 >> 6) | (b2 << 2) | ((b3 & 0x0F) << 10));
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        } else if tag == b"VP8X" && bytes.len() >= 30 {
            let w = 1 + (bytes[24] as u32 | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16));
            let h = 1 + (bytes[27] as u32 | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16));
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        }
    }

    // JPEG (scan markers until SOS/EOI, skipping APP/EXIF segments)
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            // Skip 0xFF padding bytes
            while i < bytes.len() && bytes[i] == 0xFF {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let marker = bytes[i];
            i += 1;

            // Stop at Start-of-Scan (SOS) or End-of-Image (EOI)
            if marker == 0xDA || marker == 0xD9 {
                break;
            }
            // Standalone markers without payload
            if marker == 0x00 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
                continue;
            }

            // Length-bearing marker: 2-byte big-endian payload length (including length bytes)
            if i + 2 > bytes.len() {
                break;
            }
            let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
            if seg_len < 2 {
                break;
            }

            // Start of Frame markers (SOF0..SOF3, SOF5..SOF7, SOF9..SOF11, SOF13..SOF15)
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
                && seg_len >= 7
                && i + 7 <= bytes.len()
            {
                let h = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                if w > 0 && h > 0 {
                    return Some((w, h));
                }
            }

            i += seg_len;
        }
    }

    // BMP
    if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]).unsigned_abs();
        let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).unsigned_abs();
        if w > 0 && h > 0 {
            return Some((w, h));
        }
    }

    None
}
