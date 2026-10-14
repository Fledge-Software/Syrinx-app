//! Lists remembered site permissions in Customize and forgets the one the owner picks (DW-7).

use std::fmt;

use gpui::{
    App, Context, Div, FocusHandle, FontWeight, InteractiveElement, ParentElement, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use syrinx_store::{PermissionKind, RememberedPermission};
use syrinx_ui::Palette;
use syrinx_ui::metrics::text;

use super::BrowserView;
use crate::controls::control;
use crate::session::{SessionCommand, SessionError};
use crate::theme::rgba;

const ROW_HEIGHT: f32 = 52.;

/// What the list shows: nothing yet, a first read in flight, the rows of one read, or why
/// it failed. A reread keeps the old rows drawn until its answer replaces them.
#[derive(Default)]
enum Listing {
    #[default]
    Idle,
    Loading,
    /// `generation` is the read that returned these rows; buttons drawn from them carry it.
    Loaded {
        generation: u64,
        rows: Vec<RememberedPermission>,
    },
    Failed(String),
}

/// The row a Forget button acts on, captured when the button was drawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ListedRow {
    generation: u64,
    listed: RememberedPermission,
}

/// Where keyboard focus goes after the view changes under the focused control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusTarget {
    Back,
    Manage,
    Row(usize),
}

/// After a forget's reread: the row that took the forgotten row's place, else the
/// previous one, else Back when the list is empty.
fn focus_after_reread(forgotten: usize, rows: usize) -> FocusTarget {
    match rows.checked_sub(1) {
        Some(last) => FocusTarget::Row(forgotten.min(last)),
        None => FocusTarget::Back,
    }
}

/// Which choice a row shows; the stored allow/deny value may change under the same row.
type RowIdentity = (String, PermissionKind);

fn identity(listed: &RememberedPermission) -> RowIdentity {
    (listed.origin.clone(), listed.kind)
}

/// Keeps each still-listed choice's value from `old` and makes one for each new choice. A
/// choice no longer listed takes its value with it, so focus is cleared, never passed on.
fn carry_over<T>(
    old: Vec<(RowIdentity, T)>,
    rows: &[RememberedPermission],
    mut make: impl FnMut() -> T,
) -> Vec<(RowIdentity, T)> {
    let mut old = old;
    rows.iter()
        .map(|listed| {
            let key = identity(listed);
            match old.iter().position(|(kept, _)| *kept == key) {
                Some(index) => old.swap_remove(index),
                None => (key, make()),
            }
        })
        .collect()
}

/// The handles that keep keyboard focus when the view switches or rows are read again.
/// Each is tracked by at most one element per frame (the accessibility-tree trap): every
/// row has its own handle, keyed by the choice it shows.
pub(super) struct PermissionFocus {
    manage: FocusHandle,
    back: FocusHandle,
    rows: Vec<(RowIdentity, FocusHandle)>,
}

impl PermissionFocus {
    pub(super) fn new(cx: &mut App) -> Self {
        // Explicit handles start out of Tab order unless made Tab stops (the `tab_stop` trap).
        Self {
            manage: cx.focus_handle().tab_stop(true),
            back: cx.focus_handle().tab_stop(true),
            rows: Vec::new(),
        }
    }

    /// Matches the row handles to freshly read rows, keeping each surviving choice's handle.
    fn sync_rows(&mut self, rows: &[RememberedPermission], cx: &mut App) {
        let old = std::mem::take(&mut self.rows);
        self.rows = carry_over(old, rows, || cx.focus_handle().tab_stop(true));
    }

    fn row(&self, listed: &RememberedPermission) -> Option<&FocusHandle> {
        let key = identity(listed);
        self.rows
            .iter()
            .find(|(kept, _)| *kept == key)
            .map(|(_, handle)| handle)
    }

    fn handle(&self, target: FocusTarget) -> Option<&FocusHandle> {
        match target {
            FocusTarget::Back => Some(&self.back),
            FocusTarget::Manage => Some(&self.manage),
            FocusTarget::Row(index) => self.rows.get(index).map(|(_, handle)| handle),
        }
    }
}

