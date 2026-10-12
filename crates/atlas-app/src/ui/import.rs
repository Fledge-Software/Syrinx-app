//! Lets the owner pick an exported bookmarks file, review it and confirm the import (P1-22).

use std::{fs::File, io::Read, path::Path, thread};

use atlas_core::{
    BookmarkImport, ImportCommit, ImportError, ImportLimits, ImportSummary, SkipReason,
    import_folder_title, parse_bookmark_html,
};
use atlas_platform::file_dialog::choose_file;
use atlas_ui::metrics::{radius, space, text};
use futures::channel::{
    mpsc::{UnboundedSender, unbounded},
    oneshot,
};

use super::*;

/// Skipped entries named in the preview; the rest are only counted.
const LISTED_SKIPS: usize = 50;
const PREVIEW_WIDTH: f32 = 560.;
const PREVIEW_HEIGHT: f32 = 540.;

/// The palette's command words (the spec's `>` command prefix), compared without case.
pub(super) fn is_import_command(input: &str) -> bool {
    let Some(command) = input.trim().strip_prefix('>') else {
        return false;
    };
    let words: Vec<String> = command.split_whitespace().map(str::to_lowercase).collect();
    words == ["import", "bookmarks"]
}

/// Where the import is; only one runs at a time.
#[derive(Default)]
pub(super) struct BookmarkImporter {
    stage: Stage,
    sequence: u64,
}

#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    Choosing,
    Previewing(WindowHandle<Root>),
    Writing {
        request_id: u64,
        folder_title: String,
    },
}

/// The file the owner chose, parsed off the interface thread.
enum Prepared {
    Cancelled,
    Failed(String),
    Ready {
        import: Box<BookmarkImport>,
        folder_title: String,
    },
}

enum PreviewEvent {
    Confirm {
        import: Box<BookmarkImport>,
        folder_title: String,
    },
    Cancel,
}

impl BrowserView {
    /// Opens Windows' file dialog on its own thread, then the preview once the file is parsed.
    pub(super) fn start_bookmark_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        if !matches!(self.importer.stage, Stage::Idle) {
            self.message = "A bookmark import is already in progress.".to_owned();
            return;
        }
        let (sender, receiver) = oneshot::channel();
        let owner = self.parent_hwnd;
        let spawned = thread::Builder::new()
            .name("atlas-bookmark-import".to_owned())
            .spawn(move || {
                if sender.send(prepare(owner)).is_err() {
                    eprintln!("The bookmark file was chosen after its window closed.");
                }
            });
        if spawned.is_err() {
            self.message = "Cannot open the file dialog.".to_owned();
            return;
        }
        self.importer.stage = Stage::Choosing;
        self.message = "Choose an exported bookmarks file".to_owned();
        cx.spawn_in(window, async move |view, cx| {
            let prepared = receiver.await.unwrap_or_else(|_| {
                Prepared::Failed("The import stopped unexpectedly.".to_owned())
            });
            if view
                .update_in(cx, |view, window, cx| {
                    view.bookmarks_prepared(prepared, window, cx)
                })
                .is_err()
            {
                eprintln!("The bookmark preview arrived after the browser window closed.");
            }
        })
        .detach();
    }

    fn bookmarks_prepared(
        &mut self,
        prepared: Prepared,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.importer.stage = Stage::Idle;
        match prepared {
            Prepared::Cancelled => self.message = "Bookmark import cancelled".to_owned(),
            Prepared::Failed(reason) => self.message = reason,
            Prepared::Ready {
                import,
                folder_title,
            } if !self.closing => self.open_import_preview(import, folder_title, window, cx),
            Prepared::Ready { .. } => {}
        }
        cx.notify();
    }

    fn open_import_preview(
        &mut self,
        import: Box<BookmarkImport>,
        folder_title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (events, mut receiver) = unbounded();
        let parent = self.parent_hwnd;
        let close_events = events.clone();
        let opened = cx.open_window(preview_options(window), move |popup, cx| {
            popup.set_window_title("Import bookmarks");
            if let Err(error) = native_handle(popup).and_then(|handle| {
                set_overlay_owner(handle, parent)
                    .map_err(|_| "Cannot keep the preview above Atlas.")
            }) {
                eprintln!("{error}");
            }
            popup.on_window_should_close(cx, move |_, _| {
                if close_events.unbounded_send(PreviewEvent::Cancel).is_err() {
                    eprintln!("The import preview closed after the browser window.");
                }
                true
            });
            let preview = cx.new(|_| ImportPreview {
                import: Some(import),
                folder_title,
                events,
            });
            cx.new(|cx| Root::new(preview, popup, cx))
        });
        let Ok(handle) = opened else {
            self.message = "Cannot open the import preview.".to_owned();
            return;
        };
        self.importer.stage = Stage::Previewing(handle);
        self.message = "Review the import, then confirm or cancel".to_owned();
        cx.spawn_in(window, async move |view, cx| {
            let Some(event) = receiver.next().await else {
                return;
            };
            if view
                .update_in(cx, |view, window, cx| view.preview_event(event, window, cx))
                .is_err()
            {
                eprintln!("The import choice arrived after the browser window closed.");
            }
        })
        .detach();
    }

    fn preview_event(&mut self, event: PreviewEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.importer.stage = Stage::Idle;
        match event {
            PreviewEvent::Cancel => self.message = "Bookmark import cancelled".to_owned(),
            PreviewEvent::Confirm {
                import,
                folder_title,
            } => self.confirm_import(import, folder_title),
        }
        self.focus_page(window);
        cx.notify();
    }

    fn confirm_import(&mut self, import: Box<BookmarkImport>, folder_title: String) {
        if self.closing {
            return;
        }
        self.importer.sequence = self
            .importer
            .sequence
            .checked_add(1)
            .expect("bookmark import request IDs exhausted");
        let request_id = self.importer.sequence;
        self.importer.stage = Stage::Writing {
            request_id,
            folder_title: folder_title.clone(),
        };
        self.message = "Importing bookmarks…".to_owned();
        self.command(SessionCommand::ImportBookmarks {
            request_id,
            import,
            folder_title,
        });
    }

    /// Reports the committed import, or why nothing was written.
    pub(super) fn bookmarks_imported(
        &mut self,
        request_id: u64,
        result: Result<ImportCommit, session::SessionError>,
    ) {
        let Stage::Writing {
            request_id: pending,
            folder_title,
        } = std::mem::take(&mut self.importer.stage)
        else {
            return;
        };
        if pending != request_id {
            return;
        }
        self.message = match result {
            Ok(commit) => imported_message(&commit, &folder_title),
            Err(error) => format!("Nothing was imported: {error}"),
        };
    }

    /// Closes an open preview without importing, as when Atlas closes.
    pub(super) fn close_import_preview(&mut self, cx: &mut Context<Self>) {
        let Stage::Previewing(handle) = std::mem::take(&mut self.importer.stage) else {
            return;
        };
        let handle: gpui::AnyWindowHandle = handle.into();
        if cx.windows().contains(&handle)
            && handle
                .update(cx, |_, window, _| window.remove_window())
                .is_err()
        {
            self.fail("Cannot close the import preview during shutdown.");
        }
    }
}

