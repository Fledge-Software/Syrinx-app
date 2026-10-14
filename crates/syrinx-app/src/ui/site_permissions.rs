//! Lists remembered site permissions in Customize and forgets the one the owner picks (DW-7).

use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Role, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};
use syrinx_store::{PermissionKind, RememberedPermission};
use syrinx_ui::Palette;
use syrinx_ui::metrics::text;

use super::BrowserView;
use crate::controls::control;
use crate::session::{SessionCommand, SessionError};
use crate::theme::rgba;

const LIST_MAX_HEIGHT: f32 = 360.;
const ROW_HEIGHT: f32 = 52.;

/// What the list shows: nothing yet, a read in flight, the rows of one read, or why it failed.
#[derive(Debug, Default)]
enum Listing {
    #[default]
    Idle,
    Loading(u64),
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

/// The site-permissions view inside Customize, with at most one read and one forget in flight.
#[derive(Debug, Default)]
pub(super) struct SitePermissions {
    open: bool,
    sequence: u64,
    listing: Listing,
    /// The forget in flight and the listed choice it removes, so its outcome can name it.
    forgetting: Option<(u64, RememberedPermission)>,
    status: Option<String>,
}

impl SitePermissions {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the view and returns the read that fills it.
    fn open(&mut self) -> SessionCommand {
        self.open = true;
        self.status = None;
        self.reload()
    }

    /// Hides the view; a forget still in flight reports through the footer instead.
    pub(super) fn close(&mut self) {
        self.open = false;
        self.listing = Listing::Idle;
        self.status = None;
    }

    fn reload(&mut self) -> SessionCommand {
        let request_id = self.next_request();
        self.listing = Listing::Loading(request_id);
        SessionCommand::RememberedPermissions { request_id }
    }

