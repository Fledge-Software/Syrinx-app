//! Keeping batches: replay, failures before the cursor is kept, refeeds, resets, the
//! budget and page-text expiry leaving plugin documents (D40).

use super::*;

#[test]
fn an_announced_feed_reaches_both_tiers_and_a_replay_changes_nothing() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let second = batch(vec![entry("b", 2, "Synthetic bravo two")], "c2", false);
    let mut plugin = Scripted::default()
        .answer(None, first_batch())
        .answer(Some("c1"), second);

    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
    assert_eq!(documents(&index, "quillmark"), ["a"]);
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c1".to_owned()))
    );

    // The first batch again, answered from its own cursor as a replay would be, is a no-op.
    let name = FeedName {
        plugin: PLUGIN.to_owned(),
        feed: FEED.to_owned(),
    };
    feeds.pull = Some(Pull {
        name,
        from_start: false,
        answer: Box::new(Ready(first_batch())),
        batches: 0,
    });
    let mut replay = Scripted::default();
    settle(&mut feeds, &mut replay, &store, &mut index, &mut catalogue);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
    assert_eq!(documents(&index, "quillmark"), ["a"]);

    // The next announcement goes on from the kept cursor.
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(plugin.requests, [None, Some("c1".to_owned())]);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo two"]
    );
}

#[test]
fn an_older_revision_after_a_newer_one_leaves_the_newer_row_showing() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let newer = batch(vec![entry("a", 2, "Synthetic newer")], "c1", false);
    let older = batch(vec![entry("a", 1, "Synthetic older")], "c2", false);
    let mut plugin = Scripted::default()
        .answer(None, newer)
        .answer(Some("c1"), older);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(titles(&mut catalogue, "synthetic"), ["Synthetic newer"]);
    assert_eq!(kept_rows(&store), [("a".to_owned(), 2)]);
}

#[test]
fn a_failed_index_commit_keeps_the_old_cursor_and_the_next_pull_completes_both_tiers() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let second = batch(
        vec![
            entry("c", 1, "Synthetic charlie"),
            document("c", 1, "copperleaf"),
            Change::Delete("a".to_owned()),
        ],
        "c2",
        false,
    );
    let mut plugin = Scripted::default()
        .answer(None, first_batch())
        .answer(Some("c1"), second);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);

    // The second batch reaches apply, and the index commit fails.
    announce(&mut feeds);
    let mut failing = FailingIndex {
        index: &mut index,
        passes: 0,
        failures: 1,
        remove_failures: 0,
    };
    let failed = settle_with(
        &mut feeds,
        &mut plugin,
        &store,
        &mut failing,
        &mut catalogue,
    );
    assert_eq!(failed, Err(FeedError::Index(FullTextError::Io)));
    // Nothing of the batch was kept: the store would be ahead of the index otherwise.
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c1".to_owned()))
    );
    assert_eq!(
        kept_rows(&store),
        [("a".to_owned(), 1), ("b".to_owned(), 1)]
    );

    // A restart shows the kept rows without running the plugin, then pulls from c1 again.
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::restore(&store, &mut catalogue).expect("restored rows");
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(plugin.requests.last(), Some(&Some("c1".to_owned())));
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic bravo", "Synthetic charlie"]
    );
    assert_eq!(documents(&index, "copperleaf"), ["c"]);
    assert!(documents(&index, "quillmark").is_empty());
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c2".to_owned()))
    );
}

#[test]
fn a_failed_store_commit_on_a_refeed_from_the_start_pulls_from_the_start_again() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let first = batch(
        vec![
            entry("a", 1, "Synthetic alpha"),
            document("a", 1, "quillmark"),
        ],
        "c1",
        true,
    );
    let second = batch(
        vec![
            entry("b", 1, "Synthetic bravo"),
            document("b", 1, "lanternfish"),
        ],
        "c2",
        false,
    );
    let mut plugin = Scripted::default()
        .answer(None, first)
        .answer(Some("c1"), second)
        .answer(Some("c2"), batch(Vec::new(), "c2", false));
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some("c2".to_owned()))
    );

    // A history repair empties the index; the refeed's first batch reaches the index, then
    // the store commit fails, as a crash between the two would leave it.
    let mut emptied = folder.index("fulltext-emptied");
    let failing = FailingStore {
        store: &store,
        failures: Cell::new(1),
    };
    announce(&mut feeds);
    let failed = settle_with(
        &mut feeds,
        &mut plugin,
        &failing,
        &mut emptied,
        &mut catalogue,
    );
    assert_eq!(failed, Err(FeedError::Store(StoreError::Busy)));

    // The cursor is still the old feed's end, but the index is not marked as fed, so the next
    // pull starts again from the start instead of going on from c2 and losing batch two.
    announce(&mut feeds);
    settle(
        &mut feeds,
        &mut plugin,
        &store,
        &mut emptied,
        &mut catalogue,
    );
    let refed: Vec<Option<String>> = plugin.requests[2..].to_vec();
    assert_eq!(refed, [None, None, Some("c1".to_owned())]);
    assert_eq!(documents(&emptied, "quillmark"), ["a"]);
    assert_eq!(documents(&emptied, "lanternfish"), ["b"]);
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
}

