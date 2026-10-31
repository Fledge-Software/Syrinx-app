//! Owns the one WebKit context and every page, and routes each command to its page.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};

use webkit2gtk::{
    CacheModel, CookieManagerExt, CookiePersistentStorage, WebContext, WebContextExt, WebView,
    WebsiteDataManager, WebsiteDataManagerExt, WebsiteDataManagerExtManual, WebsiteDataTypes,
};

use super::{
    BrowserCommand, BrowserError, BrowserEvent, BrowsingDataKinds, ClearOutcome, ClearSpan,
    EventSink, PageBounds,
    downloads::Downloads,
    page::{Page, TextStart},
    page_text::TextContext,
    show_active, validate_url,
};

/// A popup the page asked for, held until the owner's UI adopts or cancels it.
pub(super) struct PendingPopup {
    pub(super) opener: u64,
    pub(super) view: WebView,
}

/// Popups waiting on the UI, by request number; shared with every page's `create` handler.
pub(super) type Popups = Rc<RefCell<HashMap<u64, PendingPopup>>>;

/// What every page's handlers share with the engine.
#[derive(Clone)]
pub(super) struct Shared {
    pub(super) parent: isize,
    pub(super) events: EventSink,
    pub(super) popups: Popups,
    pub(super) peek_generation: Arc<AtomicU64>,
    /// Request numbers for popups and permissions, unique for the whole run (see the traps).
    pub(super) requests: Rc<std::cell::Cell<u64>>,
}

impl Shared {
    pub(super) fn next_request(&self) -> u64 {
        let next = self.requests.get() + 1;
        self.requests.set(next);
        next
    }
}

pub(super) struct Engine {
    context: WebContext,
    shared: Shared,
    pages: HashMap<u64, Page>,
    downloads: Downloads,
    page_text: TextContext,
    active: Option<u64>,
    bounds: PageBounds,
}

impl Engine {
    pub(super) fn new(
        parent: isize,
        profile: &Path,
        events: EventSink,
        peek_generation: Arc<AtomicU64>,
    ) -> Result<Self, BrowserError> {
        let context = build_context(profile)?;
        let shared = Shared {
            parent,
            events,
            popups: Rc::default(),
            peek_generation,
            requests: Rc::default(),
        };
        // Downloads outlive the page that started them, so their events carry no page life.
        let downloads = Downloads::watch(&context, shared.events.detached(), shared.popups.clone());
        Ok(Self {
            context,
            shared,
            pages: HashMap::new(),
            downloads,
            page_text: TextContext::default(),
            active: None,
            bounds: PageBounds::default(),
        })
    }

    pub(super) fn close_all(&mut self) {
        self.shared.popups.borrow_mut().clear();
        self.pages.clear();
    }

    pub(super) fn apply(&mut self, command: BrowserCommand) -> Result<(), BrowserError> {
        match command {
            BrowserCommand::Create { id, url } => {
                validate_url(&url)?;
                let view = WebView::with_context(&self.context);
                let page = self.new_page(id, view, false)?;
                page.navigate(&url)?;
                self.pages.insert(id, page);
            }
            BrowserCommand::Reopen { id, url } => {
                validate_url(&url)?;
                let view = WebView::with_context(&self.context);
                // A tab Syrinx puts back starts tainted and its load is not Syrinx's own (#54).
                let page = self.new_page(id, view, true)?;
                page.restore(&url)?;
                self.pages.insert(id, page);
            }
            BrowserCommand::AdoptPopup { request, id } => {
                let pending = self
                    .shared
                    .popups
                    .borrow_mut()
                    .remove(&request)
                    .ok_or(BrowserError::Popup)?;
                let tainted = self
                    .pages
                    .get(&pending.opener)
                    .ok_or(BrowserError::UnknownTab)?
                    .popup_tainted();
                // The popup's view was created related to its opener, so `window.opener` works.
                let page = self.new_page(id, pending.view, tainted)?;
                self.pages.insert(id, page);
                self.downloads.adopted(request, id);
            }
            BrowserCommand::CancelPopup { request } => {
                let pending = self.shared.popups.borrow_mut().remove(&request);
                if let Some(pending) = pending {
                    self.downloads.released(request, pending.opener);
                }
            }
            BrowserCommand::Close { id } => self.close(id)?,
            BrowserCommand::ShowActive(active) => self.show(active)?,
            BrowserCommand::ClearData {
                request,
                kinds,
                span,
            } => self.clear(request, kinds, span),
            BrowserCommand::SetPageTextPolicy(policy) => self.page_text.set_policy(policy),
            BrowserCommand::ControlDownload { download, control } => {
                self.downloads.control(download, control);
            }
            BrowserCommand::Resize(next) => {
                next.validate()?;
                for page in self.pages.values() {
                    page.resize(next);
                }
                self.bounds = next;
            }
            command => {
                let id = command.tab_id().ok_or(BrowserError::InvalidTab)?;
                let page = self.pages.get(&id).ok_or(BrowserError::UnknownTab)?;
                page.apply(command)?;
            }
        }
        Ok(())
    }

    /// A page for tab `id`; `tainted` when it puts back an earlier page or opens from one.
    fn new_page(&self, id: u64, view: WebView, tainted: bool) -> Result<Page, BrowserError> {
        if id == 0 {
            return Err(BrowserError::InvalidTab);
        }
        if self.pages.contains_key(&id) {
            return Err(BrowserError::DuplicateTab);
        }
        let text = TextStart {
            context: &self.page_text,
            tainted,
        };
        let page = Page::new(id, view, self.bounds, self.shared.clone(), text)?;
        self.downloads.track(id, page.view());
        Ok(page)
    }