/// The site-permissions view inside Customize, with at most one read and one forget in flight.
#[derive(Default)]
pub(super) struct SitePermissions {
    open: bool,
    sequence: u64,
    listing: Listing,
    /// The read whose answer the view takes; older answers are ignored.
    reading: Option<u64>,
    /// Counts openings, so an answer to a forget from an earlier opening moves no focus.
    opened: u64,
    /// The forget in flight: its request, the listed choice, that choice's row and opening.
    forgetting: Option<Forgetting>,
    /// The forgotten row whose place takes focus once the reread lands.
    refocus: Option<usize>,
    status: Option<String>,
}

struct Forgetting {
    request_id: u64,
    listed: RememberedPermission,
    index: usize,
    opened: u64,
}

// Hand-written so origins in the rows and status never reach a log or panic message.
impl fmt::Debug for SitePermissions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rows = match &self.listing {
            Listing::Loaded { rows, .. } => Some(rows.len()),
            _ => None,
        };
        formatter
            .debug_struct("SitePermissions")
            .field("open", &self.open)
            .field("rows", &rows)
            .field("forgetting", &self.forgetting.is_some())
            .field("status", &self.status.is_some())
            .finish_non_exhaustive()
    }
}

impl SitePermissions {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the view and returns the read that fills it.
    fn open(&mut self) -> SessionCommand {
        self.open = true;
        self.opened = self.opened.wrapping_add(1);
        self.status = None;
        self.refocus = None;
        self.listing = Listing::Loading;
        self.reload()
    }

    /// Hides the view; a forget still in flight reports through the footer instead.
    pub(super) fn close(&mut self) {
        self.open = false;
        self.listing = Listing::Idle;
        self.reading = None;
        self.status = None;
        self.refocus = None;
    }

    /// Asks for the list again; rows already drawn stay drawn until the answer arrives.
    fn reload(&mut self) -> SessionCommand {
        let request_id = self.next_request();
        self.reading = Some(request_id);
        if !matches!(self.listing, Listing::Loaded { .. }) {
            self.listing = Listing::Loading;
        }
        SessionCommand::RememberedPermissions { request_id }
    }

    /// The rows drawn now, for matching focus handles to them.
    fn rows(&self) -> &[RememberedPermission] {
        match &self.listing {
            Listing::Loaded { rows, .. } => rows,
            _ => &[],
        }
    }

