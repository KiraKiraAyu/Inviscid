use super::{LineEnding, TextBuffer};
use ropey::RopeBuilder;
use std::{path::PathBuf, str::Utf8Error};

/// Owns decoding state across reads; published buffers share immutable Rope and index nodes.
pub(crate) struct StreamingBuffer {
    pub buffer: TextBuffer,
    pending: Vec<u8>,
    pending_cr: bool,
    in_word: bool,
    hash: std::collections::hash_map::DefaultHasher,
}

impl StreamingBuffer {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            buffer: TextBuffer::from_rope(ropey::Rope::new(), LineEnding::Lf, 0, path),
            pending: Vec::new(),
            pending_cr: false,
            in_word: false,
            hash: Default::default(),
        }
    }

    pub fn push(&mut self, bytes: &[u8], eof: bool) -> Result<(), Utf8Error> {
        use std::hash::Hasher;
        self.hash.write(bytes);
        self.pending.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() && !eof => error.valid_up_to(),
            Err(error) => return Err(error),
        };
        let text = std::str::from_utf8(&self.pending[..valid]).unwrap();
        let mut normalized = String::with_capacity(text.len() + 1);
        let mut ending = self.buffer.line_ending;
        let mut words = self.buffer.total_word_count;
        for ch in text.chars() {
            let whitespace = ch.is_whitespace();
            words += usize::from(!whitespace && !self.in_word);
            self.in_word = !whitespace;
            if self.pending_cr {
                self.pending_cr = false;
                if ch == '\n' {
                    normalized.push('\n');
                    ending = LineEnding::CrLf;
                    continue;
                }
                normalized.push('\r');
            }
            if ch == '\r' {
                self.pending_cr = true;
            } else {
                normalized.push(ch);
            }
        }
        self.pending.drain(..valid);
        if eof && self.pending_cr {
            normalized.push('\r');
            self.pending_cr = false;
        }
        let previous = &self.buffer;
        let mut rope = previous.rope.clone();
        rope.insert(rope.len_chars(), &normalized);
        let mut next = TextBuffer::from_rope(rope, ending, words, previous.file_path.clone());
        next.version = previous.version + 1;
        next.revision = previous.revision + 1;
        next.saved_baseline.revision = next.revision;
        let start = if previous.is_empty() {
            0
        } else {
            previous.line_count() - 1
        };
        next.last_edit_delta = Some(super::EditDelta {
            start_line: super::SourceLine::new(start),
            old_line_count: previous.line_count() - start,
            new_line_count: next.line_count() - start,
        });
        next.line_index_cache = previous.line_index_cache.clone();
        drop(next.line_index());
        self.buffer = next;
        Ok(())
    }

    pub fn disk_hash(&self) -> u64 {
        use std::hash::Hasher;
        let mut hash = self.hash.clone();
        // Match Hash for str, including its terminator, without rereading the file.
        hash.write_u8(0xff);
        hash.finish()
    }
}

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
    fn streamed_snapshots_preserve_decoding_hash_and_old_versions() {
        use std::hash::{Hash, Hasher};
        let text = "\u{754c}\r\nword\rnext\n\u{1f642}\r";
        for chunk in 1..=9 {
            let mut stream = super::StreamingBuffer::new(None);
            let mut snapshots = Vec::new();
            for bytes in text.as_bytes().chunks(chunk) {
                stream.push(bytes, false).unwrap();
                snapshots.push((stream.buffer.clone(), stream.buffer.to_string_content()));
            }
            stream.push(&[], true).unwrap();
            let expected = TextBuffer::from_str(text, None);
            assert_eq!(
                stream.buffer.to_string_content(),
                expected.to_string_content()
            );
            assert_eq!(stream.buffer.word_count(), expected.word_count());
            assert_eq!(stream.buffer.line_ending(), expected.line_ending());
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hash);
            assert_eq!(stream.disk_hash(), hash.finish());
            for (snapshot, content) in snapshots {
                assert_eq!(snapshot.to_string_content(), content);
            }
            assert!(!stream.buffer.is_dirty());
        }
        let mut stream = super::StreamingBuffer::new(None);
        stream.push(&[0xf0, 0x9f], false).unwrap();
        assert!(stream.push(&[], true).is_err());
    }

    #[test]
    fn opening_fence_renders_before_closing_fence_arrives() {
        let buffer = TextBuffer::from_str("```rust\n# not a heading\nlet x = 1;", None);
        let (_, rows) = crate::markdown::MarkdownScanner::scan_window(&buffer, 0..3);
        assert!(matches!(
            rows[0].kind,
            crate::markdown::BlockKind::CodeBlock {
                is_fence_start: true
            }
        ));
        assert!(matches!(
            rows[1].kind,
            crate::markdown::BlockKind::CodeBlockContent
        ));
        assert!(matches!(
            rows[2].kind,
            crate::markdown::BlockKind::CodeBlockContent
        ));
    }

    #[test]
    fn long_code_block_does_not_expand_the_requested_window() {
        let text = format!("```text\n{}```\nafter", "# code\n".repeat(100_000));
        let buffer = TextBuffer::from_str(&text, None);
        let (range, rows) = crate::markdown::MarkdownScanner::scan_window(&buffer, 50_000..50_064);
        assert_eq!(range, 50_000..50_064);
        assert_eq!(rows.len(), 64);
        assert!(
            rows.iter()
                .all(|row| matches!(row.kind, crate::markdown::BlockKind::CodeBlockContent))
        );
    }

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
