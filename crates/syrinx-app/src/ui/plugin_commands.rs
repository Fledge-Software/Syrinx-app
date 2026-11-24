//! The palette's plugin commands as the window sees them (#29): each call goes to the session,
//! and what came of it is told on the status line until forms are drawn in the palette.

use syrinx_core::plugin_forms::{Failure, FailureKind, LateOutcome, Notice, Outcome, PluginAnswer};

use super::*;

impl BrowserView {
    /// The owner chose a plugin's command in the palette.
    pub(super) fn run_plugin_command(&mut self, plugin: &str, command: &str, cx: &mut App) {
        // Today `PaletteEvent::Run` has already ended the entry, so a form cannot be shown.
        let in_palette = self.palette.is_some();
        // The title the palette showed names the command if it is set aside later.
        let title = self
            .commands
            .find(command)
            .map_or_else(|| command.to_owned(), |(_, found)| found.title.clone());
        let (call, forgotten) = self
            .plugin_commands
            .choose(plugin, command, &title, in_palette);
        self.tell_plugin_notice(forgotten, cx);
        self.command(SessionCommand::Plugin(call));
    }

    /// The palette entry ended: its form is dropped, while a call that may write is kept. The
    /// text is for a set-aside call forgotten to make room, for the caller to show.
    pub(super) fn abandon_plugin_form(&mut self) -> Option<String> {
        self.plugin_commands.abandon().map(notice_text)
    }

    /// A plugin's worker stopped; each submission still waiting on it is told so, on one line.
    pub(super) fn plugin_stopped(&mut self, plugin: &str, cx: &mut App) {
        let notices = self.plugin_commands.plugin_stopped(plugin);
        self.tell_plugin_notices(notices, cx);
    }

    fn tell_plugin_notice(&mut self, notice: Option<Notice>, cx: &mut App) {
        self.tell_plugin_notices(notice, cx);
    }

    /// Show `notices`, joined in order, on the window's status line, where they stay once the
    /// palette closes, and on an open palette's own line too; none leaves both as they are.
    fn tell_plugin_notices(&mut self, notices: impl IntoIterator<Item = Notice>, cx: &mut App) {
        let Some(text) = notices_text(notices) else {
            return;
        };
        if let Some(popup) = &self.palette {
            popup
                .view
                .update(cx, |palette, cx| palette.show_outcome(&text, cx));
        }
        self.message = text;
    }

    /// The session's answer to a plugin call; one for an abandoned command changes nothing.
    pub(super) fn plugin_answered(&mut self, answer: PluginAnswer, cx: &mut App) {
        let notice = self.plugin_commands.receive(answer);
        self.tell_plugin_notice(notice, cx);
    }

    /// The outcome of a submission that answered pending, if it is still the one awaited.
    pub(super) fn plugin_outcome(&mut self, late: LateOutcome, cx: &mut App) {
        let notice = self.plugin_commands.receive_outcome(late);
        self.tell_plugin_notice(notice, cx);
    }
}

/// Several notices on one status line, in order; `None` when there are none.
fn notices_text(notices: impl IntoIterator<Item = Notice>) -> Option<String> {
    let texts: Vec<String> = notices.into_iter().map(notice_text).collect();
    (!texts.is_empty()).then(|| texts.join(" "))
}

/// The status line's one presentation of every plugin result.
fn notice_text(notice: Notice) -> String {
    match notice {
        Notice::Outcome(Outcome::Done(None)) => "Done.".to_owned(),
        Notice::Outcome(Outcome::Done(Some(detail))) => format!("Done: {detail}"),
        Notice::Outcome(Outcome::Failed(failure)) | Notice::Failed(failure) => {
            failure_text(&failure)
        }
        Notice::Outcome(Outcome::Uncertain(detail)) => {
            format!("This may have happened ({detail}). Check before trying again.")
        }
        Notice::FormReady { title } => format!("{title}: fill in the form."),
        Notice::FormNotShown { title } => {
            format!("{title} has a form, which the palette cannot show yet.")
        }
        Notice::Invalid { field, message } => format!("{field}: {message}"),
        Notice::Pending => "Sent; waiting for the plugin to finish.".to_owned(),
        Notice::Forgotten {
            may_have_written: true,
        } => "This may have happened. Check before trying again.".to_owned(),
        Notice::Forgotten {
            may_have_written: false,
        } => "Its result can no longer be shown.".to_owned(),
        Notice::SetAside { command, notice } => format!("{command}: {}", notice_text(*notice)),
    }
}

/// A failure in the plugin's words, named by its kind; the host's own reasons stand alone.
fn failure_text(failure: &Failure) -> String {
    let kind = match failure.kind {
        FailureKind::Host => return failure.message.clone(),
        FailureKind::InvalidInput => "Invalid input",
        FailureKind::NotFound => "Not found",
        FailureKind::Denied => "Not allowed",
        FailureKind::Revoked => "Permission withdrawn",
        FailureKind::Unavailable => "Unavailable",
        FailureKind::Internal => "The plugin failed",
        FailureKind::Cancelled => "Cancelled",
    };
    format!("{kind}: {}", failure.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_result_has_one_status_line_wording() {
        let failure = |kind| Failure {
            kind,
            message: "synthetic".to_owned(),
        };
        let cases = [
            (Notice::Outcome(Outcome::Done(None)), "Done."),
            (
                Notice::Outcome(Outcome::Done(Some("made".to_owned()))),
                "Done: made",
            ),
            (
                Notice::Outcome(Outcome::Failed(failure(FailureKind::NotFound))),
                "Not found: synthetic",
            ),
            (Notice::Failed(failure(FailureKind::Host)), "synthetic"),
            (
                Notice::Outcome(Outcome::Uncertain("timed out".to_owned())),
                "This may have happened (timed out). Check before trying again.",
            ),
            (
                Notice::FormReady {
                    title: "New item".to_owned(),
                },
                "New item: fill in the form.",
            ),
            (
                Notice::FormNotShown {
                    title: "New item".to_owned(),
                },
                "New item has a form, which the palette cannot show yet.",
            ),
            (Notice::Pending, "Sent; waiting for the plugin to finish."),
            (
                Notice::SetAside {
                    command: "New item".to_owned(),
                    notice: Box::new(Notice::Forgotten {
                        may_have_written: true,
                    }),
                },
                "New item: This may have happened. Check before trying again.",
            ),
            (
                Notice::SetAside {
                    command: "test.new-item".to_owned(),
                    notice: Box::new(Notice::Forgotten {
                        may_have_written: false,
                    }),
                },
                "test.new-item: Its result can no longer be shown.",
            ),
            (
                Notice::SetAside {
                    command: "test.echo".to_owned(),
                    notice: Box::new(Notice::Outcome(Outcome::Done(None))),
                },
                "test.echo: Done.",
            ),
            (
                Notice::Invalid {
                    field: "Name".to_owned(),
                    message: "Required.".to_owned(),
                },
                "Name: Required.",
            ),
        ];
        for (notice, expected) in cases {
            assert_eq!(notice_text(notice), expected);
        }
    }

    #[test]
    fn several_notices_share_one_line_in_order_and_none_says_nothing() {
        let named = |command: &str| Notice::SetAside {
            command: command.to_owned(),
            notice: Box::new(Notice::Outcome(Outcome::Done(None))),
        };
        assert_eq!(
            notices_text([named("First"), named("Second")]),
            Some("First: Done. Second: Done.".to_owned())
        );
        assert_eq!(notices_text(Vec::new()), None);
    }
}
