//! A synthetic plugin built against a changed copy of the contract (see
//! build.rs), one change per feature, so the host's tests can show each
//! change is refused by name. Without a feature it is a plain plugin of the
//! current contract; `contract-0-1-0` builds one of the contract before #280, and `service`
//! reads from the address the test approved.

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

/// The `service` variant's commands: `variant.<name>` reads `/<name>` from the approved address
/// and logs each header it was handed, then the status and body; `variant.other-host` asks for
/// another host by name.
#[cfg(feature = "service")]
mod service {
    use super::bindings::syrinx::plugin::host::{self, LogLevel};
    use super::bindings::syrinx::plugin::service::{self, Header, Method};
    use super::bindings::syrinx::plugin::types::PluginError;

    pub fn fetch(id: &str) -> Result<(), PluginError> {
        let name = id.strip_prefix("variant.").unwrap_or(id);
        let path = match name {
            "other-host" => "https://other.example.test/notes".to_owned(),
            name => format!("/{name}"),
        };
        let accept = Header {
            name: "accept".into(),
            value: "text/plain".into(),
        };
        let response = service::request(Method::Get, &path, &[accept], &[])?;
        for header in &response.headers {
            host::log(
                LogLevel::Info,
                &format!("header {}: {}", header.name, header.value),
            );
        }
        let body = String::from_utf8_lossy(&response.body);
        host::log(LogLevel::Info, &format!("{} {body}", response.status));
        Ok(())
    }
}

fn unused() -> PluginError {
    PluginError::NotFound("not used".into())
}

/// The one command; each variant uses what its change added, so the import is kept.
#[cfg(not(feature = "service"))]
fn command() -> Result<(), PluginError> {
    #[cfg(feature = "since-0-1-2")]
    bindings::syrinx::plugin::extra::ping();
    #[cfg(feature = "widgets")]
    bindings::syrinx::plugin::host::widget_changed("variant.widget");
    #[cfg(feature = "imports-documents")]
    bindings::syrinx::plugin::documents::read();
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
    #[cfg(all(not(feature = "renamed-export"), not(feature = "service")))]
    fn run(_id: String) -> Result<(), PluginError> {
        command()
    }

    #[cfg(feature = "service")]
    fn run(id: String) -> Result<(), PluginError> {
        service::fetch(&id)
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
