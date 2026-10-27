//! Workspace search tab: owns the query, the options, and the lifecycle of the background scan.
//! Rendering lives in the submodules.

mod header;
mod results_list;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedSender;
use gpui::prelude::*;
use gpui::*;

use crate::search::{
    FileSearchResult, ScanLimits, SearchOptions, SearchQuery, SearchToggle, WorkspaceFileCache,
    WorkspaceMatch, WorkspaceScan, WorkspaceSearchResults, replace_all_in_file,
    replace_match_in_file,
};
use crate::theme::ThemeManager;
use crate::ui::text_input::{TextInput, TextInputBg, TextInputEvent};
use crate::ui::tokens::{ControlHeight, Radius, Spacing};

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
/// Streamed results are applied immediately but rendered at most this often.
const RESULTS_FLUSH_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    Idle,
    NoWorkspace,
    InvalidQuery,
    Searching,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultRow {
    FileHeader(usize),
    Match { file_idx: usize, match_idx: usize },
}

pub struct SearchPanel {
    focus_handle: FocusHandle,
    input: Entity<TextInput>,
    replace_input: Entity<TextInput>,
    scroll_handle: UniformListScrollHandle,
    root_dir: Option<PathBuf>,
    file_cache: WorkspaceFileCache,
    prewarm_cancel: Arc<AtomicBool>,
    search_cancel: Arc<AtomicBool>,
    options: SearchOptions,
    status: SearchStatus,
    results: WorkspaceSearchResults,
    collapsed_files: HashSet<PathBuf>,
    /// Flattened view of `results` for the virtual list, rebuilt lazily when `rows_dirty`.
    rows: Vec<ResultRow>,
    rows_dirty: bool,
    show_replace: bool,
    replacing: bool,
    replace_count: Option<usize>,
    replace_error: Option<String>,
    _input_sub: Subscription,
    _replace_sub: Subscription,
}

/// Everything a background search needs, detached from the panel.
struct SearchJob {
    root: PathBuf,
    query: SearchQuery,
    cancel: Arc<AtomicBool>,
    cache: WorkspaceFileCache,
}

