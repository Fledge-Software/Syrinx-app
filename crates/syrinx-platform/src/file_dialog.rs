//! Shows Windows' own file-open dialog so the owner picks an exported bookmarks file (P1-22).

use std::{ffi::OsString, fmt, os::windows::ffi::OsStringExt, path::PathBuf};

use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::{
            Shell::{
                FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FileOpenDialog,
                IFileOpenDialog, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::IsWindow,
        },
    },
    core::{HSTRING, PWSTR},
};

/// Why the dialog could not return a choice. Carries no path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileDialogError {
    /// COM could not start on the calling thread.
    Apartment,
    /// Windows could not create or show the dialog.
    Dialog,
    /// The chosen item has no file-system path.
    NotAFile,
}

impl fmt::Display for FileDialogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Apartment => "Windows could not prepare the file dialog",
            Self::Dialog => "Windows could not show the file dialog",
            Self::NotAFile => "The chosen item is not a file on this computer",
        })
    }
}

impl std::error::Error for FileDialogError {}

/// Asks the owner for one existing file. Returns `None` when they cancel.
///
/// Blocks until the dialog closes, so call it from a dedicated thread rather than the
/// interface thread; it starts and ends its own single-threaded COM apartment there.
/// `owner` is the window the dialog stays above, or 0 for none.
pub fn choose_file(title: &str, owner: isize) -> Result<Option<PathBuf>, FileDialogError> {
    let _apartment = Apartment::new()?;
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| FileDialogError::Dialog)?;
    configure(&dialog, title)?;
    if let Err(error) = unsafe { dialog.Show(owner_window(owner)) } {
        if error.code() == ERROR_CANCELLED.to_hresult() {
            return Ok(None);
        }
        return Err(FileDialogError::Dialog);
    }
    chosen_path(&dialog).map(Some)
}

/// Only existing file-system files can be chosen.
fn configure(dialog: &IFileOpenDialog, title: &str) -> Result<(), FileDialogError> {
    unsafe {
        let options = dialog.GetOptions().map_err(|_| FileDialogError::Dialog)?;
        dialog
            .SetOptions(options | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM)
            .map_err(|_| FileDialogError::Dialog)?;
        dialog
            .SetTitle(&HSTRING::from(title))
            .map_err(|_| FileDialogError::Dialog)?;
    }
    Ok(())
}

fn owner_window(owner: isize) -> Option<HWND> {
    let window = HWND(owner as *mut _);
    (owner != 0 && unsafe { IsWindow(Some(window)) }.as_bool()).then_some(window)
}

fn chosen_path(dialog: &IFileOpenDialog) -> Result<PathBuf, FileDialogError> {
    let item = unsafe { dialog.GetResult() }.map_err(|_| FileDialogError::NotAFile)?;
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }
        .map(TaskString)
        .map_err(|_| FileDialogError::NotAFile)?;
    Ok(PathBuf::from(OsString::from_wide(unsafe {
        name.0.as_wide()
    })))
}

/// A single-threaded COM apartment for the dialog, ended when dropped.
struct Apartment;

impl Apartment {
    fn new() -> Result<Self, FileDialogError> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|_| FileDialogError::Apartment)?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// A string Windows allocated for us, freed when dropped.
struct TaskString(PWSTR);

impl Drop for TaskString {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.0.cast())) };
    }
}
