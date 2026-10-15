//! The Windows pieces `syrinx-setup.exe` needs: its dialogs, a running-Syrinx check and one
//! registry read. Installing files and registering live in `syrinx-update` and the scripts.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW},
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
        UI::{
            Controls::{
                TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOG_FLAGS,
                TASKDIALOGCONFIG, TD_ERROR_ICON, TD_INFORMATION_ICON, TDCBF_CANCEL_BUTTON,
                TDCBF_CLOSE_BUTTON, TDCBF_RETRY_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
                TDF_SIZE_TO_CONTENT, TDF_VERIFICATION_FLAG_CHECKED, TaskDialogIndirect,
            },
            WindowsAndMessaging::IDRETRY,
        },
    },
    core::{BOOL, HSTRING, PCWSTR, PWSTR},
};

/// The title every setup dialog carries.
const TITLE: &str = "Syrinx Setup";

/// Button identifiers for the dialogs' own buttons; Windows' common buttons use IDOK and so on.
const INSTALL_BUTTON: i32 = 100;
const OPEN_BUTTON: i32 = 101;

/// Windows' longest path in UTF-16 units, for reading a process's image name.
const LONGEST_PATH: usize = 32_768;

/// The confirmation dialog's text and whether its checkbox starts ticked.
pub struct Confirmation<'a> {
    pub heading: &'a str,
    pub details: &'a str,
    pub action: &'a str,
    pub checkbox: &'a str,
    pub checked: bool,
}

/// Ask to install. `None` means Cancel; otherwise whether the checkbox was ticked.
pub fn confirm(confirmation: &Confirmation<'_>) -> Option<bool> {
    let buttons = [button(INSTALL_BUTTON, confirmation.action)];
    let flags = if confirmation.checked {
        TDF_VERIFICATION_FLAG_CHECKED
    } else {
        TASKDIALOG_FLAGS(0)
    };
    let dialog = Dialog {
        icon: TD_INFORMATION_ICON,
        heading: confirmation.heading,
        details: confirmation.details,
        buttons: &buttons,
        common: TDCBF_CANCEL_BUTTON,
        checkbox: Some(confirmation.checkbox),
        flags,
    };
    let (pressed, ticked) = dialog.show();
    (pressed == INSTALL_BUTTON).then_some(ticked)
}

/// Report a problem the owner can fix and retry, such as Syrinx still running.
pub fn ask_retry(heading: &str, details: &str) -> bool {
    let dialog = Dialog {
        icon: TD_ERROR_ICON,
        heading,
        details,
        buttons: &[],
        common: TDCBF_RETRY_BUTTON | TDCBF_CANCEL_BUTTON,
        checkbox: None,
        flags: TASKDIALOG_FLAGS(0),
    };
    dialog.show().0 == IDRETRY.0
}

/// Report a failure that ends setup.
pub fn show_error(heading: &str, details: &str) {
    let dialog = Dialog {
        icon: TD_ERROR_ICON,
        heading,
        details,
        buttons: &[],
        common: TDCBF_CLOSE_BUTTON,
        checkbox: None,
        flags: TASKDIALOG_FLAGS(0),
    };
    dialog.show();
}

/// Say setup finished; `true` when the owner pressed the open button.
pub fn show_finished(heading: &str, details: &str, open: &str) -> bool {
    let buttons = [button(OPEN_BUTTON, open)];
    let dialog = Dialog {
        icon: TD_INFORMATION_ICON,
        heading,
        details,
        buttons: &buttons,
        common: TDCBF_CLOSE_BUTTON,
        checkbox: None,
        flags: TASKDIALOG_FLAGS(0),
    };
    dialog.show().0 == OPEN_BUTTON
}

/// Whether a process runs from inside one of `folders`, compared without regard to case.
pub fn running_from(folders: &[PathBuf]) -> std::io::Result<bool> {
    let images = process_images()?;
    Ok(images
        .iter()
        .any(|image| folders.iter().any(|folder| is_inside(image, folder))))
}

/// Whether the per-user key `HKCU\{key}` exists.
pub fn user_key_exists(key: &str) -> bool {
    let mut opened = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            None,
            KEY_READ,
            &mut opened,
        )
    };
    if status.is_err() {
        return false;
    }
    // Closing a key opened for reading cannot lose anything, so its status is not needed.
    let _closed = unsafe { RegCloseKey(opened) };
    true
}

