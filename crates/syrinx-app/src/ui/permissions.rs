//! Shows site consent above the native page and commits remembered choices before replying.

use super::*;
use syrinx_store::{PermissionKind, StoreError};

pub(super) const HEIGHT: f32 = 96.;

pub(super) struct Prompt {
    id: u64,
    request: u64,
    origin: String,
    kind: PermissionKind,
    remember: bool,
    waiting: bool,
}

impl BrowserView {
    pub(super) fn forget_permissions(&mut self, id: u64) {
        self.permissions.retain(|prompt| prompt.id != id);
    }

    pub(super) fn permission_requested(
        &mut self,
        id: u64,
        request: u64,
        uri: &str,
        kind: PermissionKind,
    ) {
        let Some(origin) = canonical_origin(uri) else {
            self.page_command(BrowserCommand::ResolvePermission {
                id,
                request,
                allowed: false,
            });
            return;
        };
        if self.closing
            || self.session_stopped
            || !self.model.tabs.iter().any(|tab| tab.id.get() == id)
        {
            self.page_command(BrowserCommand::ResolvePermission {
                id,
                request,
                allowed: false,
            });
            return;
        }
        self.permissions.push(Prompt {
            id,
            request,
            origin: origin.clone(),
            kind,
            remember: false,
            waiting: true,
        });
        self.command(SessionCommand::CheckPermission {
            id,
            request,
            origin,
            kind,
        });
    }

    pub(super) fn permission_result(
        &mut self,
        id: u64,
        request: u64,
        result: Result<Option<bool>, StoreError>,
    ) {
        // Only a save can change remembered choices; lookups leave Settings' list alone.
        if answers_remember(&self.permissions, id, request) {
            self.site_permissions_changed();
        }
        let Some(command) = resolve_saved_choice(
            &mut self.permissions,
            &mut self.message,
            id,
            request,
            result,
        ) else {
            return;
        };
        self.page_command(command);
    }

    pub(super) fn active_permission(&self) -> Option<usize> {
        self.permissions
            .iter()
            .position(|prompt| Some(prompt.id) == self.shown.map(TabId::get) && !prompt.waiting)
    }

    pub(super) fn permission_height(&self) -> f32 {
        if self.active_permission().is_some() {
            HEIGHT
        } else {
            0.
        }
    }

    fn permission_action(&self, id: u64, request: u64) -> Option<usize> {
        let index = self.active_permission()?;
        matches_prompt(
            &self.permissions[index],
            self.shown.map(TabId::get),
            id,
            request,
        )
        .then_some(index)
    }

    fn decide_permission(&mut self, id: u64, request: u64, allowed: bool) {
        let Some(index) = self.permission_action(id, request) else {
            return;
        };
        let prompt = &mut self.permissions[index];
        if prompt.remember {
            prompt.waiting = true;
            let command = SessionCommand::RememberPermission {
                id: prompt.id,
                request: prompt.request,
                origin: prompt.origin.clone(),
                kind: prompt.kind,
                allowed,
            };
            self.command(command);
        } else {
            let prompt = self.permissions.remove(index);
            self.page_command(BrowserCommand::ResolvePermission {
                id: prompt.id,
                request: prompt.request,
                allowed,
            });
        }
    }

    pub(super) fn permission_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let index = self.active_permission()?;
        let prompt = &self.permissions[index];
        let (id, request) = (prompt.id, prompt.request);
        let question = format!("{} wants to {}", prompt.origin, prompt.kind.label());
        let bar = div()
            .h(px(HEIGHT))
            .flex_shrink_0()
            .px_4()
            .py_2()
            .bg(rgb(0x24262c))
            .flex()
            .flex_col()
            .gap_2()
            // Plain text has no accessibility node; a named label lets NVDA read the question.
            .child(
                div()
                    .id("permission-question")
                    .role(gpui::Role::Label)
                    .aria_label(question.clone())
                    .child(question),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        button(
                            gpui::SharedString::from(format!("permission-remember-{id}-{request}")),
                            if prompt.remember {
                                "☑ Remember for this site"
                            } else {
                                "☐ Remember for this site"
                            },
                        )
                        .role(gpui::Role::CheckBox)
                        .aria_label("Remember for this site")
                        .aria_toggled(if prompt.remember {
                            gpui::Toggled::True
                        } else {
                            gpui::Toggled::False
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(index) = this.permission_action(id, request) {
                                this.permissions[index].remember =
                                    !this.permissions[index].remember;
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            gpui::SharedString::from(format!("permission-deny-{id}-{request}")),
                            "Deny",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.decide_permission(id, request, false);
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            gpui::SharedString::from(format!("permission-allow-{id}-{request}")),
                            "Allow",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.decide_permission(id, request, true);
                            cx.notify();
                        })),
                    ),
            );
        Some(bar)
    }

    pub(super) fn permission_finished(&mut self, id: u64, request: u64) {
        self.permissions
            .retain(|prompt| prompt.id != id || prompt.request != request);
    }
}

