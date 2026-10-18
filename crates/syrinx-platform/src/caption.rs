//! Answers Windows' caption-button hit test from the point Windows asks about, because GPUI
//! answers from the last pointer position it saw, one mouse move behind (issue #11).

use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GWL_STYLE, GetClientRect, GetWindowLongPtrW, HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON,
            HTMINBUTTON, HTNOWHERE, WM_NCDESTROY, WM_NCHITTEST, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
            WS_SYSMENU,
        },
    },
};

/// The caption buttons, in the order Windows places them at a window's top-right corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionButton {
    Minimize,
    Maximize,
    Close,
}

impl CaptionButton {
    /// Left to right, ending at the window's right edge.
    pub const LEFT_TO_RIGHT: [Self; 3] = [Self::Minimize, Self::Maximize, Self::Close];

    /// The `WM_NCHITTEST` answer that gives this button Windows' own behaviour.
    fn hit_code(self) -> u32 {
        match self {
            Self::Minimize => HTMINBUTTON,
            Self::Maximize => HTMAXBUTTON,
            Self::Close => HTCLOSE,
        }
    }

    /// Whether the window style still offers this button; fullscreen removes all three.
    fn offered_by(self, style: u32) -> bool {
        let required = match self {
            Self::Minimize => WS_MINIMIZEBOX.0,
            Self::Maximize => WS_MAXIMIZEBOX.0,
            Self::Close => WS_SYSMENU.0,
        };
        style & required != 0
    }
}

/// Identifies Syrinx's subclass among any others on the window.
const SUBCLASS_ID: usize = 0x5359_4341;

/// Answer caption-button hit tests on `hwnd` from Windows' own pointer position.
///
/// `button_size` is the logical width and height of each square caption button, drawn flush
/// with the top-right corner of the client area. Call this on the window's own thread.
pub fn install(hwnd: isize, button_size: f32) -> Result<(), &'static str> {
    let window = HWND(hwnd as *mut _);
    let size = button_size.to_bits() as usize;
    if unsafe { SetWindowSubclass(window, Some(subclass), SUBCLASS_ID, size) }.as_bool() {
        Ok(())
    } else {
        Err("Syrinx could not watch the window's caption buttons.")
    }
}

/// Which caption button lies under a client-area point, all values in logical pixels.
fn button_at(x: f32, y: f32, client_width: f32, button_size: f32) -> Option<CaptionButton> {
    let buttons = CaptionButton::LEFT_TO_RIGHT;
    let first_left = client_width - buttons.len() as f32 * button_size;
    let in_strip = button_size > 0. && (0. ..button_size).contains(&y);
    if !in_strip || !(first_left..client_width).contains(&x) {
        return None;
    }
    let slot = ((x - first_left) / button_size).floor() as usize;
    buttons.get(slot).copied()
}

/// Correct GPUI's hit-test answer with the button actually under the pointer.
///
/// Resize edges and other answers GPUI computes from the real point are kept. A button code
/// for a point that is not on that button is GPUI's stale answer, so it becomes client area.
fn corrected_answer(gpui: isize, under_pointer: Option<CaptionButton>, style: u32) -> isize {
    let button_answers = [HTMINBUTTON, HTMAXBUTTON, HTCLOSE];
    let caption_answers = [HTCLIENT, HTCAPTION, HTNOWHERE];
    if !is_one_of(gpui, &button_answers) && !is_one_of(gpui, &caption_answers) {
        return gpui;
    }
    if let Some(button) = under_pointer.filter(|button| button.offered_by(style)) {
        return button.hit_code() as isize;
    }
    if is_one_of(gpui, &button_answers) {
        return HTCLIENT as isize;
    }
    gpui
}

/// Whether a window procedure's answer is one of these hit-test codes.
fn is_one_of(answer: isize, codes: &[u32]) -> bool {
    codes.iter().any(|&code| code as isize == answer)
}

/// The window procedure layer that corrects `WM_NCHITTEST` after GPUI answers it.
unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    button_size: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        remove_subclass(hwnd);
    }
    let answer = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if message != WM_NCHITTEST {
        return answer;
    }
    let size = f32::from_bits(button_size as u32);
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let button = pointer_button(hwnd, lparam, size);
    LRESULT(corrected_answer(answer.0, button, style))
}