    fn next_request(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    /// Takes a read's rows if it is the read the view is waiting for, and says where focus
    /// goes when the read follows a forget.
    fn listed(
        &mut self,
        request_id: u64,
        result: Result<Vec<RememberedPermission>, SessionError>,
    ) -> Option<FocusTarget> {
        self.reading.take_if(|waiting| *waiting == request_id)?;
        let refocus = self.refocus.take();
        let rows = match result {
            Ok(rows) => rows,
            Err(error) => {
                self.listing =
                    Listing::Failed(format!("Remembered permissions could not be read: {error}"));
                return refocus.map(|_| FocusTarget::Back);
            }
        };
        let target = refocus.map(|forgotten| focus_after_reread(forgotten, rows.len()));
        self.listing = Listing::Loaded {
            generation: request_id,
            rows,
        };
        target
    }

    /// Starts forgetting `row` if it is still listed exactly as drawn; `None` refuses the click.
    fn forget(&mut self, row: &ListedRow) -> Option<SessionCommand> {
        if self.forgetting.is_some() || !self.open {
            return None;
        }
        let Listing::Loaded { generation, rows } = &self.listing else {
            return None;
        };
        let index = rows.iter().position(|listed| *listed == row.listed);
        let Some(index) = index.filter(|_| *generation == row.generation) else {
            self.status =
                Some("The list changed before that click, so nothing was forgotten.".to_owned());
            return None;
        };
        let request_id = self.next_request();
        self.forgetting = Some(Forgetting {
            request_id,
            listed: row.listed.clone(),
            index,
            opened: self.opened,
        });
        self.status = None;
        Some(SessionCommand::ForgetPermission {
            request_id,
            listed: row.listed.clone(),
        })
    }

    /// Records a forget's answer. Returns the outcome and, while the view is open, the read
    /// that refreshes the list; a closed view leaves the outcome for the footer.
    fn forgotten(
        &mut self,
        request_id: u64,
        result: Result<bool, SessionError>,
    ) -> Option<(String, Option<SessionCommand>)> {
        let forgetting = self
            .forgetting
            .take_if(|forgetting| forgetting.request_id == request_id)?;
        let message = forget_message(&forgetting.listed, result);
        if !self.open {
            return Some((message, None));
        }
        self.status = Some(message.clone());
        // After Back and Manage again, the owner is elsewhere; leave focus where it is.
        if forgetting.opened == self.opened {
            self.refocus = Some(forgetting.index);
        }
        Some((message, Some(self.reload())))
    }

    /// A page remembered a choice, so an open view reads the list again in the background.
    /// A forget in flight rereads anyway once it is answered.
    fn choices_changed(&mut self) -> Option<SessionCommand> {
        if !self.open || self.forgetting.is_some() {
            return None;
        }
        Some(self.reload())
    }

    /// A list read could not be queued.
    fn read_not_sent(&mut self) {
        self.refocus = None;
        self.reading = None;
        self.listing = Listing::Failed(format!(
            "Remembered permissions could not be read: {}",
            SessionError::StorageUnavailable
        ));
    }

    /// A forget could not be queued, so nothing was asked of storage.
    fn forget_not_sent(&mut self) {
        if self.forgetting.take().is_some() {
            self.status = Some(format!(
                "That choice was not forgotten: {}",
                SessionError::StorageUnavailable
            ));
        }
    }

    /// The session stopped: nothing in flight can answer and no button may start more work.
    pub(super) fn session_lost(&mut self) {
        if self.open {
            self.read_not_sent();
        }
        if self.forgetting.take().is_some() {
            self.status = Some(
                "Local storage stopped before answering, so that choice may not be forgotten."
                    .to_owned(),
            );
        }
    }

    fn busy(&self) -> bool {
        self.forgetting.is_some()
    }
}

fn forget_message(listed: &RememberedPermission, result: Result<bool, SessionError>) -> String {
    match result {
        Ok(true) => format!(
            "Forgotten. {} will ask again before it can {}.",
            listed.origin,
            listed.kind.label()
        ),
        Ok(false) => {
            "That choice had already changed or been forgotten, so nothing was removed.".to_owned()
        }
        Err(error) => format!("That choice was not forgotten: {error}"),
    }
}

/// A short noun for each kind; the prompt's verb phrase reads oddly in a list.
fn kind_name(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => "Microphone",
        PermissionKind::Camera => "Camera",
        PermissionKind::Location => "Location",
        PermissionKind::Notifications => "Notifications",
        PermissionKind::Clipboard => "Clipboard",
        PermissionKind::Sensors => "Motion sensors",
        PermissionKind::Midi => "MIDI devices",
        PermissionKind::Downloads => "Multiple downloads",
        PermissionKind::FileSystem => "Files",
        PermissionKind::Autoplay => "Autoplay",
        PermissionKind::Fonts => "Local fonts",
        PermissionKind::WindowManagement => "Window management",
    }
}

fn decision(allowed: bool) -> &'static str {
    if allowed { "Allowed" } else { "Denied" }
}

impl BrowserView {
    fn open_site_permissions(&mut self, window: &mut Window, cx: &mut App) {
        let read = self.home.site_permissions.open();
        self.request_permission_list(read);
        self.focus_site_permissions(FocusTarget::Back, window, cx);
    }

    fn close_site_permissions(&mut self, window: &mut Window, cx: &mut App) {
        self.home.site_permissions.close();
        self.focus_site_permissions(FocusTarget::Manage, window, cx);
    }

    fn focus_site_permissions(&self, target: FocusTarget, window: &mut Window, cx: &mut App) {
        if let Some(handle) = self.site_permission_focus.handle(target) {
            window.focus(handle, cx);
        }
    }

    /// Queues a list read; one the session cannot take fails the list at once.
    fn request_permission_list(&mut self, read: SessionCommand) {
        if !self.try_command(read) {
            self.home.site_permissions.read_not_sent();
        }
    }

