//! Names every item both platform backends must supply, with its exact type, so a signature
//! that drifts on one platform fails to build there instead of breaking the app later.

use std::{path::PathBuf, time::Duration};

use futures::channel::mpsc::UnboundedReceiver;
use syrinx_store::PermissionKind;

use crate::browser::{
    self, BrowserCommand, BrowserError, BrowserEvent, BrowserHost, OverlayMonitor, OverlayPointer,
    OverlayRect,
};
use crate::edge::{self, EdgeError};

// What the app reads about the engine, on both platforms.
const _: bool = browser::PAUSES_DOWNLOADS;
const _: bool = browser::READS_PAGE_TEXT;
const _: &[(&str, &str)] = browser::ENGINE_LIMITS;
const _: &str = browser::ENGINE_NAME;
const _: i32 = browser::RENDER_PROCESS_UNRESPONSIVE;
const _: fn(&str, PermissionKind) -> String = browser::permission_question;

// The browser host's lifecycle.
type Spawned = Result<(BrowserHost, UnboundedReceiver<BrowserEvent>), BrowserError>;
const _: fn(isize, PathBuf) -> Spawned = BrowserHost::spawn;
const _: fn(&BrowserHost, BrowserCommand) -> Result<(), BrowserError> = BrowserHost::send;
const _: fn(&BrowserHost, u64) = BrowserHost::set_peek_generation;
const _: fn(BrowserHost) -> Result<(), BrowserError> = BrowserHost::join;
const _: fn(BrowserHost, Duration) -> Result<(), BrowserError> = BrowserHost::shutdown_within;

// Keyboard focus and Syrinx's own popups over the page.
const _: fn(isize) -> Result<(), BrowserError> = browser::focus_chrome;
const _: fn(isize) -> bool = browser::chrome_has_keyboard_focus;
const _: fn(isize) -> bool = browser::holds_keyboard_focus;
const _: fn() -> bool = browser::syrinx_in_foreground;
const _: fn(isize) -> Result<bool, BrowserError> = browser::activate_if_foreground;
const _: fn(isize, isize) -> Result<(), BrowserError> = browser::set_overlay_owner;
const _: fn(isize, OverlayRect) -> Result<(), BrowserError> = browser::show_overlay;
const _: fn(isize, isize) -> Result<(), BrowserError> = browser::hide_overlay;
const _: fn(isize) -> Option<(i32, i32)> = browser::window_origin;
const _: fn(isize, Option<isize>) -> Result<OverlayPointer, String> = browser::overlay_pointer;
const _: fn(i32, i32) -> Result<OverlayMonitor, String> = browser::overlay_monitor;

// Handing a page to the other browser (D36).
const _: &str = edge::OTHER_BROWSER;
const _: &str = edge::OPEN_BUTTON;
const _: &str = edge::OPENED_MESSAGE;
const _: fn(&str) -> Result<(), EdgeError> = edge::open_in_edge;
