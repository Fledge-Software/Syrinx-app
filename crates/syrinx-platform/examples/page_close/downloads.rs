//! Proves Syrinx hears and controls every page's downloads (#143) with synthetic files only:
//! a link in the same tab, popups (including ones that close themselves, as Drive's does),
//! pause and cancel, and an executable that SmartScreen may judge.

use std::collections::HashSet;

use super::*;
use syrinx_core::{Download, DownloadControl, DownloadInterruption, DownloadList, DownloadState};
use syrinx_platform::browser::PageAction;

/// The tab every case starts from; adopted popups take the ids after it.
const OPENER: u64 = 5000;
/// The synthetic files the server offers as attachments.
const SAME_TAB: &str = "synthetic.zip";
const POPUP: &str = "synthetic-popup.zip";
const CLOSING: &str = "synthetic-closing.zip";
const HELD: &str = "synthetic-held.zip";
const PAUSED: &str = "synthetic-pause.zip";
/// A few bytes behind an executable's name; not a program, but SmartScreen judges by name too.
const EXECUTABLE: &str = "synthetic-tool.exe";
/// How long the server holds a held file's second half, so the case can act mid-download.
const HOLD: Duration = Duration::from_secs(3);
/// An empty zip archive's end record: a valid, empty synthetic archive.
const EMPTY_ZIP: [u8; 22] = [
    0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
/// A held file's zero padding before its end record, split in half by the hold.
const HELD_PADDING: usize = 64 * 1024;

const SAME_TAB_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Download same tab</title>\
    <link rel=icon href=data:,><a id=file href=/download/synthetic.zip>Synthetic file</a>\
    <script>addEventListener('load',()=>document.getElementById('file').click());</script>";
const POPUP_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Download popup</title>\
    <link rel=icon href=data:,><p>Synthetic popup download\
    <script>addEventListener('load',()=>window.open('/download/synthetic-popup.zip'));</script>";
const CLOSING_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Download closing</title>\
    <link rel=icon href=data:,><p>Synthetic closing popup\
    <script>addEventListener('load',()=>window.open('/download-closing-child'));</script>";
const CLOSING_CHILD: &str = "<!doctype html><meta charset=utf-8><title>Closing child</title>\
    <link rel=icon href=data:,><script>location.href='/download/synthetic-closing.zip';\
    setTimeout(()=>window.close(),300);</script>";
const HELD_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Download held</title>\
    <link rel=icon href=data:,><p>Synthetic held popup\
    <script>addEventListener('load',()=>window.open('/download-held-child'));</script>";
const HELD_CHILD: &str = "<!doctype html><meta charset=utf-8><title>Held child</title>\
    <link rel=icon href=data:,><script>location.href='/download/synthetic-held.zip';\
    setTimeout(()=>window.close(),300);</script>";
const PAUSE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Download pause</title>\
    <link rel=icon href=data:,><a id=file href=/download/synthetic-pause.zip>Synthetic file</a>\
    <script>addEventListener('load',()=>document.getElementById('file').click());</script>";
const EXECUTABLE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Download exe</title>\
    <link rel=icon href=data:,><a id=file href=/download/synthetic-tool.exe>Synthetic tool</a>\
    <script>addEventListener('load',()=>document.getElementById('file').click());</script>";

/// The suite's HTML pages.
pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    [
        ("/download-same", SAME_TAB_PAGE),
        ("/download-popup", POPUP_PAGE),
        ("/download-closing", CLOSING_PAGE),
        ("/download-closing-child", CLOSING_CHILD),
        ("/download-held", HELD_PAGE),
        ("/download-held-child", HELD_CHILD),
        ("/download-pause", PAUSE_PAGE),
        ("/download-exe", EXECUTABLE_PAGE),
    ]
    .into_iter()
    .find(|(path, _)| request.starts_with(format!("GET {path} ").as_bytes()))
    .map(|(_, page)| page)
}

