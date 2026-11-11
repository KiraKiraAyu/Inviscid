use super::{
    Editor, EditorEvent, ExternalChangeState, Position, TextBuffer, is_whitelisted_extension,
};
use gpui::Context;
use std::{io, path::PathBuf, time::SystemTime};

pub(crate) struct LoadedFile {
    buffer: TextBuffer,
    disk_hash: Option<u64>,
}

impl LoadedFile {
    pub(crate) fn read(path: PathBuf) -> io::Result<Self> {
        let (mut buffer, disk_hash) = if is_whitelisted_extension(&path) {
            let bytes = std::fs::read(&path)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let disk_hash = Editor::hash_file_content(text);
            let buffer = TextBuffer::from_bytes(&bytes, Some(path.clone()))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            (buffer, Some(disk_hash))
        } else {
            (TextBuffer::from_str("", Some(path.clone())), None)
        };
        let mtime: Option<SystemTime> = std::fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        buffer.set_last_saved_mtime(mtime);
        // Build source context while the file is still on the background loader.
        drop(buffer.line_index());
        Ok(Self { buffer, disk_hash })
    }
}

impl Editor {
    pub fn is_loading(&self) -> bool {
        self.is_loading
    }

    pub(crate) fn new_loading(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut editor = Self::new(cx);
        editor.begin_loading(path);
        editor
    }

    pub(crate) fn begin_loading(&mut self, path: PathBuf) {
        self.auto_save_task = None;
        self.file_watch_task = None;
        self.fingerprint_task = None;
        self.buffer.set_file_path(Some(path));
        self.load_error = None;
        self.is_loading = true;
        self.pending_open_position = None;
    }

    pub(crate) fn open_at_position(&mut self, position: Position, cx: &mut Context<Self>) {
        if self.is_loading {
            self.pending_open_position = Some(position);
        } else {
            self.buffer.set_cursor(position);
            self.scroll_to_cursor();
            self.start_scroll_animation_if_needed(cx);
            self.reset_cursor_blink(cx);
        }
    }

    pub(crate) fn finish_loading(
        &mut self,
        result: io::Result<LoadedFile>,
        cx: &mut Context<Self>,
    ) {
        let position = self.pending_open_position.take();
        match result {
            Ok(file) => {
                let find_bar = self.detach_find_bar(cx);
                let mut loaded =
                    Self::new_with_buffer_and_disk_hash(file.buffer, file.disk_hash, cx);
                loaded.focus_handle = self.focus_handle.clone();
                loaded.render_mode = self.render_mode;
                loaded.has_explicit_render_mode = self.has_explicit_render_mode;
                loaded.is_read_only = self.is_read_only;
                loaded.font_size = self.font_size;
                loaded.line_height = self.line_height;
                loaded.soft_wrap = self.soft_wrap;
                loaded.text_system = self.text_system.clone();
                *self = loaded;
                if let Some(bar) = find_bar {
                    self.attach_find_bar(bar, cx);
                }
                if let Some(position) = position {
                    self.open_at_position(position, cx);
                }
            }
            Err(error) => {
                eprintln!("Failed to load file {:?}: {error}", self.buffer.file_path());
                self.is_loading = false;
                self.load_error = Some(format!("Could not open file: {error}"));
            }
        }
        cx.emit(EditorEvent::TitleChanged);
        cx.emit(EditorEvent::DirtyChanged(self.is_dirty()));
        cx.emit(EditorEvent::ExternalChangeChanged(
            ExternalChangeState::None,
        ));
        cx.notify();
    }
}