fn preview_options(window: &Window) -> WindowOptions {
    let main = window.bounds();
    let width = f32::from(main.size.width).min(PREVIEW_WIDTH);
    let origin = point(
        main.origin.x + px((f32::from(main.size.width) - width) / 2.),
        main.origin.y + px(80.),
    );
    WindowOptions {
        kind: WindowKind::PopUp,
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            origin,
            size(px(width), px(PREVIEW_HEIGHT)),
        ))),
        focus: true,
        ..Default::default()
    }
}

/// Runs on the import thread: dialog, bounded read, parse. Logs nothing from the file.
fn prepare(owner: isize) -> Prepared {
    let path = match choose_file("Import bookmarks", owner) {
        Ok(Some(path)) => path,
        Ok(None) => return Prepared::Cancelled,
        Err(error) => return Prepared::Failed(format!("{error}.")),
    };
    let limit = ImportLimits::default().max_input_bytes;
    let bytes = match read_bounded(&path, limit) {
        Ok(bytes) => bytes,
        Err(message) => return Prepared::Failed(message),
    };
    match parse_bookmark_html(&bytes) {
        Ok(import) => Prepared::Ready {
            import: Box::new(import),
            folder_title: import_folder_title(path.file_stem().and_then(|stem| stem.to_str())),
        },
        Err(error) => Prepared::Failed(refusal_message(error)),
    }
}