#[test]
fn an_emptied_index_is_fed_again_from_the_start_dropping_old_rows_first() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let refed = batch(
        vec![
            entry("b", 1, "Synthetic bravo"),
            document("b", 1, "lanternfish"),
        ],
        "r1",
        false,
    );
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);

    // A history repair leaves a fresh index folder: the cursor no longer describes it.
    let mut emptied = folder.index("fulltext-emptied");
    plugin = plugin.answer(None, refed);
    announce(&mut feeds);
    settle(
        &mut feeds,
        &mut plugin,
        &store,
        &mut emptied,
        &mut catalogue,
    );
    assert_eq!(plugin.requests, [None, None]);
    assert_eq!(titles(&mut catalogue, "synthetic"), ["Synthetic bravo"]);
    assert_eq!(documents(&emptied, "lanternfish"), ["b"]);
    assert_eq!(kept_rows(&store), [("b".to_owned(), 1)]);

    // The index is marked again, so the next announcement goes on from the new cursor.
    announce(&mut feeds);
    plugin = plugin.answer(Some("r1"), batch(Vec::new(), "r1", false));
    settle(
        &mut feeds,
        &mut plugin,
        &store,
        &mut emptied,
        &mut catalogue,
    );
    assert_eq!(plugin.requests.last(), Some(&Some("r1".to_owned())));
}

#[test]
fn a_reset_in_a_batch_drops_what_came_before_it_in_both_tiers() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let after_reset = batch(
        vec![
            entry("z", 1, "Synthetic zulu, never kept"),
            Change::Reset,
            entry("c", 1, "Synthetic charlie"),
        ],
        "c2",
        false,
    );
    let mut plugin = Scripted::default()
        .answer(None, first_batch())
        .answer(Some("c1"), after_reset);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(titles(&mut catalogue, "synthetic"), ["Synthetic charlie"]);
    assert!(documents(&index, "quillmark").is_empty());
}

#[test]
fn a_batch_either_tier_would_refuse_writes_to_neither() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    // The document is fine; the deletion names the index's reserved marker id, which the
    // index alone would refuse, and only after the document was written.
    let refused = batch(
        vec![
            document("d", 1, "copperleaf"),
            Change::Delete("\0fed".to_owned()),
        ],
        "c1",
        false,
    );
    let mut plugin = Scripted::default().answer(None, refused);
    announce(&mut feeds);
    let failed = settle_with(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        failed,
        Err(FeedError::Index(FullTextError::InvalidDocument))
    );
    assert!(documents(&index, "copperleaf").is_empty());
    assert_eq!(store.plugin_feed_cursor(PLUGIN, FEED), Ok(None));
}

#[test]
fn a_long_feed_pulls_a_bounded_number_of_batches_per_announcement() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let first = batch(vec![entry("p0", 1, "Synthetic page 0")], "c1", true);
    let mut plugin = Scripted::default().answer(None, first);
    for page in 1..=BATCHES_PER_TRIGGER + 2 {
        let changes = vec![entry(
            &format!("p{page}"),
            1,
            &format!("Synthetic page {page}"),
        )];
        let cursor = format!("c{}", page + 1);
        plugin = plugin.answer(Some(&format!("c{page}")), batch(changes, &cursor, true));
    }
    announce(&mut feeds);
    let mut sinks = Sinks {
        store: &store,
        index: Some(&mut index),
        catalogue: &mut catalogue,
        now: NOW,
        at: Instant::now(),
    };
    // One pump starts the pull, then one applies each batch.
    for _ in 0..BATCHES_PER_TRIGGER + 1 {
        feeds.pump(&mut plugin, &mut sinks).expect("a pull");
    }
    assert_eq!(plugin.requests.len(), BATCHES_PER_TRIGGER as usize);
    assert!(feeds.pull.is_none(), "the budget is spent");
    assert_eq!(feeds.waiting.len(), 1, "the feed waits behind any other");
    feeds.pump(&mut plugin, &mut sinks).expect("the next turn");
    let resumed = plugin.requests.last().cloned().flatten();
    assert_eq!(resumed, Some(format!("c{BATCHES_PER_TRIGGER}")));
}

#[test]
fn page_text_expiry_leaves_a_pulled_plugin_document() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    settle(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    let a_year_on = NOW + 365 * 24 * 60 * 60;
    assert_eq!(index.expire(a_year_on), Ok(0));
    assert_eq!(documents(&index, "quillmark"), ["a"]);
}

#[test]
fn a_feed_with_more_that_does_not_move_its_cursor_waits_for_its_next_announcement() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let first = batch(vec![entry("a", 1, "Synthetic alpha")], "c1", true);
    let mut plugin = Scripted::default()
        .answer(None, first)
        .answer(Some("c1"), batch(Vec::new(), "c1", true));
    announce(&mut feeds);
    let stalled = settle_with(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        stalled,
        Err(FeedError::Stalled {
            plugin: PLUGIN.to_owned()
        })
    );
    assert_eq!(plugin.requests, [None, Some("c1".to_owned())]);
    assert!(
        !feeds.busy(true),
        "the feed is not asked again until announced"
    );
    assert_eq!(titles(&mut catalogue, "synthetic"), ["Synthetic alpha"]);
}

