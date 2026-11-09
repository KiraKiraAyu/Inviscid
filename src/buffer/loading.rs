use super::{LineEnding, TextBuffer};
use ropey::RopeBuilder;
use std::{path::PathBuf, str::Utf8Error};

impl TextBuffer {
    pub fn from_bytes(bytes: &[u8], file_path: Option<PathBuf>) -> Result<Self, Utf8Error> {
        Self::from_bytes_with_chunk_size(bytes, file_path, 16 * 1024)
    }

    fn from_bytes_with_chunk_size(
        bytes: &[u8],
        file_path: Option<PathBuf>,
        chunk_size: usize,
    ) -> Result<Self, Utf8Error> {
        let mut builder = RopeBuilder::new();
        let mut normalized = String::with_capacity(chunk_size + 4);
        let mut offset = 0;
        let mut pending_cr = false;
        let mut line_ending = LineEnding::Lf;
        let mut in_word = false;
        let mut words = 0;
        while offset < bytes.len() {
            let mut end = (offset + chunk_size).min(bytes.len());
            let chunk = loop {
                match std::str::from_utf8(&bytes[offset..end]) {
                    Ok(chunk) => break chunk,
                    Err(error) if error.error_len().is_none() && end < bytes.len() => end += 1,
                    // Return an error with offsets relative to the full input.
                    Err(_) => return Err(std::str::from_utf8(bytes).unwrap_err()),
                }
            };
            normalized.clear();
            for ch in chunk.chars() {
                let whitespace = ch.is_whitespace();
                words += usize::from(!whitespace && !in_word);
                in_word = !whitespace;
                if pending_cr {
                    pending_cr = false;
                    if ch == '\n' {
                        normalized.push('\n');
                        line_ending = LineEnding::CrLf;
                        continue;
                    }
                    normalized.push('\r');
                }
                if ch == '\r' {
                    pending_cr = true;
                } else {
                    normalized.push(ch);
                }
            }
            builder.append(&normalized);
            offset = end;
        }
        if pending_cr {
            builder.append("\r");
        }
        Ok(Self::from_rope(
            builder.finish(),
            line_ending,
            words,
            file_path,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn byte_loading_matches_original_normalization_with_tiny_chunks() {
        let fragments = [
            "word",
            " ",
            "\n",
            "\r\n",
            "\r",
            "\t",
            "\u{754c}",
            "\u{1f642}",
            "\u{2003}",
        ];
        let mut seed = 0x5eed_e002_u64;
        for _ in 0..200 {
            let mut text = String::new();
            for _ in 0..80 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push_str(fragments[(seed >> 32) as usize % fragments.len()]);
            }
            let normalized = text.replace("\r\n", "\n");
            // Original from_str construction, kept independently of the streaming path.
            let oracle = TextBuffer::from_rope(
                ropey::Rope::from_str(&normalized),
                LineEnding::detect(&text),
                normalized.split_whitespace().count(),
                None,
            );
            for chunk_size in 1..=9 {
                let buffer =
                    TextBuffer::from_bytes_with_chunk_size(text.as_bytes(), None, chunk_size)
                        .unwrap();
                assert_eq!(buffer.rope.to_string(), normalized);
                assert_eq!(buffer.line_ending(), LineEnding::detect(&text));
                assert_eq!(buffer.word_count(), normalized.split_whitespace().count());
                assert_eq!(buffer.to_string_content(), oracle.to_string_content());
                assert_eq!(
                    buffer.saved_baseline_snapshot(),
                    oracle.saved_baseline_snapshot()
                );
            }
        }
        for bytes in [
            vec![0xff],
            vec![b'a', 0xf0, 0x9f],
            vec![0xc0, 0x80],
            vec![b'a', 0xe4, 0x20],
        ] {
            for chunk_size in 1..=4 {
                assert!(TextBuffer::from_bytes_with_chunk_size(&bytes, None, chunk_size).is_err());
            }
        }
        for text in ["", "\r", "\r\n", "a\r\nb\r", "\u{754c}\r\n\u{1f642}"] {
            let buffer = TextBuffer::from_bytes(text.as_bytes(), None).unwrap();
            assert_eq!(buffer.rope.to_string(), text.replace("\r\n", "\n"));
            assert_eq!(buffer.line_ending(), LineEnding::detect(text));
        }
    }
}