/// Reads at most one byte past `limit`, so an oversized file is refused without loading it.
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let unreadable = |_| "Cannot read the chosen file.".to_owned();
    let file = File::open(path).map_err(unreadable)?;
    let length = file.metadata().map_err(unreadable)?.len();
    if !usize::try_from(length).is_ok_and(|length| length <= limit) {
        return Err(format!(
            "The file is too large to import (the limit is {} MiB).",
            limit / (1024 * 1024)
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    Ok(bytes)
}

fn refusal_message(error: ImportError) -> String {
    let advice = match error {
        ImportError::UnsupportedEncoding | ImportError::NotUtf8 { .. } => {
            " Export your bookmarks again from Chrome, Edge or Firefox; Atlas reads UTF-8 files only."
        }
        ImportError::NotABookmarkFile => {
            " Choose the HTML file your browser's \"Export bookmarks\" command saved."
        }
        _ => "",
    };
    format!("Cannot import this file: {error}.{advice}")
}

fn imported_message(commit: &ImportCommit, folder_title: &str) -> String {
    let mut message = format!(
        "Imported {} saved pages and {} folders into \u{201C}{folder_title}\u{201D}.",
        commit.tree.pages, commit.tree.folders
    );
    if !commit.skipped.is_empty() {
        message.push_str(&format!(
            " {} more addresses were refused and left out.",
            commit.skipped.len()
        ));
    }
    message
}

/// One skip reason with its count and the first few names, for the preview.
#[derive(Debug, Eq, PartialEq)]
struct SkipGroup {
    reason: String,
    count: usize,
    examples: Vec<String>,
}

/// Groups skipped entries by reason, in order of first appearance, naming up to `listed`.
fn skip_groups(summary: &ImportSummary, listed: usize) -> Vec<SkipGroup> {
    let mut reasons: Vec<&SkipReason> = Vec::new();
    let mut groups: Vec<SkipGroup> = Vec::new();
    let mut named = 0;
    for skipped in &summary.skipped {
        let index = match reasons.iter().position(|reason| **reason == skipped.reason) {
            Some(index) => index,
            None => {
                reasons.push(&skipped.reason);
                groups.push(SkipGroup {
                    reason: skipped.reason.to_string(),
                    count: 0,
                    examples: Vec::new(),
                });
                groups.len() - 1
            }
        };
        groups[index].count += 1;
        if named < listed {
            named += 1;
            groups[index]
                .examples
                .push(skip_example(skipped.line, &skipped.title));
        }
    }
    groups
}

fn skip_example(line: usize, title: &str) -> String {
    if title.is_empty() {
        return format!("Line {line}");
    }
    format!("Line {line}: {title}")
}

/// The counts shown before confirming, one line each; zero-valued notes are left out.
fn summary_lines(summary: &ImportSummary) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{} pages in {} folders ({} empty)",
            summary.bookmarks, summary.folders, summary.empty_folders
        ),
        format!("Folders nest up to {} levels deep", summary.max_depth),
    ];
    for (count, note) in [
        (
            summary.duplicate_addresses,
            "pages repeat an earlier address and are kept",
        ),
        (summary.truncated_titles, "long titles will be shortened"),
        (summary.unclosed_lists, "unclosed folder lists were closed"),
    ] {
        if count > 0 {
            lines.push(format!("{count} {note}"));
        }
    }
    lines
}

/// The preview window: what the file holds, what will be skipped, and Import or Cancel.
struct ImportPreview {
    import: Option<Box<BookmarkImport>>,
    folder_title: String,
    events: UnboundedSender<PreviewEvent>,
}

impl ImportPreview {
    fn finish(&mut self, confirmed: bool, window: &mut Window) {
        let event = match (confirmed, self.import.take()) {
            (true, Some(import)) => PreviewEvent::Confirm {
                import,
                folder_title: self.folder_title.clone(),
            },
            _ => PreviewEvent::Cancel,
        };
        if self.events.unbounded_send(event).is_err() {
            eprintln!("The import choice was made after the browser window closed.");
        }
        window.remove_window();
    }

    fn skipped_list(summary: &ImportSummary) -> Stateful<Div> {
        let mut list = div()
            .id("import-skipped")
            .flex()
            .flex_col()
            .gap(px(space::XS))
            .overflow_y_scroll();
        for group in skip_groups(summary, LISTED_SKIPS) {
            list = list.child(
                div()
                    .text_color(theme::text())
                    .child(format!("{} \u{00D7} {}", group.count, group.reason)),
            );
            for example in group.examples {
                list = list.child(
                    div()
                        .pl(px(space::XL))
                        .text_size(px(text::SMALL))
                        .text_color(theme::icon_dim())
                        .overflow_hidden()
                        .child(example),
                );
            }
        }
        list
    }
}

impl Render for ImportPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(import) = &self.import else {
            return div();
        };
        let summary = &import.summary;
        let has_entries = summary.bookmarks + summary.folders > 0;
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(
                div()
                    .text_size(px(text::TITLE))
                    .font_weight(gpui::FontWeight(f32::from(text::WEIGHT_SEMIBOLD)))
                    .child("Import bookmarks"),
            )
            .child(div().text_color(theme::icon()).child(format!(
                "Adds a new folder \u{201C}{}\u{201D} after your saved pages. Nothing opens.",
                self.folder_title
            )));
        for line in summary_lines(summary) {
            body = body.child(div().child(line));
        }
        if !summary.skipped.is_empty() {
            body = body.child(
                div()
                    .pt(px(space::M))
                    .text_color(theme::text())
                    .child(format!("{} entries will be skipped", summary.skipped.len())),
            );
        }
        let mut actions = div().flex().justify_end().gap(px(space::M)).child(
            preview_button("import-cancel", "Cancel", false)
                .on_click(cx.listener(|this, _, window, _| this.finish(false, window))),
        );
        if has_entries {
            actions = actions.child(
                preview_button("import-confirm", "Import", true)
                    .on_click(cx.listener(|this, _, window, _| this.finish(true, window))),
            );
        } else {
            body = body.child(
                div()
                    .text_color(theme::icon_dim())
                    .child("There is nothing to import."),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(space::XL))
            .p(px(space::XXL))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::line())
            .rounded(px(radius::PAGE_CARD))
            .text_color(theme::text())
            .text_size(px(text::BODY))
            .font_family(text::UI_FONT_FAMILIES[0])
            .child(body)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .child(Self::skipped_list(summary)),
            )
            .child(actions)
    }
}

