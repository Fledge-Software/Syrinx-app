//! Retain native resources until a complete web view can own them.

use std::{
  cell::Cell,
  io::Write,
  rc::Rc,
  sync::atomic::{AtomicU32, Ordering},
};

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use windows::Win32::{
  Foundation::*,
  UI::{Shell::*, WindowsAndMessaging::*},
};

use crate::Result;

const CONSTRUCTION_SUBCLASS_ID: usize = (WM_USER + 0x67) as usize;

// Preserve structural failures even if the process has no usable stderr.
static CLEANUP_FAILURES: AtomicU32 = AtomicU32::new(0);
const CONTROLLER_CLOSE_FAILED: u32 = 1;
const CONTAINER_DESTROY_FAILED: u32 = 2;
const OBSERVER_REMOVE_FAILED: u32 = 4;
const DIAGNOSTIC_WRITE_FAILED: u32 = 8;

fn report_cleanup_failure(bit: u32, operation: &'static str) {
  CLEANUP_FAILURES.fetch_or(bit, Ordering::Relaxed);
  if writeln!(
    std::io::stderr().lock(),
    "wry construction cleanup failed: {operation}"
  )
  .is_err()
  {
    CLEANUP_FAILURES.fetch_or(DIAGNOSTIC_WRITE_FAILED, Ordering::Relaxed);
  }
}

pub(super) struct PendingController {
  pub(super) controller: ICoreWebView2Controller,
  close_on_drop: bool,
}

impl PendingController {
  pub(super) fn new(controller: ICoreWebView2Controller) -> Self {
    Self {
      controller,
      close_on_drop: true,
    }
  }

  // Move cleanup authority to the completed InnerWebView.
  pub(super) fn release(mut self) -> ICoreWebView2Controller {
    self.close_on_drop = false;
    self.controller.clone()
  }
}

impl Drop for PendingController {
  fn drop(&mut self) {
    if self.close_on_drop && unsafe { self.controller.Close() }.is_err() {
      report_cleanup_failure(CONTROLLER_CLOSE_FAILED, "controller Close");
    }
  }
}

pub(super) struct PendingContainer {
  pub(super) hwnd: HWND,
  alive: Rc<Cell<bool>>,
  observer: Option<*mut Rc<Cell<bool>>>,
  destroy_on_drop: bool,
}

impl PendingContainer {
  // Begin ownership before the first fallible operation after CreateWindowExW.
  pub(super) fn new(hwnd: HWND) -> Result<Self> {
    let mut owner = Self {
      hwnd,
      alive: Rc::new(Cell::new(true)),
      observer: None,
      destroy_on_drop: true,
    };
    let observer = Box::into_raw(Box::new(owner.alive.clone()));
    let installed = unsafe {
      SetWindowSubclass(
        hwnd,
        Some(construction_subclass_proc),
        CONSTRUCTION_SUBCLASS_ID,
        observer as usize,
      )
    };
    if !installed.as_bool() {
      // The failed registration never takes ownership of its reference data.
      unsafe { drop(Box::from_raw(observer)) };
      return Err(windows::core::Error::from(E_FAIL).into());
    }
    owner.observer = Some(observer);
    Ok(owner)
  }

  // Native callbacks may destroy the container while creation pumps messages.
  pub(super) fn ensure_alive(&self) -> Result<()> {
    if !self.alive.get() {
      return Err(windows::core::Error::from(E_HANDLE).into());
    }
    Ok(())
  }

  // Stop observing only when no further fallible construction remains.
  pub(super) fn release(mut self) -> HWND {
    self.destroy_on_drop = false;
    self.detach_observer();
    self.hwnd
  }

  fn detach_observer(&mut self) {
    let Some(observer) = self.observer.take() else {
      return;
    };
    if !self.alive.get() {
      return;
    }
    let removed = unsafe {
      RemoveWindowSubclass(
        self.hwnd,
        Some(construction_subclass_proc),
        CONSTRUCTION_SUBCLASS_ID,
      )
    };
    if removed.as_bool() {
      unsafe { drop(Box::from_raw(observer)) };
    } else {
      // Keep callback data alive; WM_NCDESTROY can still release it.
      report_cleanup_failure(OBSERVER_REMOVE_FAILED, "construction observer removal");
    }
  }
}

impl Drop for PendingContainer {
  fn drop(&mut self) {
    if self.destroy_on_drop && self.alive.get() && unsafe { DestroyWindow(self.hwnd) }.is_err() {
      // Do not retry a raw handle: another callback might have destroyed it.
      report_cleanup_failure(CONTAINER_DESTROY_FAILED, "container DestroyWindow");
    }
  }
}

// Clear the exact attempt's ownership before destruction can recycle its HWND.
unsafe extern "system" fn construction_subclass_proc(
  hwnd: HWND,
  message: u32,
  wparam: WPARAM,
  lparam: LPARAM,
  subclass_id: usize,
  reference_data: usize,
) -> LRESULT {
  if message == WM_NCDESTROY {
    let observer = reference_data as *mut Rc<Cell<bool>>;
    (*observer).set(false);
    if RemoveWindowSubclass(hwnd, Some(construction_subclass_proc), subclass_id).as_bool() {
      drop(Box::from_raw(observer));
    } else {
      // A failed removal cannot justify freeing data a callback may still use.
      report_cleanup_failure(
        OBSERVER_REMOVE_FAILED,
        "destroyed container observer removal",
      );
    }
  }
  DefSubclassProc(hwnd, message, wparam, lparam)
}
