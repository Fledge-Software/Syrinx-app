//! Shows site consent above the native page and commits remembered choices before replying.

use super::*;
use atlas_store::{PermissionKind, StoreError};

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
        let Some(index) = self
            .permissions
            .iter()
            .position(|prompt| prompt.id == id && prompt.request == request)
        else {
            return;
        };
        match result {
            Ok(None) => self.permissions[index].waiting = false,
            result => {
                let allowed = match result {
                    Ok(Some(allowed)) => allowed,
                    Err(error) => {
                        self.message = format!(
                            "Permission denied because its saved choice could not be read or saved: {error}"
                        );
                        false
                    }
                    Ok(None) => unreachable!("handled above"),
                };
                self.permissions.remove(index);
                self.page_command(BrowserCommand::ResolvePermission {
                    id,
                    request,
                    allowed,
                });
            }
        }
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

    pub(super) fn permission_bar(&self, cx: &mut Context<Self>) -> Div {
        let Some(index) = self.active_permission() else {
            return div();
        };
        let prompt = &self.permissions[index];
        let (id, request) = (prompt.id, prompt.request);
        div()
            .h(px(HEIGHT))
            .flex_shrink_0()
            .px_4()
            .py_2()
            .bg(rgb(0x24262c))
            .flex()
            .flex_col()
            .gap_2()
            .child(format!(
                "{} wants to {}",
                prompt.origin,
                prompt.kind.label()
            ))
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
            )
    }

    pub(super) fn permission_finished(&mut self, id: u64, request: u64) {
        self.permissions
            .retain(|prompt| prompt.id != id || prompt.request != request);
    }
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
