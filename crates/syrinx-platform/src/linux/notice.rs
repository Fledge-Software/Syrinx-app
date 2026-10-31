//! Shows a start-up problem in a dialog, because a launch from the desktop has no console.

use gtk::prelude::*;

use super::gtk_thread;

/// Shows `text` and waits until the owner closes the dialog; without a display it is printed.
pub fn show_warning(text: &str) {
    let message = text.to_owned();
    let shown = gtk_thread::run(move || {
        let dialog = gtk::MessageDialog::new(
            None::<&gtk::Window>,
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Warning,
            gtk::ButtonsType::Ok,
            &message,
        );
        dialog.set_title("Syrinx");
        // The only answer is OK, so the response carries no information.
        let _closed = dialog.run();
        dialog.close();
    });
    if let Err(error) = shown {
        eprintln!("{error}\n{text}");
    }
}