impl SearchPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new("", cx)
                .id("workspace_search_input")
                .placeholder("Search in files")
                .height(ControlHeight::SM)
                .radius(Radius::SM)
                .padding_x(Spacing::SM)
                .bg(TextInputBg::Toolbar)
        });
        let replace_input = cx.new(|cx| {
            TextInput::new("", cx)
                .id("workspace_search_replace_input")
                .placeholder("Replace")
                .height(ControlHeight::SM)
                .radius(Radius::SM)
                .padding_x(Spacing::SM)
                .bg(TextInputBg::Toolbar)
        });
        // `escape` never reaches the input as `Cancel`: the `CloseSearchPanel` binding claims it
        // first and closes the panel with access to the window.
        let _input_sub = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| match event {
            TextInputEvent::Changed(_) => this.schedule_search(cx),
            TextInputEvent::Commit(_) => this.search_now(cx),
            TextInputEvent::Cancel => {}
        });
        let _replace_sub =
            cx.subscribe(
                &replace_input,
                |this, _, event: &TextInputEvent, cx| match event {
                    TextInputEvent::Changed(_) => {}
                    TextInputEvent::Commit(_) => this.replace_all(cx),
                    TextInputEvent::Cancel => {}
                },
            );

        Self {
            focus_handle: cx.focus_handle(),
            input,
            replace_input,
            scroll_handle: UniformListScrollHandle::default(),
            root_dir: None,
            file_cache: WorkspaceFileCache::default(),
            prewarm_cancel: Arc::new(AtomicBool::new(false)),
            search_cancel: Arc::new(AtomicBool::new(false)),
            options: SearchOptions::default(),
            status: SearchStatus::Idle,
            results: WorkspaceSearchResults::default(),
            collapsed_files: HashSet::new(),
            rows: Vec::new(),
            rows_dirty: false,
            show_replace: false,
            replacing: false,
            replace_count: None,
            replace_error: None,
            _input_sub,
            _replace_sub,
        }
    }

    pub fn focus_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.select_all();
            cx.notify();
        });
        window.focus(&self.input.read(cx).focus_handle(cx));
    }

    pub fn set_root_dir(&mut self, root_dir: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.root_dir == root_dir {
            return;
        }
        self.root_dir = root_dir;
        self.prewarm_file_cache(cx);
        self.schedule_search(cx);
    }

    /// Drops the file listing and re-runs the current query; call after files were created,
    /// renamed, or deleted.
    pub fn refresh_files(&mut self, cx: &mut Context<Self>) {
        self.file_cache.invalidate();
        self.prewarm_file_cache(cx);
        self.schedule_search(cx);
    }

    pub fn results(&self) -> &WorkspaceSearchResults {
        &self.results
    }

    pub fn status(&self) -> SearchStatus {
        self.status
    }

    pub fn query_text(&self, cx: &App) -> String {
        self.input.read(cx).text()
    }

    pub fn set_query_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text(text, cx));
    }

    pub fn replace_text(&self, cx: &App) -> String {
        self.replace_input.read(cx).text()
    }

    pub fn set_replace_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.replace_input
            .update(cx, |input, cx| input.set_text(text, cx));
    }

    pub fn toggle_replace(&mut self, cx: &mut Context<Self>) {
        self.show_replace = !self.show_replace;
        cx.notify();
    }

    pub fn focus_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_replace = true;
        self.replace_input.update(cx, |input, cx| {
            input.select_all();
            cx.notify();
        });
        window.focus(&self.replace_input.read(cx).focus_handle(cx));
        cx.notify();
    }

    pub fn replace_match(
        &mut self,
        path: PathBuf,
        matched: WorkspaceMatch,
        cx: &mut Context<Self>,
    ) {
        let Some(query) = self.current_query(cx) else {
            return;
        };
        if self.replacing {
            return;
        }
        let replacement = self.replace_text(cx);
        self.start_replacement(cx, move || {
            replace_match_in_file(&path, &query, &replacement, &matched)
                .map(|replaced| if replaced { 1 } else { 0 })
        });
    }

    pub fn replace_all(&mut self, cx: &mut Context<Self>) {
        let Some(query) = self.current_query(cx) else {
            return;
        };
        if self.replacing {
            return;
        }
        let Some(root) = self.root_dir.clone() else {
            return;
        };
        let replacement = self.replace_text(cx);
        let cache = self.file_cache.clone();
        self.start_replacement(cx, move || {
            let cancel = AtomicBool::new(false);
            let files = cache
                .get_or_discover(&root, &cancel)
                .ok_or_else(|| std::io::Error::other("workspace search was cancelled"))?;
            let mut count = 0;
            for path in files.iter() {
                count += replace_all_in_file(&path, &query, &replacement)?;
            }
            Ok(count)
        });
    }

    fn current_query(&self, cx: &App) -> Option<SearchQuery> {
        let text = self.query_text(cx);
        if text.is_empty() {
            None
        } else {
            SearchQuery::new(text, self.options.clone()).ok()
        }
    }

    fn start_replacement(
        &mut self,
        cx: &mut Context<Self>,
        operation: impl FnOnce() -> std::io::Result<usize> + Send + 'static,
    ) {
        self.replacing = true;
        self.replace_count = None;
        self.replace_error = None;
        let task = cx.background_executor().spawn(async move { operation() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.replacing = false;
                match result {
                    Ok(count) => {
                        this.replace_count = Some(count);
                        this.rescan_after_replacement(cx);
                    }
                    Err(error) => {
                        eprintln!("[search] Replacement failed: {error}");
                        this.refresh_files(cx);
                        this.replace_error = Some(error.to_string());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn rescan_after_replacement(&mut self, cx: &mut Context<Self>) {
        self.file_cache.invalidate();
        self.prewarm_file_cache(cx);
        self.start_search(Some(SEARCH_DEBOUNCE), cx);
    }

    /// Walks the tree in the background so the first search does not have to.
    fn prewarm_file_cache(&mut self, cx: &mut Context<Self>) {
        self.prewarm_cancel.store(true, Ordering::Relaxed);
        let Some(root) = self.root_dir.clone() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.prewarm_cancel = cancel.clone();
        let cache = self.file_cache.clone();
        cx.background_executor()
            .spawn(async move {
                cache.get_or_discover(&root, &cancel);
            })
            .detach();
    }

    fn toggle_option(&mut self, toggle: SearchToggle, cx: &mut Context<Self>) {
        let flag = match toggle {
            SearchToggle::CaseSensitive => &mut self.options.case_sensitive,
            SearchToggle::WholeWord => &mut self.options.whole_word,
            SearchToggle::Regex => &mut self.options.use_regex,
        };
        *flag = !*flag;
        self.schedule_search(cx);
    }

    fn is_option_on(&self, toggle: SearchToggle) -> bool {
        match toggle {
            SearchToggle::CaseSensitive => self.options.case_sensitive,
            SearchToggle::WholeWord => self.options.whole_word,
            SearchToggle::Regex => self.options.use_regex,
        }
    }

    fn clear_query(&mut self, cx: &mut Context<Self>) {
        // The input reports the change, which resets the results through `schedule_search`.
        self.set_query_text("", cx);
    }

    fn toggle_file_collapsed(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.collapsed_files.remove(path) {
            self.collapsed_files.insert(path.to_path_buf());
        }
        self.rows_dirty = true;
        cx.notify();
    }

    fn set_all_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if collapsed {
            self.collapsed_files
                .extend(self.results.files().iter().map(|f| f.path.clone()));
        } else {
            self.collapsed_files.clear();
        }
        self.rows_dirty = true;
        cx.notify();
    }

    fn is_all_collapsed(&self) -> bool {
        !self.results.is_empty()
            && self
                .results
                .files()
                .iter()
                .all(|f| self.collapsed_files.contains(&f.path))
    }

    fn ensure_rows(&mut self) {
        if !self.rows_dirty {
            return;
        }
        self.rows_dirty = false;
        self.rows.clear();
        for (file_idx, file) in self.results.files().iter().enumerate() {
            self.rows.push(ResultRow::FileHeader(file_idx));
            if !self.collapsed_files.contains(&file.path) {
                self.rows
                    .extend((0..file.matches.len()).map(|match_idx| ResultRow::Match {
                        file_idx,
                        match_idx,
                    }));
            }
        }
    }
}

// Search lifecycle. Each search owns a cancel flag; starting a new one cancels the previous one,
// which is how a superseded search knows to drop its results.
impl SearchPanel {
    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        self.replace_count = None;
        self.replace_error = None;
        self.start_search(Some(SEARCH_DEBOUNCE), cx);
    }

    fn search_now(&mut self, cx: &mut Context<Self>) {
        self.start_search(None, cx);
    }

    fn start_search(&mut self, debounce: Option<Duration>, cx: &mut Context<Self>) {
        let job = self.begin_search(cx);
        cx.notify();
        let Some(job) = job else {
            return;
        };
        cx.spawn(async move |this, cx| {
            if let Some(delay) = debounce {
                cx.background_executor().timer(delay).await;
            }
            Self::run_search(this, job, cx).await;
        })
        .detach();
    }

    /// Cancels the running search, resets the results, and validates the new one.
    fn begin_search(&mut self, cx: &App) -> Option<SearchJob> {
        self.search_cancel.store(true, Ordering::Relaxed);
        self.results = WorkspaceSearchResults::default();
        self.rows_dirty = true;

        let text = self.query_text(cx);
        if text.is_empty() {
            self.status = SearchStatus::Idle;
            return None;
        }
        let Some(root) = self.root_dir.clone() else {
            self.status = SearchStatus::NoWorkspace;
            return None;
        };
        let Ok(query) = SearchQuery::new(text, self.options.clone()) else {
            self.status = SearchStatus::InvalidQuery;
            return None;
        };

        self.status = SearchStatus::Searching;
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = cancel.clone();
        Some(SearchJob {
            root,
            query,
            cancel,
            cache: self.file_cache.clone(),
        })
    }

    async fn run_search(this: WeakEntity<Self>, job: SearchJob, cx: &mut AsyncApp) {
        if job.cancel.load(Ordering::Relaxed) {
            return;
        }
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let cancel = job.cancel.clone();
        let executor = cx.background_executor().clone();
        let scan_task = cx
            .background_executor()
            .spawn(async move { scan_workspace(job, executor, tx).await });

        let mut last_flush: Option<std::time::Instant> = None;
        while let Some(result) = rx.next().await {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let now = cx.background_executor().now();
            let flush = last_flush.is_none_or(|t| now.duration_since(t) >= RESULTS_FLUSH_INTERVAL);
            if flush {
                last_flush = Some(now);
            }
            let applied = this.update(cx, |this, cx| {
                this.push_result(result);
                if flush {
                    cx.notify();
                }
            });
            if applied.is_err() {
                break;
            }
        }
        // Dropping the receiver stops any worker still scanning.
        drop(rx);

        let is_capped = scan_task.await;
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        // The panel may already be gone; then there is nothing left to finish.
        let _ = this.update(cx, |this, cx| this.finish_search(is_capped, cx));
    }

    fn push_result(&mut self, result: FileSearchResult) {
        self.results.insert(result);
        self.rows_dirty = true;
    }

    fn finish_search(&mut self, is_capped: bool, cx: &mut Context<Self>) {
        if is_capped {
            self.results.mark_capped();
        }
        self.status = SearchStatus::Done;
        self.rows_dirty = true;
        cx.notify();
    }
}

/// Lists the files and scans them on the background executor, one worker per CPU. Returns
/// whether the results were capped.
async fn scan_workspace(
    job: SearchJob,
    executor: BackgroundExecutor,
    tx: UnboundedSender<FileSearchResult>,
) -> bool {
    let Some(files) = job.cache.get_or_discover(&job.root, &job.cancel) else {
        return false;
    };
    let scan = WorkspaceScan::new(
        &files,
        &job.root,
        &job.query,
        &job.cancel,
        ScanLimits::default(),
    );
    let workers = executor.num_cpus().min(files.len()).max(1);
    executor
        .scoped(|scope| {
            for _ in 0..workers {
                let tx = tx.clone();
                let scan = &scan;
                scope.spawn(async move {
                    scan.run_worker(|result| tx.unbounded_send(result).is_ok());
                });
            }
        })
        .await;
    scan.is_capped()
}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_rows();
        let theme = cx.global::<ThemeManager>().theme().clone();
        let body = if self.results.is_empty() {
            results_list::render_empty_state(self.empty_message(cx), &theme).into_any_element()
        } else {
            self.render_results_list(cx).into_any_element()
        };

        div()
            .id("search_panel")
            .key_context(crate::ui::key_context::SEARCH_PANEL)
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.render_header(&theme, cx))
            .child(body)
    }
}

