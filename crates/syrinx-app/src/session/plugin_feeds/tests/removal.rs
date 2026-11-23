//! A removed plugin's rows and text leave both tiers, even across a closed index.

use super::*;

#[test]
fn a_plugin_removed_mid_session_leaves_both_tiers_at_the_next_pump() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert!(!titles(&mut catalogue, "synthetic").is_empty());

    // The runtime removed it and the store cascaded; the next pump hears it.
    assert_eq!(store.remove_plugin(PLUGIN.to_owned()), Ok(true));
    plugin.events.push(PluginEvent::Removed {
        plugin: PLUGIN.to_owned(),
    });
    let unread = feeds.notice(plugin.take_events());
    assert!(unread.is_empty());
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert!(titles(&mut catalogue, "synthetic").is_empty());
    assert!(documents(&index, "quillmark").is_empty());
    assert_eq!(store.plugin_feed_cursor(PLUGIN, FEED), Ok(None));
    assert_eq!(store.plugin_feed_entries(), Ok(Vec::new()));
    let namespace = Namespace::plugin_feed(PLUGIN, FEED).expect("a namespace");
    assert_eq!(index.is_fed(&namespace), Ok(false));
}

#[test]
fn text_left_by_a_plugin_removed_while_the_index_was_closed_goes_at_the_next_pull() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);

    // Removed while the index is closed: the rows go, and the text is reported as left.
    assert_eq!(store.remove_plugin(PLUGIN.to_owned()), Ok(true));
    let closed = feeds.forget_plugin(PLUGIN, None, &mut catalogue);
    assert_eq!(closed, Err(FeedError::Index(FullTextError::NeedsReopen)));
    assert!(titles(&mut catalogue, "synthetic").is_empty());
    assert_eq!(documents(&index, "quillmark"), ["a"]);

    // A new session's first pump, with the index open, finds and removes it.
    let mut feeds = PluginFeeds::restore(&store, &mut catalogue).expect("nothing kept");
    let mut sinks = Sinks {
        store: &store,
        index: Some(&mut index),
        catalogue: &mut catalogue,
        now: NOW,
        at: Instant::now(),
    };
    feeds.pump(&mut plugin, &mut sinks).expect("the sweep");
    assert!(documents(&index, "quillmark").is_empty());
}

#[test]
fn two_plugins_removed_while_the_index_is_closed_lose_their_rows_at_once_and_their_text_later() {
    let folder = Folder::new();
    let store = folder.store();
    let other = PluginInstall {
        plugin_id: OTHER.to_owned(),
        ..install()
    };
    store.save_plugin_install(other).expect("a second install");
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    let other_changed = PluginEvent::FeedChanged {
        plugin: OTHER.to_owned(),
        feed: OTHER_FEED.to_owned(),
    };
    assert!(feeds.notice([feed_changed(), other_changed]).is_empty());
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(titles(&mut catalogue, "synthetic").len(), 4);
    assert_eq!(documents(&index, "quillmark"), ["a", "a"]);

    // Both are removed while the index is closed: every row goes, the text waits.
    for removed in [PLUGIN, OTHER] {
        assert_eq!(store.remove_plugin(removed.to_owned()), Ok(true));
    }
    let removals = [PLUGIN, OTHER].map(|removed| PluginEvent::Removed {
        plugin: removed.to_owned(),
    });
    assert!(feeds.notice(removals).is_empty());
    let mut closed = Sinks {
        store: &store,
        index: None,
        catalogue: &mut catalogue,
        now: NOW,
        at: Instant::now(),
    };
    let reported = feeds.pump(&mut plugin, &mut closed);
    assert_eq!(reported, Err(FeedError::Index(FullTextError::NeedsReopen)));
    assert_eq!(
        feeds.pump(&mut plugin, &mut closed),
        Ok(()),
        "reported once"
    );
    assert!(titles(&mut catalogue, "synthetic").is_empty());
    assert_eq!(documents(&index, "quillmark"), ["a", "a"]);

    // Once the index is open again, the next turn removes both plugins' text.
    let mut open = Sinks {
        store: &store,
        index: Some(&mut index),
        catalogue: &mut catalogue,
        now: NOW,
        at: Instant::now(),
    };
    feeds.pump(&mut plugin, &mut open).expect("the retry");
    assert!(documents(&index, "quillmark").is_empty());
}