/// The synthetic file a `/download/<name>` request asks for, if it is one of the suite's.
pub(super) fn file(request: &[u8]) -> Option<&'static str> {
    [SAME_TAB, POPUP, CLOSING, HELD, PAUSED, EXECUTABLE]
        .into_iter()
        .find(|name| request.starts_with(format!("GET /download/{name} ").as_bytes()))
}

/// Sends `name` as an attachment. A held file's second half follows only after `HOLD`, and
/// the browser may hang up meanwhile: the held and pause cases expect that.
pub(super) fn send_file(stream: &mut TcpStream, name: &str) -> Result<(), String> {
    let body = body(name);
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
         Content-Disposition: attachment; filename={name}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let split = if is_held(name) {
        body.len() / 2
    } else {
        body.len()
    };
    let (first, rest) = body.split_at(split);
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(first))
        .map_err(|error| io_error("send synthetic download", error))?;
    if rest.is_empty() {
        return Ok(());
    }
    trace(|| format!("server holding {name} for {HOLD:?}"));
    thread::sleep(HOLD);
    if let Err(error) = stream.write_all(rest) {
        println!(
            "The browser hung up on the held synthetic download ({}).",
            io_error("send its second half", error)
        );
    }
    Ok(())
}

fn is_held(name: &str) -> bool {
    name == HELD || name == PAUSED
}

fn body(name: &str) -> Vec<u8> {
    if name == EXECUTABLE {
        let mut body = b"MZ".to_vec();
        body.resize(64, 0);
        return body;
    }
    let padding = if is_held(name) { HELD_PADDING } else { 0 };
    let mut body = vec![0; padding];
    body.extend_from_slice(&EMPTY_ZIP);
    body
}

pub(super) fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    while !matches!(checked_event(events, deadline)?, BrowserEvent::Ready) {}
    let base = url
        .strip_suffix("/close")
        .ok_or("Synthetic URL shape changed.")?;
    // Script timers of a hidden page run late, so the pages are shown.
    unsafe {
        let _previous_visibility = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
            parent,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
        );
    }
    send(
        host,
        BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }),
    )?;
    let folder = reserve_profile()?;
    let mut watch = Watch::new(host);
    start_opener(&mut watch, events, &folder, deadline)?;

    same_tab(&mut watch, events, base, deadline)?;
    popup(&mut watch, events, base, deadline)?;
    closing_popup(&mut watch, events, base, deadline)?;
    held_popup(&mut watch, events, base, deadline)?;
    pause_and_cancel(&mut watch, events, base, deadline)?;
    executable(&mut watch, events, base, deadline)?;
    require_saved(&folder, &[SAME_TAB, POPUP, CLOSING])?;
    report_dialog(&watch);
    send(host, BrowserCommand::Close { id: OPENER })
}

/// Creates the opener blank, then points its profile's downloads at the synthetic folder.
fn start_opener(
    watch: &mut Watch,
    events: &mut UnboundedReceiver<BrowserEvent>,
    folder: &std::path::Path,
    deadline: Instant,
) -> Result<(), String> {
    send(
        watch.host,
        BrowserCommand::Create {
            id: OPENER,
            url: "about:blank".into(),
        },
    )?;
    watch.until(events, deadline, |watch| watch.created.contains(&OPENER))?;
    send(
        watch.host,
        BrowserCommand::PageAction {
            id: OPENER,
            action: PageAction::SetDownloadFolder(folder.to_owned()),
        },
    )?;
    lifecycle::barrier(watch.host, events, deadline)
}

/// Case 1: a link on the page in view downloads the file.
fn same_tab(
    watch: &mut Watch,
    events: &mut UnboundedReceiver<BrowserEvent>,
    base: &str,
    deadline: Instant,
) -> Result<(), String> {
    watch.navigate_opener(&format!("{base}/download-same"))?;
    watch.until(events, deadline, |watch| watch.finished(SAME_TAB))?;
    let seen = watch.seen(SAME_TAB)?;
    require_completed(seen)?;
    if seen.tab != OPENER {
        return Err(format!("{SAME_TAB} was reported by tab {}.", seen.tab));
    }
    println!("Synthetic same-tab download was listed and completed.");
    Ok(())
}