#[test]
fn a_failed_fed_marker_still_shows_the_rows_the_store_kept() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default().answer(None, first_batch());
    announce(&mut feeds);
    // The documents' commit succeeds; the marker's, after the store kept the rows, fails.
    let mut failing = FailingIndex {
        index: &mut index,
        passes: 1,
        failures: 1,
        remove_failures: 0,
    };
    let failed = settle_with(
        &mut feeds,
        &mut plugin,
        &store,
        &mut failing,
        &mut catalogue,
    );
    assert_eq!(failed, Err(FeedError::Index(FullTextError::Io)));
    assert_eq!(
        kept_rows(&store),
        [("a".to_owned(), 1), ("b".to_owned(), 1)]
    );
    assert_eq!(
        titles(&mut catalogue, "synthetic"),
        ["Synthetic alpha", "Synthetic bravo"]
    );
}

#[test]
fn a_feed_that_goes_round_between_cursors_stalls_and_its_repeat_is_only_logged() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let mut plugin = Scripted::default()
        .answer(
            None,
            batch(vec![entry("a", 1, "Synthetic alpha")], "A", true),
        )
        .answer(Some("A"), batch(Vec::new(), "B", true))
        .answer(Some("B"), batch(Vec::new(), "A", true));
    announce(&mut feeds);
    let stalled = settle_with(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        stalled,
        Err(FeedError::Stalled {
            plugin: PLUGIN.to_owned()
        })
    );
    assert_eq!(
        plugin.requests,
        [None, Some("A".to_owned()), Some("B".to_owned())]
    );

    // The next announcement goes round again: stopped, but the owner is not told twice.
    announce(&mut feeds);
    let repeated = settle_with(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(repeated, Ok(()));
    assert!(!feeds.busy(true));
}

#[test]
fn a_feed_that_returns_to_a_cursor_of_its_previous_announcement_stalls() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    let page = |cursor: usize| batch(Vec::new(), &format!("c{cursor}"), true);
    let mut plugin = Scripted::default().answer(None, page(1));
    let budget = BATCHES_PER_TRIGGER as usize;
    for cursor in 1..budget {
        plugin = plugin.answer(Some(&format!("c{cursor}")), page(cursor + 1));
    }
    // After spending its budget, the feed goes back to its second cursor.
    plugin = plugin.answer(Some(&format!("c{budget}")), page(2));
    announce(&mut feeds);
    // One turn spends the budget, the next (the feed waits behind no other) goes round.
    let stalled = settle_with(&mut feeds, &mut plugin, &store, &mut index, &mut catalogue);
    assert_eq!(
        stalled,
        Err(FeedError::Stalled {
            plugin: PLUGIN.to_owned()
        })
    );
    assert_eq!(plugin.requests.len(), budget + 1);
}

#[test]
fn a_feed_pulled_again_from_its_start_part_way_through_is_not_taken_for_going_round() {
    let folder = Folder::new();
    let store = folder.store();
    let mut index = folder.index("fulltext");
    let mut catalogue = Catalogue::new();
    let mut feeds = PluginFeeds::default();
    // Offset-style cursors: a feed pulled again from its start sends the same ones again.
    let last = BATCHES_PER_TRIGGER as usize + 3;
    let page = |offset: usize| {
        let changes = vec![entry(
            &format!("p{offset}"),
            1,
            &format!("Synthetic page {offset}"),
        )];
        batch(changes, &format!("{}", (offset + 1) * 256), offset < last)
    };
    let mut plugin = Scripted::default().answer(None, page(0));
    for offset in 1..=last {
        plugin = plugin.answer(Some(&format!("{}", offset * 256)), page(offset));
    }
    announce(&mut feeds);
    let mut sinks = Sinks {
        store: &store,
        index: Some(&mut index),
        catalogue: &mut catalogue,
        now: NOW,
        at: Instant::now(),
    };
    // One pump starts the pull, then one applies each batch, until the budget is spent.
    for _ in 0..=BATCHES_PER_TRIGGER {
        feeds.pump(&mut plugin, &mut sinks).expect("a pull");
    }
    assert_eq!(
        feeds.waiting.len(),
        1,
        "the feed waits with its cursors carried"
    );

    // A history repair empties the index part-way: the feed is pulled again from its start,
    // sending cursors its previous announcement carried, and completes without a stall.
    let mut emptied = folder.index("fulltext-emptied");
    settle(
        &mut feeds,
        &mut plugin,
        &store,
        &mut emptied,
        &mut catalogue,
    );
    assert_eq!(titles(&mut catalogue, "synthetic").len(), last + 1);
    assert_eq!(
        store.plugin_feed_cursor(PLUGIN, FEED),
        Ok(Some(format!("{}", (last + 1) * 256)))
    );
}