    fn next_request(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    /// Takes a read's rows if it is the read the view is waiting for.
    fn listed(&mut self, request_id: u64, result: Result<Vec<RememberedPermission>, SessionError>) {
        if !matches!(self.listing, Listing::Loading(waiting) if waiting == request_id) {
            return;
        }
        self.listing = match result {
            Ok(rows) => Listing::Loaded {
                generation: request_id,
                rows,
            },
            Err(error) => {
                Listing::Failed(format!("Remembered permissions could not be read: {error}"))
            }
        };
    }

    /// Starts forgetting `row` if it is still listed exactly as drawn; `None` refuses the click.
    fn forget(&mut self, row: &ListedRow) -> Option<SessionCommand> {
        if self.forgetting.is_some() || !self.open {
            return None;
        }
        let Listing::Loaded { generation, rows } = &self.listing else {
            return None;
        };
        if *generation != row.generation || !rows.contains(&row.listed) {
            self.status =
                Some("The list changed before that click, so nothing was forgotten.".to_owned());
            return None;
        }
        let request_id = self.next_request();
        self.forgetting = Some((request_id, row.listed.clone()));
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
        let (_, listed) = self
            .forgetting
            .take_if(|(waiting, _)| *waiting == request_id)?;
        let message = forget_message(&listed, result);
        if !self.open {
            return Some((message, None));
        }
        self.status = Some(message.clone());
        Some((message, Some(self.reload())))
    }

    /// The session stopped or refused a request, so nothing in flight can answer any more.
    pub(super) fn session_lost(&mut self) {
        if matches!(self.listing, Listing::Loading(_)) {
            self.listing = Listing::Failed(format!(
                "Remembered permissions could not be read: {}",
                SessionError::StorageUnavailable
            ));
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
    fn open_site_permissions(&mut self) {
        let read = self.home.site_permissions.open();
        self.request_site_permissions(read);
    }

    /// Queues a list or forget request; one the session cannot take ends its wait at once.
    fn request_site_permissions(&mut self, command: SessionCommand) {
        if !self.try_command(command) {
            self.home.site_permissions.session_lost();
        }
    }

    fn forget_site_permission(&mut self, row: &ListedRow) {
        let Some(forget) = self.home.site_permissions.forget(row) else {
            return;
        };
        self.request_site_permissions(forget);
    }

    pub(super) fn site_permissions_listed(
        &mut self,
        request_id: u64,
        result: Result<Vec<RememberedPermission>, SessionError>,
    ) {
        self.home.site_permissions.listed(request_id, result);
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
            Some(read) => self.request_site_permissions(read),
            None => self.message = message,
        }
    }

    /// Customize's way in: what the list holds and a Manage button.
    pub(super) fn site_permissions_entry(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let manage = control(
            div().id("site-permissions-manage"),
            Role::Button,
            "Manage site permissions",
        )
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
        .on_click(cx.listener(|this, _, _, cx| {
            this.open_site_permissions();
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
            .child(listing(state, palette, cx));
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
        .size(px(28.))
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgba(palette.icon_dim))
        .cursor_pointer()
        .child("‹")
        .on_click(cx.listener(|this, _, _, cx| {
            this.home.site_permissions.close();
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
}

/// The list itself, or a line saying why there is none.
fn listing(state: &SitePermissions, palette: &Palette, cx: &mut Context<BrowserView>) -> Div {
    let (generation, rows) = match &state.listing {
        Listing::Idle | Listing::Loading(_) => return note(palette, "Reading…"),
        Listing::Failed(error) => return note(palette, error),
        Listing::Loaded { generation, rows } => (*generation, rows),
    };
    if rows.is_empty() {
        return note(
            palette,
            "No choices are remembered. When a site asks, tick Remember for this site to keep \
             your answer.",
        );
    }
    let mut list = div()
        .id("site-permission-list")
        .max_h(px(LIST_MAX_HEIGHT))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(8.));
    for (index, listed) in rows.iter().enumerate() {
        let row = ListedRow {
            generation,
            listed: listed.clone(),
        };
        list = list.child(permission_row(palette, row, index, state.busy(), cx));
    }
    div().child(list)
}

/// One remembered choice: origin, kind and decision, and the button that forgets it.
fn permission_row(
    palette: &Palette,
    row: ListedRow,
    index: usize,
    busy: bool,
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
    let mut forget = control(div().id(id), Role::Button, button_name)
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
        state.listed(request_id, Ok(rows.clone()));
        rows.into_iter()
            .map(|listed| ListedRow {
                generation: request_id,
                listed,
            })
            .collect()
    }

    fn forget_id(command: Option<SessionCommand>) -> (u64, RememberedPermission) {
        let Some(SessionCommand::ForgetPermission { request_id, listed }) = command else {
            panic!("expected a forget request");
        };
        (request_id, listed)
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
        assert!(matches!(state.listing, Listing::Loading(waiting) if waiting == reread));
        assert!(!state.busy());
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
        let rows = loaded(
            &mut state,
            vec![choice(
                "https://gone.example.test",
                PermissionKind::Midi,
                true,
            )],
        );
        let (request_id, _) = forget_id(state.forget(&rows[0]));
        assert!(state.forgotten(request_id + 100, Ok(true)).is_none());
        let (message, _) = state.forgotten(request_id, Ok(false)).expect("answered");
        assert_eq!(
            message,
            "That choice had already changed or been forgotten, so nothing was removed."
        );

        let rows = loaded(
            &mut state,
            vec![choice(
                "https://gone.example.test",
                PermissionKind::Midi,
                true,
            )],
        );
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
            matches!(state.listing, Listing::Loading(_)),
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
    fn a_failed_read_names_its_cause() {
        let mut state = SitePermissions::default();
        let request_id = read_id(state.open());
        state.listed(request_id, Err(SessionError::StorageUnavailable));
        assert!(matches!(
            &state.listing,
            Listing::Failed(message) if message.ends_with("Local storage is unavailable")
        ));
    }
}
