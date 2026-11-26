use gpui::*;
use std::path::PathBuf;

use crate::markdown::ImageInsertionTarget;

impl super::Editor {
    pub fn show_notice(&mut self, message: impl Into<String>, is_error: bool, cx: &mut Context<Self>) {
        self.notice_epoch = self.notice_epoch.wrapping_add(1);
        let epoch = self.notice_epoch;
        self.notice = Some(super::EditorNotice {
            message: message.into(),
            is_error,
        });
        cx.notify();

        let executor = cx.background_executor().clone();
        self.notice_task = Some(cx.spawn(async move |this, cx| {
            executor.timer(std::time::Duration::from_secs(4)).await;
            let _ = this.update(cx, |editor, cx| {
                if editor.notice_epoch == epoch {
                    editor.notice = None;
                    editor.notice_task = None;
                    cx.notify();
                }
            });
        }));
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.notice.is_some() {
            self.notice_epoch = self.notice_epoch.wrapping_add(1);
            self.notice = None;
            self.notice_task = None;
            cx.notify();
        }
    }

    pub(crate) fn clear_external_drop_target(&mut self, cx: &mut Context<Self>) {
        if self.external_drop_target.take().is_some() {
            cx.notify();
        }
    }

    /// Resolves the boundary line index (`0..=line_count`) for an external drag position.
    pub(crate) fn resolve_drop_insertion_line(&self, mouse_pos: Point<Pixels>) -> usize {
        let line_count = self.buffer.line_count();
        if line_count == 0 {
            return 0;
        }

        let layout = self.layout_snapshot();
        let viewport = self.viewport_bounds_val();
        let anchor = self.scroll.anchor_line;
        let doc_y = mouse_pos.y - viewport.origin.y + self.scroll.current_scroll_top;

        let first_visible = if layout
            .metrics
            .height(0)
            .is_some_and(|h| h > px(0.0))
        {
            Some(0)
        } else {
            layout.metrics.next_visible_line_after(0)
        };
        if let Some(first) = first_visible
            && let Some(first_top) = layout.metrics.top_relative(first, anchor)
            && doc_y <= first_top
        {
            return 0;
        }

        let last_visible = (0..line_count)
            .rev()
            .find(|&idx| layout.metrics.height(idx).is_some_and(|h| h > px(0.0)));
        if let Some(last) = last_visible
            && let (Some(last_top), Some(last_h)) = (
                layout.metrics.top_relative(last, anchor),
                layout.metrics.height(last),
            )
            && doc_y >= last_top + last_h
        {
            return line_count;
        }

        let raw_idx = layout
            .metrics
            .line_at_relative_y(anchor, doc_y)
            .unwrap_or(0)
            .min(line_count - 1);
        let source_line = crate::buffer::SourceLine::new(raw_idx);

        if self.is_line_folded(source_line)
            && let Some(region) = self
                .display_map
                .fold_map()
                .folded_regions()
                .iter()
                .find(|r| source_line >= r.start_line && source_line <= r.end_line)
        {
            let header_idx = region.start_line.index();
            let top = layout
                .metrics
                .top_relative(header_idx, anchor)
                .unwrap_or(px(0.0));
            let h = layout.metrics.height(header_idx).unwrap_or(px(0.0));
            return if doc_y < top + h / 2.0 {
                header_idx
            } else {
                (region.end_line.index() + 1).min(line_count)
            };
        }

        if let Some(block) = crate::markdown::block_at_buffer(&self.buffer, source_line) {
            match block.kind {
                crate::markdown::ProjectedBlockKind::CodeBlock { .. }
                | crate::markdown::ProjectedBlockKind::MathBlock
                | crate::markdown::ProjectedBlockKind::Table { .. } => {
                    let start_idx = block.source_range.start.index();
                    let end_idx = block.source_range.end.index().min(line_count);
                    let last_vis = (start_idx..end_idx)
                        .rev()
                        .find(|&i| layout.metrics.height(i).is_some_and(|h| h > px(0.0)))
                        .unwrap_or(start_idx);
                    if let (Some(top), Some(last_top), Some(last_h)) = (
                        layout.metrics.top_relative(start_idx, anchor),
                        layout.metrics.top_relative(last_vis, anchor),
                        layout.metrics.height(last_vis),
                    ) {
                        let mid = (top + last_top + last_h) / 2.0;
                        return if doc_y < mid { start_idx } else { end_idx };
                    }
                }
                crate::markdown::ProjectedBlockKind::BlankSeparator => {
                    return raw_idx;
                }
                _ => {}
            }
        }

        let top = layout.metrics.top_relative(raw_idx, anchor).unwrap_or(px(0.0));
        let h = layout.metrics.height(raw_idx).unwrap_or(px(0.0));
        if doc_y < top + h / 2.0 {
            raw_idx
        } else {
            (raw_idx + 1).min(line_count)
        }
    }

