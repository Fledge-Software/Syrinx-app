//! Asks the owner for a file or folder through GTK's native chooser, which uses the desktop portal.

use std::{fmt, path::PathBuf};

use gtk::prelude::*;

use super::gtk_thread;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileDialogError {
    Apartment,
    Dialog,
    NotAFile,
}

impl fmt::Display for FileDialogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Apartment => "GTK could not start to show the file dialog",
            Self::Dialog => "GTK could not show the file dialog",
            Self::NotAFile => "The chosen item is not a file or folder on this computer",
        })
    }
}

impl std::error::Error for FileDialogError {}

/// Asks for one existing file. Blocks until the dialog closes; `owner` is unused on Linux.
pub fn choose_file(title: &str, _owner: isize) -> Result<Option<PathBuf>, FileDialogError> {
    choose(title, gtk::FileChooserAction::Open, None)
}

/// Asks for one existing folder.
pub fn choose_folder(title: &str, _owner: isize) -> Result<Option<PathBuf>, FileDialogError> {
    choose(title, gtk::FileChooserAction::SelectFolder, None)
}

/// Asks where to save a new file, offering `suggested_name`; GTK asks before replacing one.
pub fn choose_new_file(
    title: &str,
    suggested_name: &str,
    _owner: isize,
) -> Result<Option<PathBuf>, FileDialogError> {
    choose(
        title,
        gtk::FileChooserAction::Save,
        Some(suggested_name.to_owned()),
    )
}

fn choose(
    title: &str,
    action: gtk::FileChooserAction,
    suggested_name: Option<String>,
) -> Result<Option<PathBuf>, FileDialogError> {
    let title = title.to_owned();
    let chosen = gtk_thread::run(move || {
        let dialog =
            gtk::FileChooserNative::new(Some(&title), None::<&gtk::Window>, action, None, None);
        dialog.set_modal(true);
        if let Some(name) = &suggested_name {
            dialog.set_current_name(name);
            dialog.set_do_overwrite_confirmation(true);
        }
        let response = dialog.run();
        let path = (response == gtk::ResponseType::Accept)
            .then(|| dialog.file().and_then(|file| file.path()));
        dialog.destroy();
        path
    })
    .map_err(|_| FileDialogError::Apartment)?;
    // Accepted with nothing chosen, or a remote location with no local path, is not a file.
    match chosen {
        None => Ok(None),
        Some(path) => path.map(Some).ok_or(FileDialogError::NotAFile),
    }
}
