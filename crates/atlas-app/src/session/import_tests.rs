//! Times ordinary worker changes after a synthetic 50,000-entry bookmark import (P1-22).

use atlas_core::{
    BookmarkImport, FolderRole, ImportSummary, ImportedBookmark, ImportedFolder, ImportedItem,
};

use super::*;

/// Far above the measured cost, far below the full rewrite it replaced.
const REGRESSION_BOUND: Duration = Duration::from_millis(250);

/// 50 folders of 999 pages each: the parser's 50,000-entry bound.
pub(super) fn large_import() -> BookmarkImport {
    let items = (0..50)
        .map(|folder| {
            ImportedItem::Folder(ImportedFolder {
                title: format!("Synthetic folder {folder}"),
                added: None,
                role: FolderRole::Ordinary,
                items: (0..999)
                    .map(|page| {
                        ImportedItem::Bookmark(ImportedBookmark {
                            title: format!("Synthetic page {folder}-{page}"),
                            address: format!("https://f{folder}.example.test/{page}"),
                            added: None,
                        })
                    })
                    .collect(),
            })
        })
        .collect();
    BookmarkImport {
        items,
        summary: ImportSummary::default(),
    }
}

/// Median and worst of the durations.
fn spread(mut samples: Vec<Duration>) -> (Duration, Duration) {
    samples.sort();
    (samples[samples.len() / 2], samples[samples.len() - 1])
}

#[test]
fn ordinary_changes_after_a_large_import_stay_fast() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: LIVE_ADDRESS.to_owned(),
        },
    );
    let tab = opened.model.tabs[0].id;
    let started = Instant::now();
    send(
        &handle,
        SessionCommand::ImportBookmarks {
            request_id: 1,
            import: Box::new(large_import()),
            folder_title: "Synthetic".to_owned(),
        },
    );
    match next_event(&mut events) {
        SessionEvent::Imported {
            request_id: 1,
            result: Ok(commit),
        } => assert_eq!(commit.tree.pages, 49_950),
        event => panic!("expected the import result, got {event:?}"),
    }
    assert_eq!(state(&mut events).model.saved_layout.len(), 50_001);
    let import_time = started.elapsed();

    // Title and address updates take the worker's save and catalogue path, as page loads do.
    let mut titles = Vec::new();
    let mut navigations = Vec::new();
    for round in 0..20 {
        let started = Instant::now();
        change(
            &handle,
            &mut events,
            SessionCommand::ObserveMetadata {
                id: tab,
                title: Some(format!("Synthetic title {round}")),
                address: None,
            },
        );
        titles.push(started.elapsed());
        let started = Instant::now();
        change(
            &handle,
            &mut events,
            SessionCommand::ObserveMetadata {
                id: tab,
                title: None,
                address: Some(format!("https://live-session.example.test/{round}")),
            },
        );
        navigations.push(started.elapsed());
    }
    let (title_median, title_worst) = spread(titles);
    let (navigation_median, navigation_worst) = spread(navigations);
    eprintln!(
        "50,000-entry import: {import_time:?}; then title update median {title_median:?} \
         (worst {title_worst:?}), navigation median {navigation_median:?} (worst {navigation_worst:?})"
    );
    // Measured about 43 ms on the owner's machine, against about 1 s before the saved rows were
    // kept; the bound leaves room for a busy machine while still catching a full rewrite.
    for median in [title_median, navigation_median] {
        assert!(median < REGRESSION_BOUND, "median {median:?}");
    }
    stop(handle, &mut events);

    // After a restart the first change compares with the loaded revision, not a full rewrite.
    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model.saved_layout.len(), 50_001);
    let started = Instant::now();
    change(
        &handle,
        &mut events,
        SessionCommand::ObserveMetadata {
            id: tab,
            title: Some("Synthetic title after restart".to_owned()),
            address: None,
        },
    );
    let first_after_restart = started.elapsed();
    eprintln!("first title update after restart: {first_after_restart:?}");
    assert!(first_after_restart < REGRESSION_BOUND);
    stop(handle, &mut events);
    fixture.cleanup();
}