    /// Computes the screen-space Y coordinate for the drop indicator line at `before_line`.
    pub(crate) fn drop_indicator_screen_y(
        &self,
        before_line: usize,
        viewport: Bounds<Pixels>,
    ) -> Pixels {
        let line_count = self.buffer.line_count();
        let scroll_top = self.scroll.current_scroll_top;
        if line_count == 0 {
            return viewport.origin.y
                + crate::editor::line_metrics::DOCUMENT_TOP_PADDING
                - scroll_top;
        }

        let layout = self.layout_snapshot();
        let anchor = self.scroll.anchor_line;
        let clamped = before_line.min(line_count);

        let prev_visible = (0..clamped)
            .rev()
            .find(|&idx| layout.metrics.height(idx).is_some_and(|h| h > px(0.0)));
        let next_visible = if clamped < line_count {
            if layout
                .metrics
                .height(clamped)
                .is_some_and(|h| h > px(0.0))
            {
                Some(clamped)
            } else {
                layout.metrics.next_visible_line_after(clamped)
            }
        } else {
            None
        };

        let doc_y = match (prev_visible, next_visible) {
            (Some(prev), Some(next)) => {
                let prev_bottom = layout.metrics.top_relative(prev, anchor).unwrap_or(px(0.0))
                    + layout.metrics.height(prev).unwrap_or(px(0.0));
                let next_top = layout
                    .metrics
                    .top_relative(next, anchor)
                    .unwrap_or(prev_bottom);
                (prev_bottom + next_top) / 2.0
            }
            (None, Some(next)) => layout.metrics.top_relative(next, anchor).unwrap_or(px(24.0)),
            (Some(prev), None) => {
                layout.metrics.top_relative(prev, anchor).unwrap_or(px(0.0))
                    + layout.metrics.height(prev).unwrap_or(px(0.0))
            }
            (None, None) => crate::editor::line_metrics::DOCUMENT_TOP_PADDING,
        };

        viewport.origin.y + doc_y - scroll_top
    }

    pub(crate) fn handle_external_drag_move(
        &mut self,
        event: &DragMoveEvent<ExternalPaths>,
        cx: &mut Context<Self>,
    ) {
        let paths = event.drag(cx).paths().to_vec();
        self.handle_external_drag_paths_move(&paths, event.event.position, event.bounds, cx);
    }

    pub(crate) fn handle_external_drag_paths_move(
        &mut self,
        paths: &[PathBuf],
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit()
            || !paths.iter().any(|p| crate::media::is_supported_image_path(p))
            || !bounds.contains(&position)
        {
            self.clear_external_drop_target(cx);
            return;
        }

        self.apply_drag_edge_scroll(position.y);
        let target = self.resolve_drop_insertion_line(position);
        self.external_drop_target = Some(target);
        cx.notify();
    }

    pub(crate) fn handle_external_drop(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_external_drop_paths(paths.paths(), window, cx);
    }

    pub(crate) fn handle_external_drop_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target_line = self
            .external_drop_target
            .take()
            .unwrap_or_else(|| self.resolve_drop_insertion_line(window.mouse_position()));
        cx.notify();

        if !self.can_edit() {
            return;
        }

        let image_paths: Vec<PathBuf> = paths
            .iter()
            .filter(|path| path.is_file() && crate::media::is_supported_image_path(path))
            .cloned()
            .collect();
        if image_paths.is_empty() {
            return;
        }