    fn forget_site_permission(&mut self, row: &ListedRow) {
        let Some(forget) = self.home.site_permissions.forget(row) else {
            return;
        };
        if !self.try_command(forget) {
            self.home.site_permissions.forget_not_sent();
        }
    }

    pub(super) fn site_permissions_listed(
        &mut self,
        request_id: u64,
        result: Result<Vec<RememberedPermission>, SessionError>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let target = self.home.site_permissions.listed(request_id, result);
        // Handles follow the choices they show, so a reread never moves focus onto another.
        self.site_permission_focus
            .sync_rows(self.home.site_permissions.rows(), cx);
        if let Some(target) = target {
            self.focus_site_permissions(target, window, cx);
        }
    }

    pub(super) fn site_permission_forgotten(
        &mut self,
        request_id: u64,
        result: Result<bool, SessionError>,
    ) {
        let Some((message, reload)) = self.home.site_permissions.forgotten(request_id, result)
        else {
            return;
        };
        match reload {
            Some(read) => self.request_permission_list(read),
            None => self.message = message,
        }
    }

    /// A page's permission was checked or remembered; an open list may now be out of date.
    pub(super) fn site_permissions_changed(&mut self) {
        if let Some(read) = self.home.site_permissions.choices_changed() {
            self.request_permission_list(read);
        }
    }

    /// Customize's way in: what the list holds and a Manage button.
    pub(super) fn site_permissions_entry(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let manage = control(
            div().id("site-permissions-manage"),
            Role::Button,
            "Manage site permissions",
        )
        .track_focus(&self.site_permission_focus.manage)
        .h(px(28.))
        .px(px(12.))
        .flex_none()
        .rounded(px(14.))
        .border_1()
        .border_color(rgba(palette.pill_line))
        .text_color(rgba(palette.text_secondary))
        .text_size(px(text::SMALL))
        .flex()
        .items_center()
        .cursor_pointer()
        .child("Manage")
        .on_click(cx.listener(|this, _, window, cx| {
            this.open_site_permissions(window, cx);
            cx.notify();
        }));
        div()
            .pt(px(14.))
            .border_t_1()
            .border_color(rgba(palette.panel_divider))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Site permissions"),
                    )
                    .child(
                        div()
                            .text_size(px(text::SMALL))
                            .text_color(rgba(palette.text_muted))
                            .child("Forget an Allow or Deny you asked Syrinx to remember."),
                    ),
            )
            .child(manage)
    }

    /// Replaces Customize's choices while the owner manages remembered permissions.
    pub(super) fn site_permissions_view(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let state = &self.home.site_permissions;
        let mut view = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.site_permissions_header(palette, cx))
            .child(
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_muted))
                    .child(
                        "Choices you asked Syrinx to remember for a site. Forget one and the \
                         site asks again the next time it wants that permission.",
                    ),
            )
            .child(self.permission_listing(palette, cx));
        if let Some(status) = &state.status {
            view = view.child(status_line(palette, status));
        }
        view
    }

    fn site_permissions_header(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let back = control(
            div().id("site-permissions-back"),
            Role::Button,
            "Back to Customize",
        )
        .track_focus(&self.site_permission_focus.back)
        .size(px(28.))
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgba(palette.icon_dim))
        .cursor_pointer()
        .child("‹")
        .on_click(cx.listener(|this, _, window, cx| {
            this.close_site_permissions(window, cx);
            cx.notify();
        }));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(back)
            .child(
                div()
                    .flex_1()
                    .text_size(px(text::SUBTITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Site permissions"),
            )
            .child(self.customize_close_button(palette, cx))
    }

    /// The list itself, or a line saying why there is none. The panel scrolls, not the list.
    fn permission_listing(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let state = &self.home.site_permissions;
        let (generation, rows) = match &state.listing {
            Listing::Idle | Listing::Loading => return note(palette, "Reading…"),
            Listing::Failed(error) => return note(palette, error),
            Listing::Loaded { generation, rows } => (*generation, rows),
        };
        if rows.is_empty() {
            return note(
                palette,
                "No choices are remembered. When a site asks, tick Remember for this site to \
                 keep your answer.",
            );
        }
        let mut list = div().flex().flex_col().gap(px(8.));
        for (index, listed) in rows.iter().enumerate() {
            let row = ListedRow {
                generation,
                listed: listed.clone(),
            };
            let focus = self.site_permission_focus.row(listed);
            list = list.child(permission_row(palette, row, index, state.busy(), focus, cx));
        }
        list
    }
}

