//! A synthetic plugin that uses only `lifecycle` and one command, answering
//! every other export with the hand-written default stubs the template ships.

mod bindings {
    wit_bindgen::generate!({ path: "../../../../../../wit/v0_1", world: "plugin" });

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

fn unused(what: &str) -> PluginError {
    PluginError::NotFound(format!("{what} is not used by this plugin"))
}

impl lifecycle::Guest for Plugin {
    fn activate(_info: lifecycle::Activation) -> Result<(), PluginError> {
        Ok(())
    }

    fn deactivate() {}
}

impl commands::Guest for Plugin {
    fn run(id: String) -> Result<(), PluginError> {
        if id == "mini.hello" {
            return Ok(());
        }
        Err(PluginError::NotFound(id))
    }
}

impl forms::Guest for Plugin {
    fn form_for(_id: String) -> Result<FormSpec, PluginError> {
        Err(unused("forms"))
    }

    fn validate(_id: String, _values: Vec<FieldValue>) -> Result<Vec<FieldError>, PluginError> {
        Err(unused("forms"))
    }

    fn submit(_id: String, _request: u64, _values: Vec<FieldValue>) -> SubmitResult {
        SubmitResult::Finished(Outcome::Failed(unused("forms")))
    }
}

impl views::Guest for Plugin {
    fn opened(_view: u64, _kind: String, _address: String) -> Result<(), PluginError> {
        Err(unused("views"))
    }

    fn message(_request: ViewRequest) -> Result<String, PluginError> {
        Err(unused("views"))
    }

    fn closed(_view: u64) {}
}

impl search_feed::Guest for Plugin {
    fn changes(_feed: String, _after: Option<String>, _max: u32) -> Result<Batch, PluginError> {
        Err(unused("search feeds"))
    }

    fn resolve(_feed: String, _id: String) -> Result<String, PluginError> {
        Err(unused("search feeds"))
    }
}

impl events::Guest for Plugin {
    fn deliver(_request: u64, _outcome: Outcome) {}
}
