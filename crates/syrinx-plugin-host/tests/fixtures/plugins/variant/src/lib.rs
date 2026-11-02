//! A synthetic plugin built against a changed copy of the contract (see
//! build.rs), one change per feature, so the host's tests can show each
//! change is refused by name. Without a feature it is a plain 0.1.0 plugin.

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

    #[cfg(target_arch = "wasm32")]
    use super::Plugin;
    #[cfg(target_arch = "wasm32")]
    export!(Plugin);
}

use bindings::exports::syrinx::plugin::forms::{FieldError, FieldValue, FormSpec, SubmitResult};
use bindings::exports::syrinx::plugin::search_feed::Batch;
use bindings::exports::syrinx::plugin::views::ViewRequest;
use bindings::exports::syrinx::plugin::{commands, events, forms, lifecycle, search_feed, views};
use bindings::syrinx::plugin::types::{Outcome, PluginError};

struct Plugin;

fn unused() -> PluginError {
    PluginError::NotFound("not used".into())
}

/// The one command; each variant uses what its change added, so the import is kept.
fn command() -> Result<(), PluginError> {
    #[cfg(feature = "since-0-1-1")]
    bindings::syrinx::plugin::extra::ping();
    #[cfg(feature = "widgets")]
    bindings::syrinx::plugin::host::widget_changed("variant.widget");
    #[cfg(feature = "filesystem")]
    {
        let entries = std::fs::read_dir("/").map(|entries| entries.count());
        bindings::syrinx::plugin::host::log(
            bindings::syrinx::plugin::host::LogLevel::Info,
            &format!("{entries:?}"),
        );
    }
    Ok(())
}

impl lifecycle::Guest for Plugin {
    fn activate(_info: lifecycle::Activation) -> Result<(), PluginError> {
        Ok(())
    }

    fn deactivate() {}
}

impl commands::Guest for Plugin {
    #[cfg(not(feature = "renamed-export"))]
    fn run(_id: String) -> Result<(), PluginError> {
        command()
    }

    #[cfg(feature = "renamed-export")]
    fn start(_id: String) -> Result<(), PluginError> {
        command()
    }
}

impl forms::Guest for Plugin {
    fn form_for(_id: String) -> Result<FormSpec, PluginError> {
        Err(unused())
    }

    fn validate(_id: String, _values: Vec<FieldValue>) -> Result<Vec<FieldError>, PluginError> {
        Err(unused())
    }

    fn submit(_id: String, _request: u64, _values: Vec<FieldValue>) -> SubmitResult {
        SubmitResult::Finished(Outcome::Failed(unused()))
    }
}

impl views::Guest for Plugin {
    fn opened(_view: u64, _kind: String, _address: String) -> Result<(), PluginError> {
        Err(unused())
    }

    fn message(_request: ViewRequest) -> Result<String, PluginError> {
        Err(unused())
    }

    fn closed(_view: u64) {}
}

impl search_feed::Guest for Plugin {
    fn changes(_feed: String, _after: Option<String>, _max: u32) -> Result<Batch, PluginError> {
        Err(unused())
    }

    fn resolve(_feed: String, _id: String) -> Result<String, PluginError> {
        Err(unused())
    }
}

impl events::Guest for Plugin {
    fn deliver(_request: u64, _outcome: Outcome) {}
}
