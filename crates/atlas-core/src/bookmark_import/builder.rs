//! Builds the folder tree from tokens, tolerating the missing closing tags real exports have.

use std::collections::HashSet;

use super::text::{display_text, truncate_chars};
use super::tokenizer::{StartTag, Token};
use super::{
    BookmarkImport, FolderRole, ImportError, ImportLimits, ImportSummary, ImportedBookmark,
    ImportedFolder, ImportedItem, SkipReason, SkippedEntry, address,
};

/// An open `<DL>` list: a folder's contents, or a list with no heading of its own.
enum Frame {
    Folder(ImportedFolder),
    /// The export's outer list, or a stray one; its entries belong to the enclosing folder.
    List,
}

/// The entry whose text is still being read.
enum Pending<'a> {
    None,
    /// An `<H3>` folder heading; `closed` once `</H3>` ends its name, awaiting its `<DL>`.
    Heading {
        folder: ImportedFolder,
        text: String,
        closed: bool,
        line: usize,
    },
    /// An `<A>` bookmark whose title runs until `</A>` or the next structural tag.
    Link {
        tag: StartTag<'a>,
        text: String,
        line: usize,
    },
}

pub(super) struct Builder<'a> {
    limits: ImportLimits,
    /// Receives entries outside every folder; `</DL>` never closes it.
    root: ImportedFolder,
    /// Open lists, innermost last.
    frames: Vec<Frame>,
    pending: Pending<'a>,
    summary: ImportSummary,
    addresses: HashSet<String>,
    entries: usize,
}

impl<'a> Builder<'a> {
    pub fn new(limits: ImportLimits) -> Self {
        Self {
            limits,
            root: empty_folder(None, FolderRole::Ordinary),
            frames: Vec::new(),
            pending: Pending::None,
            summary: ImportSummary::default(),
            addresses: HashSet::new(),
            entries: 0,
        }
    }

    /// Applies one token; only the structural tags of the export format matter.
    pub fn token(&mut self, line: usize, token: Token<'a>) -> Result<(), ImportError> {
        match token {
            Token::Text(text) => self.text(text),
            Token::Start(tag) => self.start_tag(line, tag)?,
            Token::End(name) => self.end_tag(name)?,
            Token::Doctype(_) => {}
        }
        Ok(())
    }

    /// Closes everything still open and returns the tree with its summary.
    pub fn finish(mut self) -> Result<BookmarkImport, ImportError> {
        self.finish_pending()?;
        while !self.frames.is_empty() {
            self.summary.unclosed_lists += 1;
            self.close_frame();
        }
        Ok(BookmarkImport {
            items: self.root.items,
            summary: self.summary,
        })
    }

    fn text(&mut self, text: &str) {
        // Text matters only inside a heading or link; descriptions and `<TITLE>` are ignored.
        match &mut self.pending {
            Pending::Heading {
                text: collected,
                closed: false,
                ..
            }
            | Pending::Link {
                text: collected, ..
            } => collected.push_str(text),
            _ => {}
        }
    }

    /// Structural tags also end an unclosed link or heading; other tags are ignored.
    fn start_tag(&mut self, line: usize, tag: StartTag<'a>) -> Result<(), ImportError> {
        if tag.is("A") {
            self.finish_pending()?;
            self.pending = Pending::Link {
                tag,
                text: String::new(),
                line,
            };
        } else if tag.is("H3") {
            self.finish_pending()?;
            self.pending = Pending::Heading {
                folder: empty_folder(added(tag.attribute("ADD_DATE")), folder_role(&tag)),
                text: String::new(),
                closed: false,
                line,
            };
        } else if tag.is("DL") {
            self.open_list(line)?;
        } else if tag.is("HR") {
            self.finish_pending()?;
            self.skip(line, SkipReason::Separator, String::new())?;
        } else if tag.is("DD") {
            self.end_description_boundary()?;
        } else if tag.is("DT") || tag.is("H1") {
            self.finish_pending()?;
        }
        Ok(())
    }

    fn end_tag(&mut self, name: &str) -> Result<(), ImportError> {
        if name.eq_ignore_ascii_case("A") {
            self.finish_link()?;
        } else if name.eq_ignore_ascii_case("H3") {
            if let Pending::Heading { closed, .. } = &mut self.pending {
                *closed = true;
            }
        } else if name.eq_ignore_ascii_case("DL") {
            self.finish_pending()?;
            // A stray `</DL>` with no list open is ignored.
            self.close_frame();
        }
        Ok(())
    }

    /// A `<DD>` description ends a link, and ends a heading's name while keeping it pending.
    fn end_description_boundary(&mut self) -> Result<(), ImportError> {
        if let Pending::Heading { closed, .. } = &mut self.pending {
            *closed = true;
            return Ok(());
        }
        self.finish_link()
    }

    /// Opens a pending heading's folder, or a list with no heading of its own.
    fn open_list(&mut self, line: usize) -> Result<(), ImportError> {
        self.finish_link()?;
        let frame = match std::mem::replace(&mut self.pending, Pending::None) {
            Pending::Heading {
                folder, text, line, ..
            } => Frame::Folder(self.start_folder(folder, &text, line)?),
            _ => self.start_bare_list(line)?,
        };
        self.frames.push(frame);
        Ok(())
    }

