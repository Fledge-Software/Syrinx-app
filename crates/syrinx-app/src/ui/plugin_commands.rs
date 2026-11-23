//! The palette's plugin commands as the window sees them (#29): each call goes to the session,
//! and what came of it is told on the status line until forms are drawn in the palette.

use syrinx_core::plugin_forms::{Failure, FailureKind, LateOutcome, Notice, Outcome, PluginAnswer};

use super::*;

impl BrowserView {
    /// The owner chose a plugin's command in the palette.
    pub(super) fn run_plugin_command(&mut self, plugin: &str, command: &str) {
        // Today `PaletteEvent::Run` has already ended the entry, so a form cannot be shown.
        let in_palette = self.palette.is_some();
        let (call, forgotten) = self.plugin_commands.choose(plugin, command, in_palette);
        self.tell_plugin_notice(forgotten);
        self.command(SessionCommand::Plugin(call));
    }

    /// The palette entry ended: its form is dropped, while a call that may write is kept. The
    /// text is for a set-aside call forgotten to make room, for the caller to show.
    pub(super) fn abandon_plugin_form(&mut self) -> Option<String> {
        self.plugin_commands.abandon().map(notice_text)
    }

    /// A plugin's worker stopped; a form still waiting on it is told so.
    pub(super) fn plugin_stopped(&mut self, plugin: &str) {
        let notice = self.plugin_commands.plugin_stopped(plugin);
        self.tell_plugin_notice(notice);
    }

    fn tell_plugin_notice(&mut self, notice: Option<Notice>) {
        if let Some(notice) = notice {
            self.message = notice_text(notice);
        }
    }

    /// The session's answer to a plugin call; one for an abandoned command changes nothing.
    pub(super) fn plugin_answered(&mut self, answer: PluginAnswer) {
        let notice = self.plugin_commands.receive(answer);
        self.tell_plugin_notice(notice);
    }

    /// The outcome of a submission that answered pending, if it is still the one awaited.
    pub(super) fn plugin_outcome(&mut self, late: LateOutcome) {
        let notice = self.plugin_commands.receive_outcome(late);
        self.tell_plugin_notice(notice);
    }
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
        Notice::Invalid(error) => format!("{}: {}", error.name, error.message),
        Notice::Pending => "Sent; waiting for the plugin to finish.".to_owned(),
        Notice::Forgotten {
            may_have_written: true,
        } => "It may have happened; check before trying again.".to_owned(),
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
    use syrinx_core::plugin_forms::FieldError;

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
                "New item: It may have happened; check before trying again.",
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
                Notice::Invalid(FieldError {
                    name: "name".to_owned(),
                    message: "Required.".to_owned(),
                }),
                "name: Required.",
            ),
        ];
        for (notice, expected) in cases {
            assert_eq!(notice_text(notice), expected);
        }
    }
}