        window.focus(&self.focus_handle);
        self.insert_external_image_files(
            image_paths,
            ImageInsertionTarget::AtBoundary(target_line),
            cx,
        );
    }

    pub(crate) fn insert_external_image_files(
        &mut self,
        paths: Vec<PathBuf>,
        target: ImageInsertionTarget,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() || paths.is_empty() {
            return;
        }

        let configured_dir = cx
            .try_global::<crate::config::AppConfig>()
            .map(|cfg| cfg.asset_dir.clone())
            .unwrap_or_else(|| crate::config::UserPreferences::DEFAULT_ASSET_DIR.to_string());
        let doc_path = self.file_path_buf();
        let resolved_dir = match crate::media::resolve_asset_dir(&configured_dir, doc_path.as_deref())
        {
            Ok(dir) => dir,
            Err(err) => {
                self.show_notice(err.to_string(), true, cx);
                return;
            }
        };

        let work = cx.background_executor().spawn(async move {
            let mut saved = Vec::with_capacity(paths.len());
            let mut first_error = None;
            for src in paths {
                match crate::media::store_image_file(&src, &resolved_dir) {
                    Ok(asset) => saved.push(asset),
                    Err(err) if first_error.is_none() => first_error = Some(err.to_string()),
                    Err(_) => {}
                }
            }
            (saved, first_error)
        });

        cx.spawn(async move |this, cx| {
            let (saved, first_error) = work.await;
            let _ = this.update(cx, |editor, cx| {
                editor.apply_saved_images(saved, first_error, doc_path.as_deref(), target, cx);
            });
        })
        .detach();
    }

    pub(crate) fn insert_clipboard_image_bytes(
        &mut self,
        bytes: Vec<u8>,
        format: ImageFormat,
        target: ImageInsertionTarget,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() || bytes.is_empty() {
            return;
        }

        let configured_dir = cx
            .try_global::<crate::config::AppConfig>()
            .map(|cfg| cfg.asset_dir.clone())
            .unwrap_or_else(|| crate::config::UserPreferences::DEFAULT_ASSET_DIR.to_string());
        let doc_path = self.file_path_buf();
        let resolved_dir = match crate::media::resolve_asset_dir(&configured_dir, doc_path.as_deref())
        {
            Ok(dir) => dir,
            Err(err) => {
                self.show_notice(err.to_string(), true, cx);
                return;
            }
        };

        let work = cx.background_executor().spawn(async move {
            crate::media::store_clipboard_image(&bytes, format, &resolved_dir)
                .map_err(|err| err.to_string())
        });

        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |editor, cx| match result {
                Ok(asset) => editor.apply_saved_images(
                    vec![asset],
                    None,
                    doc_path.as_deref(),
                    target,
                    cx,
                ),
                Err(err) => {
                    use crate::i18n::{ActiveLocale, TextKey};
                    let msg = cx.tf(TextKey::EditorSaveImageError, &[("error", &err)]);
                    editor.show_notice(msg, true, cx);
                }
            });
        })
        .detach();
    }

    fn apply_saved_images(
        &mut self,
        saved: Vec<crate::media::SavedImageAsset>,
        first_error: Option<String>,
        expected_doc_path: Option<&std::path::Path>,
        target: ImageInsertionTarget,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() || self.file_path() != expected_doc_path {
            return;
        }

        if !saved.is_empty() {
            let mut markdown_lines = Vec::with_capacity(saved.len());
            for asset in &saved {
                if let Some((w, h)) = asset.dimensions {
                    let key = crate::http::image_size_key(&asset.markdown_url, expected_doc_path);
                    crate::http::record_image_size(&key, w, h);
                }
                markdown_lines.push(crate::media::format_markdown_image_line(
                    &asset.alt_text,
                    &asset.markdown_url,
                ));
            }
            self.perform_edit(cx, |this| {
                crate::markdown::insert_image_blocks(&mut this.buffer, target, &markdown_lines);
            });
        }

        if let Some(err) = first_error {
            use crate::i18n::{ActiveLocale, TextKey};
            let msg = cx.tf(TextKey::EditorSaveImageError, &[("error", &err)]);
            self.show_notice(msg, true, cx);
        }
    }
}
