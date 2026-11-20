//! Checks native ownership and stale-window probes without mapping or focusing any window.

use super::{probes::Windows, *};

#[test]
fn linux_probes_observe_hidden_ownership_and_window_destruction() {
    let result = crate::gtk_thread::run(|| -> Result<_, String> {
        let mut windows = Windows::new()?;
        let main = windows.create(false, true)?;
        let popup = windows.create(false, true)?;
        crate::browser::set_overlay_owner(popup, main).map_err(|error| error.to_string())?;
        let stacking = overlay_stacking(popup, main);
        let live = window_exists(popup);
        windows.destroy(popup);
        let destroyed = window_exists(popup);
        Ok((main, stacking, live, destroyed))
    })
    .expect("GTK thread")
    .expect("fixture windows");
    let stacking = result.1.expect("native ownership");
    assert_eq!(stacking.owner, result.0);
    assert!(!stacking.visible && !stacking.topmost);
    assert!(result.2, "an existing window must be reported live");
    assert!(!result.3, "a destroyed window must not be reported live");
}