/// One remembered choice: origin, kind and decision, and the button that forgets it.
fn permission_row(
    palette: &Palette,
    row: ListedRow,
    index: usize,
    busy: bool,
    focus: Option<&FocusHandle>,
    cx: &mut Context<BrowserView>,
) -> Div {
    let listed = &row.listed;
    let name = kind_name(listed.kind);
    let summary = format!("{name} · {}", decision(listed.allowed));
    let button_name = format!(
        "Forget {} {} for {}",
        decision(listed.allowed).to_lowercase(),
        name.to_lowercase(),
        listed.origin
    );
    let origin = listed.origin.clone();
    // A fresh id per read, so a click begun on an old row never lands on its replacement.
    let id = SharedString::from(format!("forget-permission-{}-{index}", row.generation));
    let mut forget = control(div().id(id), Role::Button, button_name);
    if let Some(focus) = focus {
        forget = forget.track_focus(focus);
    }
    let mut forget = forget
        .h(px(28.))
        .px(px(12.))
        .flex_none()
        .rounded(px(14.))
        .border_1()
        .border_color(rgba(palette.pill_line))
        .text_color(rgba(palette.text_secondary))
        .text_size(px(text::SMALL))
        .flex()
        .items_center()
        .cursor_pointer()
        .child("Forget")
        .on_click(cx.listener(move |this, _, _, cx| {
            this.forget_site_permission(&row);
            cx.notify();
        }));
    if busy {
        forget = forget.opacity(0.5);
    }
    div()
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex_none()
        .rounded(px(12.))
        .border_1()
        .border_color(rgba(palette.panel_divider))
        .pl(px(14.))
        .pr(px(10.))
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(origin),
                )
                .child(
                    div()
                        .text_size(px(text::SMALL))
                        .text_color(rgba(palette.text_muted))
                        .child(summary),
                ),
        )
        .child(forget)
}

fn note(palette: &Palette, message: &str) -> Div {
    div()
        .text_size(px(text::SMALL))
        .text_color(rgba(palette.text_secondary))
        .child(message.to_owned())
}