/// Case 2: `window.open` on the file's address; the adopted popup's download is heard.
fn popup(
    watch: &mut Watch,
    events: &mut UnboundedReceiver<BrowserEvent>,
    base: &str,
    deadline: Instant,
) -> Result<(), String> {
    watch.navigate_opener(&format!("{base}/download-popup"))?;
    watch.until(events, deadline, |watch| watch.finished(POPUP))?;
    let seen = watch.seen(POPUP)?;
    require_completed(seen)?;
    let tab = seen.tab;
    println!(
        "Synthetic popup download was listed and completed (from tab {tab}; the popup {} to close).",
        if watch.close_requests.contains(&tab) {
            "asked"
        } else {
            "did not ask"
        }
    );
    watch.close_popups()
}

/// Case 3: a popup starts the download and closes itself. As in Syrinx, the shared
/// `DownloadList` holds its close until its download has ended; the download completes.
fn closing_popup(
    watch: &mut Watch,
    events: &mut UnboundedReceiver<BrowserEvent>,
    base: &str,
    deadline: Instant,
) -> Result<(), String> {
    watch.navigate_opener(&format!("{base}/download-closing"))?;
    watch.until(events, deadline, |watch| {
        watch.finished(CLOSING)
            && watch
                .seen(CLOSING)
                .is_ok_and(|seen| watch.close_requests.contains(&seen.tab))
    })?;
    require_completed(watch.seen(CLOSING)?)?;
    watch.until(events, deadline, |watch| watch.popups.is_empty())?;
    println!("Synthetic self-closing popup's download was listed and completed.");
    Ok(())
}

/// Case 4, a measurement: a popup closes itself mid-download and is closed at once, without
/// the deferral. Prints whether WebView2 kept the download; either outcome passes.
fn held_popup(
    watch: &mut Watch,
    events: &mut UnboundedReceiver<BrowserEvent>,
    base: &str,
    deadline: Instant,
) -> Result<(), String> {
    watch.defer_closes = false;
    watch.navigate_opener(&format!("{base}/download-held"))?;
    watch.until(events, deadline, |watch| {
        watch
            .seen(HELD)
            .is_ok_and(|seen| watch.close_requests.contains(&seen.tab))
    })?;
    let outcome = watch.until(events, outcome_deadline(deadline), |watch| {
        watch.finished(HELD)
    });
    let state = watch.seen(HELD)?.state;
    match (outcome, state) {
        (Ok(()), DownloadState::Completed) => {
            println!("Closing a popup's web view mid-download kept the download: it completed.")
        }
        (Ok(()), state) => println!(
            "Closing a popup's web view mid-download stopped the download ({state:?}); keep the deferral."
        ),
        (Err(_), state) => println!(
            "Closing a popup's web view mid-download left it {state:?} with no final state; keep the deferral."
        ),
    }
    watch.defer_closes = true;
    Ok(())
}