impl SearchPanel {
    fn empty_message(&self, cx: &App) -> &'static str {
        match self.status {
            SearchStatus::Idle => "Search in workspace files",
            SearchStatus::NoWorkspace => "Open a folder to search its files",
            SearchStatus::InvalidQuery => "Invalid regular expression",
            SearchStatus::Searching => "Searching files...",
            SearchStatus::Done if self.query_text(cx).is_empty() => "Search in workspace files",
            SearchStatus::Done => "No matches found",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    // Load-bearing: the parent module's glob also brings in `gpui::test`, so this explicit
    // import shadows it and keeps the `#[test]` that `#[gpui::test]` expands to resolvable.
    use core::prelude::v1::test;
    use std::fs;

    fn init_globals(cx: &mut TestAppContext) {
        let config = AppConfig::default().with_theme("Dracula");
        let theme_manager = ThemeManager::from_config(&config);
        cx.update(|cx| {
            cx.set_global(theme_manager);
            cx.set_global(config);
            crate::app::actions::bind_default_bindings(cx);
        });
    }

    fn settle(cx: &mut VisualTestContext) {
        cx.background_executor.advance_clock(SEARCH_DEBOUNCE * 2);
        cx.run_until_parked();
    }

    fn relative_paths(panel: &SearchPanel) -> Vec<String> {
        panel
            .results()
            .files()
            .iter()
            .map(|f| f.relative_path.clone())
            .collect()
    }

    #[gpui::test]
    fn test_search_panel_initial_state(cx: &mut TestAppContext) {
        init_globals(cx);
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.read_with(cx, |panel, cx| {
            assert_eq!(panel.query_text(cx), "");
            assert_eq!(panel.status(), SearchStatus::Idle);
            assert!(panel.results().is_empty());
        });
    }

    #[gpui::test]
    fn test_search_panel_toggle_options(cx: &mut TestAppContext) {
        init_globals(cx);
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.update(cx, |p, cx| {
            for toggle in [
                SearchToggle::CaseSensitive,
                SearchToggle::WholeWord,
                SearchToggle::Regex,
            ] {
                p.toggle_option(toggle, cx);
                assert!(p.is_option_on(toggle));
            }
        });
    }

    #[gpui::test]
    fn test_search_panel_collapse_expand_files(cx: &mut TestAppContext) {
        init_globals(cx);
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));
        let path = PathBuf::from("/test/file.md");

