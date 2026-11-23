//! Proves the application hears a plugin's changed search feeds and can pull them (#141): a
//! `feed-changed` signal and each activation under an `index` grant announce the feed, and
//! nothing is announced or pulled without that grant.

use syrinx_plugin_host::bindings::exports::syrinx::plugin::search_feed::{Batch, Change};
use syrinx_plugin_host::worker::PluginEvent;

use super::*;

/// The fixture's one declared feed, asked to be indexed.
const INDEX: &str = "[[capabilities]]\nkind = \"index\"\nfeeds = [\"test.items\"]\nreason = \"Indexes synthetic items.\"\nrequired = false\n";

/// The feeds announced since the last look, as (plugin, feed).
fn announced(runtime: &OwnerRuntime) -> Vec<(String, String)> {
    runtime
        .take_events()
        .into_iter()
        .filter_map(|event| match event {
            PluginEvent::FeedChanged { plugin, feed } => Some((plugin, feed)),
            _ => None,
        })
        .collect()
}

/// The fixture, asking for `index`, with `storage` granted: `echo` keeps a setting.
fn install_asking_for_index(runtime: &mut OwnerRuntime) {
    install(runtime, "test", &format!("{INDEX}{STORAGE}"));
    runtime
        .grant("test", CapabilityKind::Storage, 20)
        .expect("storage granted");
}

fn test_items() -> Vec<(String, String)> {
    vec![("test".to_owned(), "test.items".to_owned())]
}

fn pull(runtime: &mut OwnerRuntime, feed: &str) -> CallResult<Result<Batch, PluginError>> {
    answer(runtime.pull_feed("test", feed, None, 64))
}

#[test]
fn activation_and_feed_changed_announce_the_feed_under_an_index_grant() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = start(&store);
    install_asking_for_index(&mut runtime);
    runtime
        .grant("test", CapabilityKind::Index, 20)
        .expect("granted");

    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    // One announcement at activation, one for the echo's own `feed-changed`.
    assert_eq!(announced(&runtime), [test_items(), test_items()].concat());
    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    assert_eq!(announced(&runtime), test_items(), "no second activation");
    close(runtime, store);
}

#[test]
fn without_an_index_grant_nothing_is_announced_or_pulled() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = start(&store);
    install_asking_for_index(&mut runtime);

    assert_eq!(echo(&mut runtime, "test"), Ok(Ok(())));
    assert!(announced(&runtime).is_empty());
    assert!(matches!(
        runtime.pull_feed("test", "test.items", None, 64),
        Err(HostError::Refused(_))
    ));
    assert!(matches!(
        runtime.resolve_feed_entry("test", "test.items", "item-1"),
        Err(HostError::Refused(_))
    ));
    close(runtime, store);
}

#[test]
fn a_granted_feed_is_pulled_bounded_and_resolved() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = start(&store);
    install_asking_for_index(&mut runtime);
    runtime
        .grant("test", CapabilityKind::Index, 20)
        .expect("granted");

    let batch = pull(&mut runtime, "test.items")
        .expect("answered")
        .expect("a batch");
    assert_eq!(batch.next_cursor, "1");
    assert!(!batch.more);
    // The feed declares only the catalogue tier, so the fixture's document is dropped.
    assert_eq!(batch.changes.len(), 3);
    assert!(
        !batch
            .changes
            .iter()
            .any(|change| matches!(change, Change::UpsertDocument(_)))
    );
    assert!(
        logged(&mut runtime, "test")
            .iter()
            .any(|line| line.contains("full-text document for a tier the feed does not declare"))
    );
    let Change::UpsertEntry(entry) = &batch.changes[1] else {
        panic!("the fixture's second change is an entry");
    };
    assert_eq!(
        entry.keywords.len(),
        syrinx_plugin_host::limits::MAX_KEYWORDS
    );
    let address = answer(runtime.resolve_feed_entry("test", "test.items", "item-1"));
    assert_eq!(address, Ok(Ok("syrinx-test:item-1".to_owned())));
    assert!(matches!(
        pull(&mut runtime, "test.undeclared"),
        Err(HostError::Refused(_))
    ));
    close(runtime, store);
}

#[test]
fn removing_a_plugin_tells_the_application_once_the_store_kept_it() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut runtime = start(&store);
    install_asking_for_index(&mut runtime);
    runtime.take_events();

    runtime.remove("test").expect("removed");
    let removed: Vec<String> = runtime
        .take_events()
        .into_iter()
        .filter_map(|event| match event {
            PluginEvent::Removed { plugin } => Some(plugin),
            _ => None,
        })
        .collect();
    assert_eq!(removed, ["test"]);
    close(runtime, store);
}