fn preview_button(id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    let (background, foreground) = if primary {
        (theme::accent(), theme::rgba(atlas_ui::accent::BADGE_TEXT))
    } else {
        (theme::line(), theme::text())
    };
    div()
        .id(id)
        .px(px(space::XL))
        .py(px(space::S))
        .rounded(px(radius::CONTROL))
        .bg(background)
        .text_color(foreground)
        .cursor_pointer()
        .child(label)
}

#[cfg(test)]
mod tests {
    use atlas_core::{SkippedEntry, parse_bookmark_html};

    use super::*;

    #[test]
    fn the_command_accepts_spacing_and_case_but_nothing_else() {
        for input in [
            ">import bookmarks",
            "  > Import   Bookmarks ",
            ">IMPORT BOOKMARKS",
        ] {
            assert!(is_import_command(input), "{input}");
        }
        for input in [
            "import bookmarks",
            ">import",
            ">import bookmarks now",
            "> import-bookmarks",
            "",
        ] {
            assert!(!is_import_command(input), "{input}");
        }
    }

    #[test]
    fn skipped_entries_group_by_reason_and_name_only_the_first_few() {
        let entry = |line, reason, title: &str| SkippedEntry {
            line,
            reason,
            title: title.to_owned(),
        };
        let summary = ImportSummary {
            skipped: vec![
                entry(3, SkipReason::ScriptAddress, "Synthetic bookmarklet"),
                entry(5, SkipReason::Separator, ""),
                entry(9, SkipReason::ScriptAddress, "Another"),
                entry(11, SkipReason::ScriptAddress, "Third"),
            ],
            ..ImportSummary::default()
        };
        assert_eq!(
            skip_groups(&summary, 3),
            [
                SkipGroup {
                    reason: SkipReason::ScriptAddress.to_string(),
                    count: 3,
                    examples: vec![
                        "Line 3: Synthetic bookmarklet".to_owned(),
                        "Line 9: Another".to_owned(),
                    ],
                },
                SkipGroup {
                    reason: SkipReason::Separator.to_string(),
                    count: 1,
                    examples: vec!["Line 5".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn summary_lines_report_counts_and_leave_out_zero_notes() {
        let import = parse_bookmark_html(
            b"<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<DL><p>\n<DT><H3>Synthetic</H3>\n<DL><p>\n\
              <DT><A HREF=\"https://a.example.test/\">A</A>\n\
              <DT><A HREF=\"https://a.example.test/\">A again</A>\n</DL><p>\n</DL><p>\n",
        )
        .expect("synthetic export parses");
        assert_eq!(
            summary_lines(&import.summary),
            [
                "2 pages in 1 folders (0 empty)",
                "Folders nest up to 1 levels deep",
                "1 pages repeat an earlier address and are kept",
            ]
        );
    }

    #[test]
    fn oversized_and_unreadable_files_are_refused_before_parsing() {
        let directory = std::env::temp_dir().join(format!(
            "atlas-import-read-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock follows the epoch")
                .as_nanos()
        ));
        std::fs::create_dir(&directory).expect("create disposable directory");
        let file = directory.join("synthetic.html");
        std::fs::write(&file, b"0123456789").expect("write synthetic file");
        assert_eq!(read_bounded(&file, 10), Ok(b"0123456789".to_vec()));
        assert!(read_bounded(&file, 9).is_err());
        assert_eq!(
            read_bounded(&directory.join("missing.html"), 10),
            Err("Cannot read the chosen file.".to_owned())
        );
        std::fs::remove_file(&file).expect("remove synthetic file");
        std::fs::remove_dir(&directory).expect("remove disposable directory");
    }

    #[test]
    fn refusals_ask_for_a_fresh_utf8_export() {
        assert!(refusal_message(ImportError::NotUtf8 { offset: 4 }).contains("UTF-8 files only"));
        assert!(refusal_message(ImportError::NotABookmarkFile).contains("Export bookmarks"));
    }
}