    fn close(&mut self, id: u64) -> Result<(), BrowserError> {
        let dropped: Vec<u64> = self
            .shared
            .popups
            .borrow()
            .iter()
            .filter(|(_, pending)| pending.opener == id)
            .map(|(request, _)| *request)
            .collect();
        for request in dropped {
            drop(self.shared.popups.borrow_mut().remove(&request));
            self.downloads.released(request, id);
        }
        let page = self.pages.remove(&id).ok_or(BrowserError::UnknownTab)?;
        self.downloads.forget(id);
        drop(page);
        if self.active == Some(id) {
            self.active = None;
        }
        Ok(())
    }

    fn show(&mut self, active: Option<u64>) -> Result<(), BrowserError> {
        if active.is_some_and(|id| !self.pages.contains_key(&id)) {
            return Err(BrowserError::UnknownTab);
        }
        let ids: Vec<u64> = self.pages.keys().copied().collect();
        let pages = &self.pages;
        show_active(ids.into_iter(), active, &mut self.active, |id, visible| {
            pages.get(&id).map_or(Ok(()), |page| {
                page.show(visible);
                Ok(())
            })
        })
    }

    /// Clears WebKit's own data; it can only clear from a moment until now, not a closed span.
    fn clear(&self, request: u64, kinds: BrowsingDataKinds, span: Option<ClearSpan>) {
        let events = self.shared.events.clone();
        let types = data_types(kinds);
        let Some(manager) = self.context.website_data_manager() else {
            events.emit(BrowserEvent::DataCleared {
                request,
                outcome: refused(),
            });
            return;
        };
        let Some(timespan) = timespan(span) else {
            events.emit(BrowserEvent::DataCleared {
                request,
                outcome: refused(),
            });
            return;
        };
        if types.is_empty() {
            events.emit(BrowserEvent::DataCleared {
                request,
                outcome: ClearOutcome {
                    cleared: Ok(()),
                    closed: Ok(()),
                },
            });
            return;
        }
        manager.clear(
            types,
            timespan,
            None::<&webkit2gtk::gio::Cancellable>,
            move |result| {
                let cleared = result.map_err(|_| BrowserError::ClearData { code: -1 });
                events.emit(BrowserEvent::DataCleared {
                    request,
                    outcome: ClearOutcome {
                        cleared,
                        closed: Ok(()),
                    },
                });
            },
        );
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn refused() -> ClearOutcome {
    ClearOutcome {
        cleared: Err(BrowserError::ClearData { code: -1 }),
        closed: Ok(()),
    }
}

/// WebKit's kinds for the owner's choices. WebKit keeps no browsing history of its own beyond
/// each page's back list, which closing the tab already drops.
fn data_types(kinds: BrowsingDataKinds) -> WebsiteDataTypes {
    let mut types = WebsiteDataTypes::empty();
    if kinds.cookies_and_site_data {
        types |= WebsiteDataTypes::COOKIES
            | WebsiteDataTypes::LOCAL_STORAGE
            | WebsiteDataTypes::SESSION_STORAGE
            | WebsiteDataTypes::INDEXEDDB_DATABASES
            | WebsiteDataTypes::OFFLINE_APPLICATION_CACHE
            | WebsiteDataTypes::SERVICE_WORKER_REGISTRATIONS
            | WebsiteDataTypes::DOM_CACHE
            | WebsiteDataTypes::WEBSQL_DATABASES;
    }
    if kinds.cached_files {
        types |= WebsiteDataTypes::DISK_CACHE | WebsiteDataTypes::MEMORY_CACHE;
    }
    types
}

/// WebKit clears data modified within the last `timespan` microseconds (0 is all time). A span
/// that ends well before now cannot be honoured without clearing more, so it is refused.
fn timespan(span: Option<ClearSpan>) -> Option<webkit2gtk::glib::TimeSpan> {
    let Some(span) = span else {
        return Some(webkit2gtk::glib::TimeSpan(0));
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let now = i64::try_from(now).ok()?;
    if span.end < now - 60 || span.start >= span.end {
        return None;
    }
    let seconds = (now - span.start).max(1);
    Some(webkit2gtk::glib::TimeSpan(seconds.checked_mul(1_000_000)?))
}

/// One context for every page, with its data, cache and cookies inside the profile.
fn build_context(profile: &Path) -> Result<WebContext, BrowserError> {
    let folder = |name: &str| {
        profile
            .join("WebKit")
            .join(name)
            .to_string_lossy()
            .into_owned()
    };
    let manager = WebsiteDataManager::builder()
        .base_data_directory(folder("data"))
        .base_cache_directory(folder("cache"))
        .build();
    let context = WebContext::with_website_data_manager(&manager);
    // Keep each page's web process inside WebKit's bubblewrap sandbox (AGENTS.md).
    context.set_sandbox_enabled(true);
    context.set_cache_model(CacheModel::WebBrowser);
    let cookies = manager.cookie_manager().ok_or(BrowserError::Environment)?;
    cookies.set_persistent_storage(&folder("cookies.sqlite"), CookiePersistentStorage::Sqlite);
    Ok(context)
}

/// The engine's version, reported once after `Ready` as WebView2's runtime version is.
pub(super) fn runtime_version() -> String {
    // SAFETY: these read constants compiled into the linked WebKitGTK library.
    let (major, minor, micro) = unsafe {
        (
            webkit2gtk::ffi::webkit_get_major_version(),
            webkit2gtk::ffi::webkit_get_minor_version(),
            webkit2gtk::ffi::webkit_get_micro_version(),
        )
    };
    format!("WebKitGTK {major}.{minor}.{micro}")
}