        panel.update(cx, |p, cx| p.toggle_file_collapsed(&path, cx));
        panel.read_with(cx, |p, _| assert!(p.collapsed_files.contains(&path)));

        panel.update(cx, |p, cx| p.toggle_file_collapsed(&path, cx));
        panel.read_with(cx, |p, _| assert!(!p.collapsed_files.contains(&path)));
    }

    #[gpui::test]
    fn test_search_panel_results_are_sorted_and_complete(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        for name in ["c.txt", "a.txt", "b.md"] {
            fs::write(root.join(name), "apple banana apple").unwrap();
        }
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.update(cx, |p, cx| {
            p.set_root_dir(Some(root.to_path_buf()), cx);
            p.set_query_text("apple", cx);
        });
        settle(cx);

        panel.read_with(cx, |p, _| {
            assert_eq!(p.status(), SearchStatus::Done);
            assert_eq!(relative_paths(p), ["a.txt", "b.md", "c.txt"]);
            assert_eq!(p.results().total_matches(), 6);
            assert!(!p.results().is_capped());
        });
    }

    #[gpui::test]
    fn test_search_panel_refresh_picks_up_new_files(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("a.txt"), "needle").unwrap();
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.update(cx, |p, cx| {
            p.set_root_dir(Some(root.to_path_buf()), cx);
            p.set_query_text("needle", cx);
        });
        settle(cx);
        panel.read_with(cx, |p, _| assert_eq!(relative_paths(p), ["a.txt"]));

        fs::write(root.join("b.txt"), "needle").unwrap();
        panel.update(cx, |p, cx| p.refresh_files(cx));
        settle(cx);

        panel.read_with(cx, |p, _| assert_eq!(relative_paths(p), ["a.txt", "b.txt"]));
    }

    #[gpui::test]
    fn test_search_panel_invalid_regex(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.update(cx, |p, cx| {
            p.set_root_dir(Some(temp_dir.path().to_path_buf()), cx);
            p.toggle_option(SearchToggle::Regex, cx);
            p.set_query_text("[invalid_regex", cx);
        });
        settle(cx);

        panel.read_with(cx, |p, _| {
            assert_eq!(p.status(), SearchStatus::InvalidQuery);
            assert!(p.results().is_empty());
        });
    }

    #[gpui::test]
    fn test_search_panel_replace_all_updates_workspace_files(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let first = root.join("a.txt");
        let second = root.join("b.md");
        fs::write(&first, "apple apple\n").unwrap();
        fs::write(&second, "apple\n").unwrap();
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.update(cx, |p, cx| {
            p.set_root_dir(Some(root.to_path_buf()), cx);
            p.set_query_text("apple", cx);
            p.set_replace_text("pear", cx);
        });
        settle(cx);
        panel.update(cx, |p, cx| p.replace_all(cx));
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        assert_eq!(fs::read_to_string(&first).unwrap(), "pear pear\n");
        assert_eq!(fs::read_to_string(&second).unwrap(), "pear\n");
    }

    #[gpui::test]
    fn test_search_panel_replace_match_updates_only_selected_hit(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let path = root.join("a.txt");
        fs::write(&path, "apple apple\n").unwrap();
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.update(cx, |p, cx| {
            p.set_root_dir(Some(root.to_path_buf()), cx);
            p.set_query_text("apple", cx);
            p.set_replace_text("pear", cx);
        });
        settle(cx);
        let matched = panel.read_with(cx, |p, _| p.results().files()[0].matches[0].clone());
        panel.update(cx, |p, cx| p.replace_match(path.clone(), matched, cx));
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        assert_eq!(fs::read_to_string(&path).unwrap(), "pear apple\n");
    }

    #[gpui::test]
    fn test_search_panel_without_workspace(cx: &mut TestAppContext) {
        init_globals(cx);
        let (panel, cx) = cx.add_window_view(|_window, cx| SearchPanel::new(cx));

        panel.update(cx, |p, cx| p.set_query_text("anything", cx));
        settle(cx);

        panel.read_with(cx, |p, _| assert_eq!(p.status(), SearchStatus::NoWorkspace));
    }
}