    /// Bounds headingless lists separately from folder depth; the outer list is one.
    fn start_bare_list(&self, line: usize) -> Result<Frame, ImportError> {
        let open = self
            .frames
            .iter()
            .filter(|frame| matches!(frame, Frame::List))
            .count();
        if open >= self.limits.max_bare_lists {
            return Err(ImportError::TooManyBareLists {
                line,
                limit: self.limits.max_bare_lists,
            });
        }
        Ok(Frame::List)
    }

    /// Names a new folder and checks its depth and the entry bound.
    fn start_folder(
        &mut self,
        mut folder: ImportedFolder,
        raw_title: &str,
        line: usize,
    ) -> Result<ImportedFolder, ImportError> {
        let depth = self.folder_depth() + 1;
        if depth > self.limits.max_depth {
            return Err(self.too_deep(line));
        }
        self.count_entry(line)?;
        self.summary.max_depth = self.summary.max_depth.max(depth);
        folder.title = self.title(raw_title);
        Ok(folder)
    }

    /// Ends the pending entry; a heading never followed by `<DL>` is an empty folder.
    fn finish_pending(&mut self) -> Result<(), ImportError> {
        match std::mem::replace(&mut self.pending, Pending::None) {
            Pending::None => Ok(()),
            Pending::Link { tag, text, line } => self.add_link(tag, &text, line),
            Pending::Heading {
                folder, text, line, ..
            } => {
                let folder = self.start_folder(folder, &text, line)?;
                self.add_folder(folder);
                Ok(())
            }
        }
    }

    fn finish_link(&mut self) -> Result<(), ImportError> {
        if !matches!(self.pending, Pending::Link { .. }) {
            return Ok(());
        }
        self.finish_pending()
    }

    /// Validates a link's address and adds it as a bookmark or a skipped entry.
    fn add_link(
        &mut self,
        tag: StartTag<'a>,
        raw_title: &str,
        line: usize,
    ) -> Result<(), ImportError> {
        let title = self.title(raw_title);
        let address = match address::classify(tag.attribute("HREF"), self.limits.max_address_bytes)
        {
            Ok(address) => address,
            Err(reason) => return self.skip(line, reason, title),
        };
        self.count_entry(line)?;
        if !self.addresses.insert(address.clone()) {
            self.summary.duplicate_addresses += 1;
        }
        self.summary.bookmarks += 1;
        let bookmark = ImportedBookmark {
            title,
            address,
            added: added(tag.attribute("ADD_DATE")),
        };
        self.current_items().push(ImportedItem::Bookmark(bookmark));
        Ok(())
    }

    fn skip(&mut self, line: usize, reason: SkipReason, title: String) -> Result<(), ImportError> {
        self.count_entry(line)?;
        self.summary.skipped.push(SkippedEntry {
            line,
            reason,
            title,
        });
        Ok(())
    }

    /// Pops the innermost list; a folder joins its parent once its contents are known.
    fn close_frame(&mut self) {
        if let Some(Frame::Folder(folder)) = self.frames.pop() {
            self.add_folder(folder);
        }
    }

    fn add_folder(&mut self, folder: ImportedFolder) {
        self.summary.folders += 1;
        if folder.items.is_empty() {
            self.summary.empty_folders += 1;
        }
        self.current_items().push(ImportedItem::Folder(folder));
    }

    /// The entries of the innermost open folder; headingless lists add to their parent.
    fn current_items(&mut self) -> &mut Vec<ImportedItem> {
        let innermost = self.frames.iter_mut().rev().find_map(|frame| match frame {
            Frame::Folder(folder) => Some(&mut folder.items),
            Frame::List => None,
        });
        innermost.unwrap_or(&mut self.root.items)
    }

    /// Open folders, not counting headingless lists.
    fn folder_depth(&self) -> usize {
        self.frames
            .iter()
            .filter(|frame| matches!(frame, Frame::Folder(_)))
            .count()
    }

    fn title(&mut self, raw: &str) -> String {
        let mut title = display_text(raw);
        if truncate_chars(&mut title, self.limits.max_title_chars) {
            self.summary.truncated_titles += 1;
        }
        title
    }

    fn count_entry(&mut self, line: usize) -> Result<(), ImportError> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(ImportError::TooManyEntries {
                line,
                limit: self.limits.max_entries,
            });
        }
        Ok(())
    }

    fn too_deep(&self, line: usize) -> ImportError {
        ImportError::TooDeep {
            line,
            limit: self.limits.max_depth,
        }
    }
}

fn empty_folder(added: Option<u64>, role: FolderRole) -> ImportedFolder {
    ImportedFolder {
        title: String::new(),
        added,
        role,
        items: Vec::new(),
    }
}

/// Reads the toolbar and unfiled markers browsers put on their special folders.
fn folder_role(tag: &StartTag<'_>) -> FolderRole {
    let marked = |name| {
        tag.attribute(name)
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
    };
    if marked("PERSONAL_TOOLBAR_FOLDER") {
        FolderRole::Toolbar
    } else if marked("UNFILED_BOOKMARKS_FOLDER") {
        FolderRole::Unfiled
    } else {
        FolderRole::Ordinary
    }
}

/// Reads an `ADD_DATE` in whole seconds; anything else is treated as absent.
fn added(raw: Option<&str>) -> Option<u64> {
    let digits = raw?.trim();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}