/// Case 5: the flyout's Pause, Resume and Cancel, sent as Syrinx sends them, while the
/// server holds the file's second half.
fn pause_and_cancel(
    watch: &mut Watch,
    events: &mut UnboundedReceiver<BrowserEvent>,
    base: &str,
    deadline: Instant,
) -> Result<(), String> {
    watch.navigate_opener(&format!("{base}/download-pause"))?;
    watch.until(events, deadline, |watch| watch.seen(PAUSED).is_ok())?;
    let download = watch.seen(PAUSED)?.id;
    for (control, expected) in [
        (
            DownloadControl::Pause,
            DownloadState::Interrupted(DownloadInterruption::Paused),
        ),
        (DownloadControl::Resume, DownloadState::InProgress),
        (
            DownloadControl::Cancel,
            DownloadState::Interrupted(DownloadInterruption::Cancelled),
        ),
    ] {
        let offered = watch.seen(PAUSED)?.controls();
        if !offered.contains(&control) {
            return Err(format!("{PAUSED} did not offer {control:?} ({offered:?})."));
        }
        send(
            watch.host,
            BrowserCommand::ControlDownload { download, control },
        )?;
        watch.until(events, deadline, |watch| {
            watch.seen(PAUSED).is_ok_and(|seen| seen.state == expected)
        })?;
        println!("Synthetic download {control:?} reached {expected:?}.");
    }
    if !watch.seen(PAUSED)?.controls().is_empty() {
        return Err(format!("A cancelled {PAUSED} still offers controls."));
    }
    Ok(())
}

/// Case 6, a record: an executable's final state and reason, which SmartScreen decides.
/// Its verdict on a loopback file may vary, so no verdict is asserted; a download still
/// running afterwards is cancelled, since only SmartScreen's own dialog could keep it.
fn executable(
    watch: &mut Watch,
    events: &mut UnboundedReceiver<BrowserEvent>,
    base: &str,
    deadline: Instant,
) -> Result<(), String> {
    watch.navigate_opener(&format!("{base}/download-exe"))?;
    watch.until(events, deadline, |watch| watch.seen(EXECUTABLE).is_ok())?;
    let outcome = watch.until(events, outcome_deadline(deadline), |watch| {
        watch.finished(EXECUTABLE)
    });
    let seen = watch.seen(EXECUTABLE)?;
    let (download, state, offered) = (seen.id, seen.state, seen.controls());
    println!(
        "Synthetic executable download ended {state:?} ({}); Syrinx offers {offered:?}.",
        if outcome.is_ok() {
            "a final state"
        } else {
            "still running after the wait"
        }
    );
    if offered.contains(&DownloadControl::Resume)
        && matches!(
            state,
            DownloadState::Interrupted(DownloadInterruption::Blocked(_))
        )
    {
        return Err("A blocked executable offered Resume.".into());
    }
    if state != DownloadState::InProgress {
        return Ok(());
    }
    send(
        watch.host,
        BrowserCommand::ControlDownload {
            download,
            control: DownloadControl::Cancel,
        },
    )?;
    watch.until(events, deadline, |watch| watch.finished(EXECUTABLE))
}

/// A shorter wait for a measured outcome, within the suite's deadline.
fn outcome_deadline(deadline: Instant) -> Instant {
    deadline.min(Instant::now() + HOLD + Duration::from_secs(10))
}

/// WebView2's own dialog must stay closed, because Syrinx sets Handled on every download.
fn report_dialog(watch: &Watch) {
    match watch.dialog_opened {
        0 => println!("WebView2's own download dialog never opened."),
        times => println!(
            "Warning: WebView2's own download dialog opened {times} times despite Handled."
        ),
    }
}

/// Follows the suite's tabs as Syrinx does: adopts popups, shows them, keeps the downloads in
/// the shared `DownloadList`, and closes a page that asks, after its active downloads unless
/// `defer_closes` is off.
struct Watch<'a> {
    host: &'a BrowserHost,
    next_popup: u64,
    created: HashSet<u64>,
    popups: Vec<u64>,
    downloads: DownloadList,
    close_requests: Vec<u64>,
    defer_closes: bool,
    dialog_opened: usize,
}

impl<'a> Watch<'a> {
    fn new(host: &'a BrowserHost) -> Self {
        Self {
            host,
            next_popup: OPENER + 1,
            created: HashSet::new(),
            popups: Vec::new(),
            downloads: DownloadList::default(),
            close_requests: Vec::new(),
            defer_closes: true,
            dialog_opened: 0,
        }
    }