/// The last outcome, named for screen readers as the permission prompt's question is.
fn status_line(palette: &Palette, status: &str) -> Div {
    div().child(
        div()
            .id("site-permissions-status")
            .role(Role::Label)
            .aria_label(status.to_owned())
            .text_size(px(text::SMALL))
            .text_color(rgba(palette.text_secondary))
            .child(status.to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(origin: &str, kind: PermissionKind, allowed: bool) -> RememberedPermission {
        RememberedPermission {
            origin: origin.to_owned(),
            kind,
            allowed,
        }
    }

    fn read_id(command: SessionCommand) -> u64 {
        let SessionCommand::RememberedPermissions { request_id } = command else {
            panic!("expected a list read");
        };
        request_id
    }

    /// Opens the view and answers its read with `rows`, returning the rows as drawn.
    fn loaded(state: &mut SitePermissions, rows: Vec<RememberedPermission>) -> Vec<ListedRow> {
        let request_id = read_id(state.open());
        assert_eq!(state.listed(request_id, Ok(rows.clone())), None);
        drawn(request_id, rows)
    }

    fn drawn(generation: u64, rows: Vec<RememberedPermission>) -> Vec<ListedRow> {
        rows.into_iter()
            .map(|listed| ListedRow { generation, listed })
            .collect()
    }

    fn forget_id(command: Option<SessionCommand>) -> (u64, RememberedPermission) {
        let Some(SessionCommand::ForgetPermission { request_id, listed }) = command else {
            panic!("expected a forget request");
        };
        (request_id, listed)
    }

    fn origins(count: usize) -> Vec<RememberedPermission> {
        (0..count)
            .map(|index| {
                choice(
                    &format!("https://row{index}.example.test"),
                    PermissionKind::Camera,
                    true,
                )
            })
            .collect()
    }

    #[test]
    fn a_drawn_row_forgets_exactly_its_choice_and_then_rereads_the_list() {
        let mut state = SitePermissions::default();
        let camera = choice("https://camera.example.test", PermissionKind::Camera, true);
        let fonts = choice("https://fonts.example.test", PermissionKind::Fonts, false);
        let rows = loaded(&mut state, vec![camera.clone(), fonts]);

        let (request_id, listed) = forget_id(state.forget(&rows[0]));
        assert_eq!(
            listed, camera,
            "the stored allow/deny value travels with the click"
        );
        // A second click while the first is in flight sends nothing.
        assert!(state.forget(&rows[1]).is_none());
        assert!(state.busy());

        let (message, reload) = state
            .forgotten(request_id, Ok(true))
            .expect("the answer matches the forget in flight");
        assert_eq!(
            message,
            "Forgotten. https://camera.example.test will ask again before it can use your camera."
        );
        assert_eq!(state.status.as_deref(), Some(message.as_str()));
        let reread = read_id(reload.expect("an open view rereads the list"));
        assert_eq!(state.reading, Some(reread));
        assert!(!state.busy());
    }

    #[test]
    fn focus_after_a_forget_goes_to_the_next_row_then_the_previous_then_back() {
        assert_eq!(focus_after_reread(0, 2), FocusTarget::Row(0));
        assert_eq!(focus_after_reread(2, 2), FocusTarget::Row(1));
        assert_eq!(focus_after_reread(0, 0), FocusTarget::Back);
    }

    #[test]
    fn the_reread_after_a_forget_moves_focus_and_only_that_reread_does() {
        let mut state = SitePermissions::default();
        let rows = loaded(&mut state, origins(3));
        let (request_id, _) = forget_id(state.forget(&rows[1]));
        let (_, reload) = state.forgotten(request_id, Ok(true)).expect("answered");
        let reread = read_id(reload.expect("rereads"));
        let mut remaining = origins(3);
        remaining.remove(1);
        assert_eq!(
            state.listed(reread, Ok(remaining.clone())),
            Some(FocusTarget::Row(1))
        );

        // A later reread for another reason moves no focus.
        let again = read_id(state.choices_changed().expect("an open view rereads"));
        assert_eq!(state.listed(again, Ok(remaining)), None);

        // Forgetting the only row sends focus to Back.
        let mut state = SitePermissions::default();
        let rows = loaded(&mut state, origins(1));
        let (request_id, _) = forget_id(state.forget(&rows[0]));
        let (_, reload) = state.forgotten(request_id, Ok(true)).expect("answered");
        let reread = read_id(reload.expect("rereads"));
        assert_eq!(
            state.listed(reread, Ok(Vec::new())),
            Some(FocusTarget::Back)
        );
    }

    #[test]
    fn a_background_reread_keeps_the_rows_drawn_until_its_answer_arrives() {
        let mut state = SitePermissions::default();
        let rows = loaded(&mut state, origins(2));
        let reread = read_id(state.choices_changed().expect("an open view rereads"));
        assert!(
            matches!(&state.listing, Listing::Loaded { rows, .. } if rows.len() == 2),
            "the list never blanks to Reading…"
        );
        // An answer to an older read changes nothing; the awaited one swaps the rows.
        assert_eq!(state.listed(reread - 1, Ok(Vec::new())), None);
        assert_eq!(state.rows().len(), 2);
        assert_eq!(state.listed(reread, Ok(origins(3))), None);
        assert_eq!(state.rows().len(), 3);
        assert_eq!(state.reading, None);
        // A button drawn before the reread now belongs to an older read.
        assert!(state.forget(&rows[0]).is_none());
    }

    #[test]
    fn focus_handles_follow_their_choice_and_never_pass_to_another() {
        let first = origins(3);
        let mut next_handle = 0;
        let mut make = || {
            next_handle += 1;
            next_handle
        };
        let handles = carry_over(Vec::new(), &first, &mut make);
        assert_eq!(
            handles
                .iter()
                .map(|(_, handle)| *handle)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );

        // Row 1 is gone and a new choice arrives first: survivors keep their handles in
        // their new places, the new choice gets a fresh one, and handle 2 is not reused.
        let newcomer = choice("https://new.example.test", PermissionKind::Fonts, false);
        let mut changed = vec![newcomer, first[0].clone(), first[2].clone()];
        // A survivor whose stored value changed is still the same choice.
        changed[2].allowed = false;
        let handles = carry_over(handles, &changed, &mut make);
        assert_eq!(
            handles
                .iter()
                .map(|(_, handle)| *handle)
                .collect::<Vec<_>>(),
            [4, 1, 3]
        );
        assert_eq!(handles[1].0, identity(&first[0]));
    }

    #[test]
    fn a_late_forget_answer_moves_no_focus_after_the_list_was_reopened() {
        let mut state = SitePermissions::default();
        let rows = loaded(&mut state, origins(2));
        let (request_id, _) = forget_id(state.forget(&rows[0]));
        // The owner pressed Back and then Manage before the answer arrived.
        state.close();
        let reopened = read_id(state.open());
        let (_, reload) = state.forgotten(request_id, Ok(true)).expect("answered");
        let reread = read_id(reload.expect("an open view rereads"));
        assert_eq!(
            state.listed(reopened, Ok(origins(1))),
            None,
            "superseded read"
        );
        assert_eq!(state.listed(reread, Ok(origins(1))), None, "no focus move");
    }

    #[test]
    fn a_click_from_an_older_list_or_on_a_changed_row_removes_nothing() {
        let mut state = SitePermissions::default();
        let deny = choice(
            "https://stale.example.test",
            PermissionKind::Location,
            false,
        );
        let old_rows = loaded(&mut state, vec![deny.clone()]);

        // The list was read again; a button drawn from the old read is refused.
        let reread = read_id(state.reload());
        let allow = choice("https://stale.example.test", PermissionKind::Location, true);
        state.listed(reread, Ok(vec![allow.clone()]));
        assert!(state.forget(&old_rows[0]).is_none());
        assert_eq!(
            state.status.as_deref(),
            Some("The list changed before that click, so nothing was forgotten.")
        );

        // A row whose value differs from the current list is refused too.
        let mismatched = ListedRow {
            generation: reread,
            listed: deny,
        };
        assert!(state.forget(&mismatched).is_none());
        let current = ListedRow {
            generation: reread,
            listed: allow.clone(),
        };
        assert_eq!(forget_id(state.forget(&current)).1, allow);
    }

    #[test]
    fn already_gone_and_failed_forgets_say_so_and_stale_answers_are_ignored() {
        let mut state = SitePermissions::default();
        let gone = vec![choice(
            "https://gone.example.test",
            PermissionKind::Midi,
            true,
        )];
        let rows = loaded(&mut state, gone.clone());
        let (request_id, _) = forget_id(state.forget(&rows[0]));
        assert!(state.forgotten(request_id + 100, Ok(true)).is_none());
        let (message, _) = state.forgotten(request_id, Ok(false)).expect("answered");
        assert_eq!(
            message,
            "That choice had already changed or been forgotten, so nothing was removed."
        );

        let rows = loaded(&mut state, gone);
        let (request_id, _) = forget_id(state.forget(&rows[0]));
        let (message, _) = state
            .forgotten(request_id, Err(SessionError::StorageUnavailable))
            .expect("answered");
        assert_eq!(
            message,
            "That choice was not forgotten: Local storage is unavailable"
        );
    }

    #[test]
    fn a_forget_answered_after_the_view_closed_goes_to_the_footer() {
        let mut state = SitePermissions::default();
        let rows = loaded(
            &mut state,
            vec![choice(
                "https://closed.example.test",
                PermissionKind::Camera,
                false,
            )],
        );
        let (request_id, _) = forget_id(state.forget(&rows[0]));
        state.close();
        let (message, reload) = state.forgotten(request_id, Ok(true)).expect("answered");
        assert!(reload.is_none(), "a closed view reads nothing");
        assert!(message.starts_with("Forgotten. https://closed.example.test"));
        assert_eq!(state.status, None);
        // The closed view's old buttons can start nothing.
        assert!(state.forget(&rows[0]).is_none());
    }

    #[test]
    fn reads_match_their_request_and_a_lost_session_ends_every_wait() {
        let mut state = SitePermissions::default();
        let first = read_id(state.open());
        state.listed(first + 1, Ok(Vec::new()));
        assert!(
            matches!(state.listing, Listing::Loading),
            "a stale read is ignored"
        );
        state.session_lost();
        assert!(matches!(
            &state.listing,
            Listing::Failed(message)
                if message == "Remembered permissions could not be read: Local storage is unavailable"
        ));

        let rows = loaded(
            &mut state,
            vec![choice(
                "https://lost.example.test",
                PermissionKind::Fonts,
                true,
            )],
        );
        forget_id(state.forget(&rows[0]));
        state.session_lost();
        assert!(!state.busy());
        assert_eq!(
            state.status.as_deref(),
            Some("Local storage stopped before answering, so that choice may not be forgotten.")
        );
    }

    #[test]
    fn a_lost_session_leaves_no_live_forget_buttons() {
        let mut state = SitePermissions::default();
        let rows = loaded(
            &mut state,
            vec![choice(
                "https://idle.example.test",
                PermissionKind::Camera,
                true,
            )],
        );
        state.session_lost();
        assert!(
            matches!(state.listing, Listing::Failed(_)),
            "no rows are drawn"
        );
        assert!(state.forget(&rows[0]).is_none());
        assert_eq!(
            state.status, None,
            "nothing was in flight, so nothing is reported"
        );
    }

    #[test]
    fn a_forget_that_could_not_be_queued_says_it_was_not_sent() {
        let mut state = SitePermissions::default();
        let rows = loaded(
            &mut state,
            vec![choice(
                "https://unsent.example.test",
                PermissionKind::Camera,
                true,
            )],
        );
        forget_id(state.forget(&rows[0]));
        state.forget_not_sent();
        assert!(!state.busy());
        assert_eq!(
            state.status.as_deref(),
            Some("That choice was not forgotten: Local storage is unavailable")
        );
        state.read_not_sent();
        assert!(matches!(state.listing, Listing::Failed(_)));
    }

    #[test]
    fn a_changed_choice_rereads_an_open_list_but_not_a_closed_or_busy_one() {
        let mut state = SitePermissions::default();
        assert!(
            state.choices_changed().is_none(),
            "a closed view reads nothing"
        );
        loaded(&mut state, origins(1));
        let reread = read_id(state.choices_changed().expect("an open view rereads"));
        assert_eq!(state.reading, Some(reread));
        state.listed(reread, Ok(origins(2)));
        let rows = drawn(reread, origins(2));
        forget_id(state.forget(&rows[0]));
        assert!(
            state.choices_changed().is_none(),
            "the forget's own answer rereads"
        );
    }

    #[test]
    fn a_failed_read_names_its_cause() {
        let mut state = SitePermissions::default();
        let request_id = read_id(state.open());
        state.listed(request_id, Err(SessionError::StorageUnavailable));
        assert!(matches!(
            &state.listing,
            Listing::Failed(message) if message.ends_with("Local storage is unavailable")
        ));
    }

    #[test]
    fn debug_output_names_no_origin() {
        let mut state = SitePermissions::default();
        let rows = loaded(
            &mut state,
            vec![choice(
                "https://private.example.test",
                PermissionKind::Camera,
                true,
            )],
        );
        let (request_id, _) = forget_id(state.forget(&rows[0]));
        state.forgotten(request_id, Ok(true));
        assert!(
            state
                .status
                .as_deref()
                .is_some_and(|status| status.contains("private.example.test"))
        );
        let printed = format!("{state:?}");
        assert!(!printed.contains("private.example.test"), "{printed}");
    }
}