/// The caption button under the screen point packed in a `WM_NCHITTEST` `lparam`.
fn pointer_button(hwnd: HWND, lparam: LPARAM, button_size: f32) -> Option<CaptionButton> {
    let mut point = POINT {
        x: i32::from(lparam.0 as u16 as i16),
        y: i32::from((lparam.0 >> 16) as u16 as i16),
    };
    let mut client = RECT::default();
    // Without the client geometry or DPI, GPUI's own answer stands.
    if !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
        return None;
    }
    let Ok(()) = (unsafe { GetClientRect(hwnd, &mut client) }) else {
        return None;
    };
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        return None;
    }
    let scale = dpi as f32 / 96.;
    button_at(
        point.x as f32 / scale,
        point.y as f32 / scale,
        client.right as f32 / scale,
        button_size,
    )
}

/// Comctl32 asks for each subclass to be removed as its window is destroyed.
fn remove_subclass(hwnd: HWND) {
    if !unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) }.as_bool() {
        eprintln!("Syrinx could not stop watching a closed window's caption buttons.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: f32 = 36.;
    const WIDTH: f32 = 1280.;
    const NORMAL: u32 = WS_SYSMENU.0 | WS_MAXIMIZEBOX.0 | WS_MINIMIZEBOX.0;

    #[test]
    fn buttons_sit_left_to_right_at_the_top_right_corner() {
        assert_eq!(
            button_at(WIDTH - 1., 18., WIDTH, SIZE),
            Some(CaptionButton::Close)
        );
        assert_eq!(
            button_at(WIDTH - 36., 18., WIDTH, SIZE),
            Some(CaptionButton::Close)
        );
        assert_eq!(
            button_at(WIDTH - 37., 18., WIDTH, SIZE),
            Some(CaptionButton::Maximize)
        );
        assert_eq!(
            button_at(WIDTH - 72., 0., WIDTH, SIZE),
            Some(CaptionButton::Maximize)
        );
        assert_eq!(
            button_at(WIDTH - 108., 35., WIDTH, SIZE),
            Some(CaptionButton::Minimize)
        );
    }

    #[test]
    fn points_outside_the_three_buttons_are_no_button() {
        assert_eq!(button_at(WIDTH - 109., 18., WIDTH, SIZE), None);
        assert_eq!(button_at(WIDTH - 1., 36., WIDTH, SIZE), None);
        assert_eq!(button_at(WIDTH - 1., -1., WIDTH, SIZE), None);
        assert_eq!(button_at(WIDTH, 18., WIDTH, SIZE), None);
        assert_eq!(button_at(WIDTH - 1., 18., WIDTH, 0.), None);
        assert_eq!(button_at(f32::NAN, 18., WIDTH, SIZE), None);
    }

    #[test]
    fn the_button_under_the_pointer_replaces_gpuis_stale_answer() {
        // GPUI last saw the pointer over Maximize; Windows asks about Close.
        let close = Some(CaptionButton::Close);
        assert_eq!(
            corrected_answer(HTMAXBUTTON as isize, close, NORMAL),
            HTCLOSE as isize
        );
        // GPUI last saw the pointer on the drag area or the page.
        let minimize = Some(CaptionButton::Minimize);
        assert_eq!(
            corrected_answer(HTCAPTION as isize, minimize, NORMAL),
            HTMINBUTTON as isize
        );
        let maximize = Some(CaptionButton::Maximize);
        assert_eq!(
            corrected_answer(HTCLIENT as isize, maximize, NORMAL),
            HTMAXBUTTON as isize
        );
    }

    #[test]
    fn a_stale_button_answer_off_the_buttons_becomes_client_area() {
        assert_eq!(
            corrected_answer(HTCLOSE as isize, None, NORMAL),
            HTCLIENT as isize
        );
        assert_eq!(
            corrected_answer(HTCAPTION as isize, None, NORMAL),
            HTCAPTION as isize
        );
        assert_eq!(
            corrected_answer(HTCLIENT as isize, None, NORMAL),
            HTCLIENT as isize
        );
    }

    #[test]
    fn resize_edges_and_fullscreen_keep_gpuis_answer() {
        use windows::Win32::UI::WindowsAndMessaging::{HTTOP, HTTOPRIGHT};
        let close = Some(CaptionButton::Close);
        assert_eq!(
            corrected_answer(HTTOPRIGHT as isize, close, NORMAL),
            HTTOPRIGHT as isize
        );
        assert_eq!(
            corrected_answer(HTTOP as isize, close, NORMAL),
            HTTOP as isize
        );
        // Fullscreen strips the caption styles, and GPUI then answers client area.
        assert_eq!(
            corrected_answer(HTCLIENT as isize, close, 0),
            HTCLIENT as isize
        );
    }
}