    /// Shows the opener again and loads `url` in it.
    fn navigate_opener(&self, url: &str) -> Result<(), String> {
        send(self.host, BrowserCommand::ShowActive(Some(OPENER)))?;
        send(
            self.host,
            BrowserCommand::Navigate {
                id: OPENER,
                url: url.into(),
            },
        )
    }

    /// Handles events until `done` holds.
    fn until(
        &mut self,
        events: &mut UnboundedReceiver<BrowserEvent>,
        deadline: Instant,
        done: impl Fn(&Self) -> bool,
    ) -> Result<(), String> {
        while !done(self) {
            let event = next_event(events, deadline)?;
            self.observe(event)?;
        }
        Ok(())
    }

    fn observe(&mut self, event: BrowserEvent) -> Result<(), String> {
        match event {
            BrowserEvent::PopupRequested { request, .. } => {
                let id = self.next_popup;
                self.next_popup += 1;
                send(self.host, BrowserCommand::AdoptPopup { request, id })?;
            }
            BrowserEvent::Created { id } => {
                self.created.insert(id);
                if id != OPENER {
                    self.popups.push(id);
                    send(self.host, BrowserCommand::ShowActive(Some(id)))?;
                }
            }
            BrowserEvent::DownloadStarted {
                download,
                tab,
                file_name,
                total,
            } => self.downloads.add(download, tab, &file_name, total),
            BrowserEvent::DownloadChanged {
                download,
                state,
                received,
                total,
                can_resume,
            } => {
                self.downloads.progress(download, received, total);
                self.downloads.set_resumable(download, can_resume);
                if let Some(tab) = self.downloads.update(download, state, received) {
                    self.close(tab)?;
                }
            }
            BrowserEvent::DownloadDialog { open: true, .. } => self.dialog_opened += 1,
            BrowserEvent::CloseRequested { id } => self.close_requested(id)?,
            // A navigation that becomes a download ends without a document.
            BrowserEvent::NavigationCompleted { success: false, .. } => {}
            other => check_event(&other)?,
        }
        Ok(())
    }

    fn close_requested(&mut self, id: u64) -> Result<(), String> {
        self.close_requests.push(id);
        if self.defer_closes && self.downloads.defer_close(id) {
            return Ok(());
        }
        self.close(id)
    }

    /// Closes popup `id` once; a late request from a popup already closed is ignored.
    fn close(&mut self, id: u64) -> Result<(), String> {
        if !self.popups.contains(&id) {
            return Ok(());
        }
        self.popups.retain(|popup| *popup != id);
        send(self.host, BrowserCommand::Close { id })
    }

    /// Closes the popups still open, such as one that never asked to close.
    fn close_popups(&mut self) -> Result<(), String> {
        for popup in std::mem::take(&mut self.popups) {
            send(self.host, BrowserCommand::Close { id: popup })?;
        }
        Ok(())
    }

    fn finished(&self, name: &str) -> bool {
        self.seen(name)
            .is_ok_and(|seen| seen.state != DownloadState::InProgress)
    }

    fn seen(&self, name: &str) -> Result<&Download, String> {
        self.downloads
            .downloads()
            .iter()
            .find(|seen| seen.file_name == name)
            .ok_or_else(|| format!("{name} was never reported."))
    }
}

fn require_completed(seen: &Download) -> Result<(), String> {
    if seen.state == DownloadState::Completed {
        return Ok(());
    }
    Err(format!("{} ended {:?}.", seen.file_name, seen.state))
}

/// Each file reached the synthetic folder with every byte the server sent.
fn require_saved(folder: &std::path::Path, names: &[&str]) -> Result<(), String> {
    for name in names {
        let saved = fs::metadata(folder.join(name))
            .map_err(|error| io_error(&format!("find saved {name}"), error))?;
        let expected = body(name).len() as u64;
        if saved.len() != expected {
            return Err(format!(
                "{name} saved {} bytes, not {expected}.",
                saved.len()
            ));
        }
    }
    println!("Every synthetic download reached the synthetic folder.");
    Ok(())
}