/// One task dialog, kept as borrowed text until it is shown.
struct Dialog<'a> {
    icon: PCWSTR,
    heading: &'a str,
    details: &'a str,
    buttons: &'a [(i32, HSTRING)],
    common: TASKDIALOG_COMMON_BUTTON_FLAGS,
    checkbox: Option<&'a str>,
    flags: TASKDIALOG_FLAGS,
}

impl Dialog<'_> {
    /// Show the dialog; the pressed button, and whether the checkbox ended ticked.
    fn show(&self) -> (i32, bool) {
        let title = HSTRING::from(TITLE);
        let heading = HSTRING::from(self.heading);
        let details = HSTRING::from(self.details);
        let checkbox = self.checkbox.map(HSTRING::from);
        let buttons: Vec<TASKDIALOG_BUTTON> = self
            .buttons
            .iter()
            .map(|(id, text)| TASKDIALOG_BUTTON {
                nButtonID: *id,
                pszButtonText: PCWSTR(text.as_ptr()),
            })
            .collect();
        let mut config = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: HWND::default(),
            dwFlags: self.flags | TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT,
            dwCommonButtons: self.common,
            pszWindowTitle: PCWSTR(title.as_ptr()),
            pszMainInstruction: PCWSTR(heading.as_ptr()),
            pszContent: PCWSTR(details.as_ptr()),
            cButtons: buttons.len() as u32,
            pButtons: buttons.as_ptr(),
            ..Default::default()
        };
        config.Anonymous1.pszMainIcon = self.icon;
        if let Some(checkbox) = &checkbox {
            config.pszVerificationText = PCWSTR(checkbox.as_ptr());
        }
        let mut pressed = 0;
        let mut ticked = BOOL(0);
        let shown =
            unsafe { TaskDialogIndirect(&config, Some(&mut pressed), None, Some(&mut ticked)) };
        // A dialog Windows could not show counts as Cancel, so nothing is changed.
        if let Err(error) = shown {
            eprintln!("setup could not show its dialog: {error}");
            return (0, false);
        }
        (pressed, ticked.as_bool())
    }
}

fn button(id: i32, text: &str) -> (i32, HSTRING) {
    (id, HSTRING::from(text))
}

/// The full image path of every process this user may query.
fn process_images() -> std::io::Result<Vec<PathBuf>> {
    let snapshot = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut images = Vec::new();
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok();
    while more {
        // Processes of other users or protected ones refuse the query and are skipped.
        if let Some(image) = image_path(entry.th32ProcessID) {
            images.push(image);
        }
        more = unsafe { Process32NextW(snapshot.0, &mut entry) }.is_ok();
    }
    Ok(images)
}

fn image_path(process_id: u32) -> Option<PathBuf> {
    let process = OwnedHandle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?,
    );
    let mut buffer = vec![0u16; LONGEST_PATH];
    let mut length = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()?;
    buffer.truncate(length as usize);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

/// Whether `path` is `folder` or below it, ignoring case as Windows does.
fn is_inside(path: &Path, folder: &Path) -> bool {
    let path = path.to_string_lossy().to_lowercase();
    let folder = folder.to_string_lossy().to_lowercase();
    let folder = folder.trim_end_matches('\\');
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('\\'))
}

/// A handle closed when dropped.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            eprintln!("setup could not close a process handle: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_contains_only_paths_below_it() {
        let folder = Path::new(r"C:\Users\someone\AppData\Local\Programs\Syrinx");
        assert!(is_inside(
            Path::new(r"c:\users\SOMEONE\appdata\local\programs\syrinx\syrinx.exe"),
            folder
        ));
        assert!(is_inside(folder, folder));
        assert!(!is_inside(
            Path::new(r"C:\Users\someone\AppData\Local\Programs\Syrinx.previous\syrinx.exe"),
            folder
        ));
        assert!(!is_inside(Path::new(r"C:\Windows\explorer.exe"), folder));
    }

    #[test]
    fn this_test_process_is_found_by_its_own_folder() {
        let executable = std::env::current_exe().expect("test executable");
        let folder = executable.parent().expect("its folder").to_path_buf();
        assert_eq!(running_from(&[folder]).ok(), Some(true));
        let elsewhere = folder_that_runs_nothing();
        assert_eq!(running_from(&[elsewhere]).ok(), Some(false));
    }

    fn folder_that_runs_nothing() -> PathBuf {
        PathBuf::from(r"C:\syrinx-setup-test.invalid")
    }
}
