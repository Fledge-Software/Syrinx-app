//! Checks that plugin feed text (#141) stays apart from page text, leaves with its plugin
//! and leaves a marker an emptied index loses.

use super::super::*;
use super::fixture::{DAY, Fixture, NOW, delete, document, page_document, upsert};

fn plugin(plugin: &str, feed: &str) -> Namespace {
    Namespace::plugin_feed(plugin, feed).expect("valid synthetic plugin feed")
}

/// Commits one document whose body holds `word` into `namespace`.
fn feed(index: &mut FullText, namespace: Namespace, id: &str, word: &str) {
    index
        .stage(&[upsert(document(namespace, id, 1, 0, word))], NOW)
        .expect("stage synthetic plugin document");
    index.commit().expect("commit synthetic plugin document");
}

#[test]
fn each_plugin_and_feed_pair_has_its_own_namespace() {
    assert_ne!(plugin("a.b", "c"), plugin("a", "b.c"));
    assert_ne!(
        plugin("a", "b").stored(),
        Namespace::feed("a").expect("feed").stored()
    );
    for (id, name) in [
        ("", "items"),
        ("notes", ""),
        ("Notes", "items"),
        ("notes", "a:b"),
    ] {
        assert_eq!(
            Namespace::plugin_feed(id, name),
            Err(FullTextError::InvalidNamespace)
        );
    }
    let namespace = plugin("notes.local", "notes.titles-2");
    assert_eq!(Namespace::from_stored(&namespace.stored()), Ok(namespace));
    assert_eq!(plugin("notes", "items").retention(), Retention::Kept);
}

#[test]
fn page_text_maintenance_never_touches_a_plugin_document() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    // Dated at the epoch: as old as a document can look.
    feed(&mut index, plugin("notes", "items"), "n1", "quillmark");
    index
        .stage(&[upsert(page_document("n1", 1, NOW, "quillmark"))], NOW)
        .expect("stage the page with the same id");
    index.commit().expect("commit the page");

    // D40's expiry takes the page, never the plugin's document.
    let far_future = NOW + 400 * DAY;
    assert_eq!(index.expire(far_future), Ok(1));
    let clear = ClearRequest {
        generation: 1,
        scope: ClearScope::Pages {
            start: i64::MIN,
            end: i64::MAX,
        },
    };
    assert_eq!(index.apply_clears(&[clear]), Ok(1));
    index.restart_clears().expect("restart the clears");
    assert_eq!(index.remove_pages(|_| true), Ok(0));
    assert_eq!(fixture.ids(&index, "quillmark"), ["n1"]);
    let held = index.held_revision(&plugin("notes", "items"), "n1");
    assert_eq!(held, Ok(Some(1)));
    assert_eq!(index.held_revision(&Namespace::page(), "n1"), Ok(None));
}

#[test]
fn removing_a_plugin_takes_every_feed_of_it_and_nothing_else() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    feed(&mut index, plugin("a", "items"), "1", "lanternfish");
    feed(&mut index, plugin("a", "notes.text"), "2", "lanternfish");
    feed(&mut index, plugin("a.b", "items"), "3", "lanternfish");
    feed(&mut index, plugin("ab", "items"), "4", "lanternfish");
    feed(
        &mut index,
        Namespace::feed("a").expect("feed"),
        "5",
        "lanternfish",
    );
    index
        .stage(&[upsert(page_document("6", 1, NOW, "lanternfish"))], NOW)
        .expect("stage a page");
    index.commit().expect("commit a page");
    index
        .mark_fed(&plugin("a", "items"))
        .expect("mark the feed");
    index.commit().expect("commit the marker");

    index.remove_plugin("a").expect("remove the plugin");
    let mut left = fixture.ids(&index, "lanternfish");
    left.sort();
    assert_eq!(left, ["3", "4", "5", "6"]);
    assert_eq!(index.is_fed(&plugin("a", "items")), Ok(false));
    assert!(
        fixture
            .audit(&index)
            .iter()
            .all(|row| !row.key.starts_with("plugin:a:")),
        "no document, tombstone or marker of the plugin is left on disk"
    );
}

#[test]
fn a_deletion_at_the_held_revision_removes_a_plugin_document() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let namespace = plugin("notes", "items");
    feed(&mut index, namespace.clone(), "n1", "copperleaf");
    let held = index
        .held_revision(&namespace, "n1")
        .expect("read the revision")
        .expect("held");
    index
        .stage(&[delete(namespace.clone(), "n1", held)], NOW)
        .expect("stage the deletion");
    assert_eq!(index.held_revision(&namespace, "n1"), Ok(Some(held)));
    index.commit().expect("commit the deletion");
    assert!(fixture.ids(&index, "copperleaf").is_empty());
}

#[test]
fn the_fed_marker_lasts_until_the_feed_or_the_index_is_emptied() {
    let fixture = Fixture::new();
    let namespace = plugin("notes", "items");
    {
        let mut index = fixture.open();
        assert_eq!(index.is_fed(&namespace), Ok(false));
        index.mark_fed(&namespace).expect("mark the feed");
        assert_eq!(index.is_fed(&namespace), Ok(true), "counted once staged");
        index.commit().expect("commit the marker");
        // Marking again is a replay and stages nothing.
        index.mark_fed(&namespace).expect("mark again");
        index.commit().expect("commit nothing new");
        assert_eq!(index.expire(NOW + 400 * DAY), Ok(0));
    }
    let mut index = fixture.open();
    assert_eq!(index.is_fed(&namespace), Ok(true), "kept across a reopen");
    assert!(index.search(&request("fed")).expect("search").is_empty());
    index.reset(&namespace).expect("reset the feed");
    assert_eq!(index.is_fed(&namespace), Ok(false));

    // A freshly created folder, as a history repair leaves it, holds no marker.
    let emptied = Fixture::new();
    assert_eq!(emptied.open().is_fed(&namespace), Ok(false));
}

#[test]
fn no_caller_may_write_the_reserved_marker_id_or_mark_page_text() {
    let fixture = Fixture::new();
    let mut index = fixture.open();
    let namespace = plugin("notes", "items");
    let forged = [delete(namespace.clone(), "\0fed", 1)];
    assert_eq!(
        index.stage(&forged, NOW),
        Err(FullTextError::InvalidDocument)
    );
    let forged = [upsert(document(namespace, "\0other", 1, 0, "word"))];
    assert_eq!(
        index.stage(&forged, NOW),
        Err(FullTextError::InvalidDocument)
    );
    assert_eq!(
        index.mark_fed(&Namespace::page()),
        Err(FullTextError::InvalidNamespace)
    );
}

fn request(text: &str) -> SearchRequest {
    SearchRequest {
        text: text.to_owned(),
        prefix_last: true,
        namespaces: NamespaceFilter::All,
        limit: 10,
    }
}
