//! A synthetic plugin implementing every `syrinx:plugin@0.1.0` export, with
//! commands that misbehave on purpose, for the host's contract tests.

mod bindings {
    wit_bindgen::generate!({ path: "../../../../../../wit/v0_1", world: "plugin" });

    #[cfg(target_arch = "wasm32")]
    use super::Plugin;
    #[cfg(target_arch = "wasm32")]
    export!(Plugin);
}

use std::collections::HashMap;
use std::time::Instant;

use bindings::exports::syrinx::plugin::forms::{
    ChoiceOption, Field, FieldError, FieldKind, FieldValue, FormSpec, NumberOptions, SubmitResult,
    TextFormat, TextOptions,
};
use bindings::exports::syrinx::plugin::search_feed::{Batch, CatalogueEntry, Change, Document};
use bindings::exports::syrinx::plugin::views::ViewRequest;
use bindings::exports::syrinx::plugin::{commands, events, forms, lifecycle, search_feed, views};
use bindings::syrinx::plugin::host::{self, LogLevel};
use bindings::syrinx::plugin::types::{Icon, Outcome, PluginError};

struct Plugin;

fn note(message: String) {
    host::log(LogLevel::Info, &message);
}

impl lifecycle::Guest for Plugin {
    fn activate(info: lifecycle::Activation) -> Result<(), PluginError> {
        note(format!(
            "activate {} granted={}",
            info.host_version,
            info.granted.len()
        ));
        if info.host_version.ends_with("panic-on-activate") {
            panic!("synthetic activation panic");
        }
        Ok(())
    }

    fn deactivate() {
        note("deactivate".into());
    }
}

impl commands::Guest for Plugin {
    fn run(id: String) -> Result<(), PluginError> {
        match id.as_str() {
            "test.echo" => echo(),
            "test.fail" => Err(PluginError::InvalidInput("synthetic failure".into())),
            "test.spin" => spin(),
            "test.grow" => grow(),
            "test.alloc" => alloc(),
            "test.open" => open_views(),
            "test.print" => print(),
            "test.panic" => panic!("synthetic panic"),
            "test.exit" => std::process::exit(3),
            "test.sleep" => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                Ok(())
            }
            "test.clock" => clock(),
            "test.views" => many_views(),
            "test.events" => many_events(),
            "test.chain" => open_chain(),
            "test.long-error" => Err(PluginError::Internal("e".repeat(10_000))),
            "test.cancel" => {
                host::cancel(1);
                Ok(())
            }
            other => Err(PluginError::NotFound(other.into())),
        }
    }
}

/// Exercise the plain host imports and report what came back.
fn echo() -> Result<(), PluginError> {
    host::settings_set("test.greeting", b"hello")?;
    let stored = host::settings_get("test.greeting")?;
    host::feed_changed("test.items");
    note(format!("echo now={} stored={:?}", host::now(), stored));
    Ok(())
}

fn spin() -> Result<(), PluginError> {
    let mut counter: u64 = 0;
    loop {
        counter = std::hint::black_box(counter.wrapping_add(1));
    }
}