// Keep stale replies inert and leave an unknown choice available for explicit consent.
fn resolve_saved_choice(
    prompts: &mut Vec<Prompt>,
    message: &mut String,
    id: u64,
    request: u64,
    result: Result<Option<bool>, StoreError>,
) -> Option<BrowserCommand> {
    let index = prompts
        .iter()
        .position(|prompt| prompt.id == id && prompt.request == request)?;
    let allowed = match result {
        Ok(None) => {
            prompts[index].waiting = false;
            return None;
        }
        Ok(Some(allowed)) => allowed,
        Err(error) => {
            *message = format!(
                "Permission denied because its saved choice could not be read or saved: {error}"
            );
            false
        }
    };
    prompts.remove(index);
    Some(BrowserCommand::ResolvePermission {
        id,
        request,
        allowed,
    })
}

/// Whether a session answer is to a remembered choice being saved. Remember can be ticked
/// only on a shown prompt, so a lookup's answer never finds it set.
fn answers_remember(prompts: &[Prompt], id: u64, request: u64) -> bool {
    prompts
        .iter()
        .any(|prompt| prompt.id == id && prompt.request == request && prompt.remember)
}

fn matches_prompt(prompt: &Prompt, shown: Option<u64>, id: u64, request: u64) -> bool {
    shown == Some(id) && prompt.id == id && prompt.request == request && !prompt.waiting
}

fn canonical_origin(uri: &str) -> Option<String> {
    let Ok(url) = url::Url::parse(uri) else {
        return None;
    };
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_prompt() -> Prompt {
        Prompt {
            id: 1,
            request: 2,
            origin: "https://example.test".into(),
            kind: PermissionKind::Camera,
            remember: false,
            waiting: true,
        }
    }

    #[test]
    fn only_a_remembered_save_counts_as_a_changed_choice() {
        let mut prompts = vec![pending_prompt()];
        assert!(!answers_remember(&prompts, 1, 2), "a lookup's answer");
        prompts[0].remember = true;
        assert!(answers_remember(&prompts, 1, 2), "the save's answer");
        assert!(!answers_remember(&prompts, 1, 3), "another request");
        assert!(!answers_remember(&prompts, 9, 2), "another page");
    }

    #[test]
    fn unknown_choice_waits_for_consent_and_stale_replies_are_inert() {
        let mut prompts = vec![pending_prompt()];
        let mut message = String::new();
        for (id, request) in [(1, 3), (3, 2)] {
            assert!(
                resolve_saved_choice(&mut prompts, &mut message, id, request, Ok(Some(true)))
                    .is_none()
            );
            assert!(prompts[0].waiting);
        }
        assert!(resolve_saved_choice(&mut prompts, &mut message, 1, 2, Ok(None)).is_none());
        assert_eq!(prompts.len(), 1);
        assert!(!prompts[0].waiting);
        assert!(message.is_empty());
    }

    #[test]
    fn remembered_choices_finish_once_and_storage_errors_deny() {
        for (result, expected, failed) in [
            (Ok(Some(true)), true, false),
            (Ok(Some(false)), false, false),
            (
                Err(StoreError::InvalidEncoding(
                    syrinx_store::EncodingField::Permission,
                )),
                false,
                true,
            ),
        ] {
            let mut prompts = vec![pending_prompt()];
            let mut message = String::new();
            assert!(matches!(
                resolve_saved_choice(&mut prompts, &mut message, 1, 2, result),
                Some(BrowserCommand::ResolvePermission { id: 1, request: 2, allowed }) if allowed == expected
            ));
            assert!(prompts.is_empty());
            assert_eq!(message.contains("Permission denied"), failed);
            assert!(
                resolve_saved_choice(&mut prompts, &mut message, 1, 2, Ok(Some(true))).is_none()
            );
        }
    }

    #[test]
    fn stale_consent_cannot_grant_a_replacement_request_or_tab() {
        let mut prompt = Prompt {
            id: 1,
            request: 2,
            origin: "https://example.test".into(),
            kind: PermissionKind::Camera,
            remember: false,
            waiting: false,
        };
        assert!(matches_prompt(&prompt, Some(1), 1, 2));
        assert!(!matches_prompt(&prompt, Some(1), 1, 1));
        assert!(!matches_prompt(&prompt, Some(3), 1, 2));
        assert!(!matches_prompt(&prompt, Some(1), 3, 2));
        prompt.waiting = true;
        assert!(!matches_prompt(&prompt, Some(1), 1, 2));
    }

    #[test]
    fn remembered_sites_are_origins_not_paths_or_prefixes() {
        assert_eq!(
            canonical_origin("https://EXAMPLE.test:443/private?a=b"),
            Some("https://example.test".into())
        );
        assert_eq!(
            canonical_origin("http://example.test:8080/path"),
            Some("http://example.test:8080".into())
        );
        assert_ne!(
            canonical_origin("https://example.test"),
            canonical_origin("https://example.test.other.test")
        );
        assert_eq!(canonical_origin("about:blank"), None);
        assert_eq!(canonical_origin("file:///synthetic"), None);
        assert_eq!(canonical_origin("not a URL"), None);
    }
}