/// Grow memory a page at a time until the host refuses; report it as a typed error.
fn grow() -> Result<(), PluginError> {
    #[cfg(target_arch = "wasm32")]
    loop {
        if core::arch::wasm32::memory_grow(0, 1) == usize::MAX {
            return Err(PluginError::Unavailable("memory growth refused".into()));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    Ok(())
}

/// Allocate as ordinary code does, until the allocator aborts.
fn alloc() -> Result<(), PluginError> {
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    loop {
        blocks.push(std::hint::black_box(vec![1; 1024 * 1024]));
    }
}

fn open_views() -> Result<(), PluginError> {
    let first = host::open_view("test.page", "syrinx-test:one")?;
    let second = host::open_view("test.page", "syrinx-test:refuse")?;
    host::set_view_metadata(first, "One", Some("3"))?;
    host::post_view_event(first, "test.hello", "{\"n\":1}")?;
    note(format!("opening {first} {second}"));
    Ok(())
}

/// Open views until the host refuses one, and report how many it allowed.
fn many_views() -> Result<(), PluginError> {
    let mut opened = 0;
    loop {
        match host::open_view("test.page", "syrinx-test:many") {
            Ok(_) => opened += 1,
            Err(error) => {
                note(format!("views opened={opened}"));
                return Err(error);
            }
        }
    }
}

/// Post 64 KiB events to view 1 until the host refuses one.
fn many_events() -> Result<(), PluginError> {
    let body = format!("\"{}\"", "b".repeat(64 * 1024));
    let mut posted = 0;
    loop {
        match host::post_view_event(1, "test.bulk", &body) {
            Ok(()) => posted += 1,
            Err(error) => {
                note(format!("events posted={posted}"));
                return Err(error);
            }
        }
    }
}

/// Each chained view opens the next from its own `opened`.
fn open_chain() -> Result<(), PluginError> {
    host::open_view("test.page", "syrinx-test:chain").map(|_| ())
}

fn print() -> Result<(), PluginError> {
    for line in 0..4001 {
        println!("synthetic output line {line}");
    }
    eprintln!("synthetic error line");
    Ok(())
}

/// Use what pulls in the clocks and the hash seed.
fn clock() -> Result<(), PluginError> {
    let started = Instant::now();
    let mut counts: HashMap<&str, u32> = HashMap::new();
    *counts.entry("tick").or_default() += 1;
    let wall = std::time::SystemTime::now();
    note(format!(
        "clock elapsed_ok={} counts={} wall_ok={}",
        started.elapsed().as_nanos() < u128::MAX,
        counts.len(),
        wall.elapsed().is_ok()
    ));
    Ok(())
}

impl forms::Guest for Plugin {
    fn form_for(id: String) -> Result<FormSpec, PluginError> {
        if id == "test.too-big" {
            return Ok(FormSpec {
                title: "Too big".into(),
                fields: Vec::new(),
                submit_label: "Go".into(),
            });
        }
        Ok(every_field_kind())
    }

    fn validate(_id: String, values: Vec<FieldValue>) -> Result<Vec<FieldError>, PluginError> {
        let name = values.iter().find(|value| value.name == "name");
        if name.is_some_and(|value| value.value == "boom") {
            return Err(PluginError::Internal("validation failed".into()));
        }
        if name.is_some_and(|value| !value.value.is_empty()) {
            return Ok(Vec::new());
        }
        Ok(vec![FieldError {
            name: "name".into(),
            message: "A name is required.".into(),
        }])
    }

    fn submit(_id: String, request: u64, values: Vec<FieldValue>) -> SubmitResult {
        let name = values
            .iter()
            .find(|value| value.name == "name")
            .map(|value| value.value.clone())
            .unwrap_or_default();
        match name.as_str() {
            "" => SubmitResult::Invalid(vec![FieldError {
                name: "name".into(),
                message: "Required.".into(),
            }]),
            "later" => SubmitResult::Pending(request),
            "wrong-id" => SubmitResult::Pending(request + 1),
            _ => SubmitResult::Finished(Outcome::Done(Some(format!("created {name}")))),
        }
    }
}

/// A form using every field kind and option, to round-trip each one.
fn every_field_kind() -> FormSpec {
    let text = |format| TextOptions {
        max_length: 80,
        placeholder: Some("Ada".into()),
        format,
    };
    let kinds = [
        ("name", FieldKind::Text(text(TextFormat::Plain))),
        ("email", FieldKind::Text(text(TextFormat::Email))),
        ("site", FieldKind::Text(text(TextFormat::WebAddress))),
        ("notes", FieldKind::Multiline(text(TextFormat::Plain))),
        (
            "count",
            FieldKind::Number(NumberOptions {
                min: Some(0.0),
                max: None,
                step: Some(0.5),
            }),
        ),
        ("value", FieldKind::Money("USD".into())),
        (
            "source",
            FieldKind::Choice(vec![ChoiceOption {
                value: "web".into(),
                label: "Web".into(),
            }]),
        ),
        ("urgent", FieldKind::Toggle),
        ("due", FieldKind::Date),
        ("at", FieldKind::DateTime),
    ];
    let fields = kinds
        .into_iter()
        .map(|(name, kind)| Field {
            name: name.into(),
            label: name.to_uppercase(),
            kind,
            required: name == "name",
            initial: None,
        })
        .collect();
    FormSpec {
        title: "New item".into(),
        fields,
        submit_label: "Create".into(),
    }
}

impl views::Guest for Plugin {
    fn opened(view: u64, kind: String, address: String) -> Result<(), PluginError> {
        note(format!("opened {view} {kind} {address}"));
        if address.ends_with("refuse") {
            return Err(PluginError::Denied("this view is refused".into()));
        }
        if address.ends_with("chain") {
            return open_chain();
        }
        Ok(())
    }

    fn message(request: ViewRequest) -> Result<String, PluginError> {
        note(format!(
            "message {} {} {}",
            request.view, request.request_id, request.method
        ));
        match request.method.as_str() {
            "bad-json" => Ok("{not json".into()),
            _ => Ok(format!("{{\"echo\":{}}}", request.json)),
        }
    }

    fn closed(view: u64) {
        note(format!("closed {view}"));
    }
}

impl search_feed::Guest for Plugin {
    fn changes(_feed: String, after: Option<String>, max: u32) -> Result<Batch, PluginError> {
        if after.as_deref() == Some("flood") {
            let flood = (0..=max)
                .map(|index| Change::Delete(format!("item-{index}")))
                .collect();
            return Ok(Batch {
                changes: flood,
                next_cursor: "end".into(),
                more: false,
            });
        }
        let mut keywords: Vec<String> = (0..20).map(|index| format!("keyword-{index}")).collect();
        keywords.push("k".repeat(121));
        let entry = CatalogueEntry {
            id: "item-1".into(),
            revision: 1,
            title: "Synthetic item".into(),
            subtitle: "example.test".into(),
            icon: Icon::List,
            last_used: Some(host::now()),
            keywords,
        };
        let document = Document {
            id: "item-1".into(),
            revision: 1,
            title: "Synthetic item".into(),
            body: "Made-up text.".into(),
        };
        let changes = vec![
            Change::Reset,
            Change::UpsertEntry(entry),
            Change::UpsertDocument(document),
            Change::Delete("item-0".into()),
        ];
        Ok(Batch {
            changes,
            next_cursor: "1".into(),
            more: false,
        })
    }

    fn resolve(_feed: String, id: String) -> Result<String, PluginError> {
        Ok(format!("syrinx-test:{id}"))
    }
}

impl events::Guest for Plugin {
    fn deliver(request: u64, outcome: Outcome) {
        let summary = match outcome {
            Outcome::Done(detail) => format!("done {detail:?}"),
            Outcome::Failed(error) => format!("failed {error:?}"),
            Outcome::Uncertain(detail) => format!("uncertain {detail}"),
        };
        note(format!("deliver {request} {summary}"));
    }
}
