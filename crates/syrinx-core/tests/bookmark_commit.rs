//! Proves a confirmed bookmark import becomes unloaded saved rows in one new folder (P1-22).

use std::collections::HashMap;

use syrinx_core::{
    Activation, BookmarkImport, DEFAULT_IMPORT_FOLDER, FolderRole, ImportSummary, ImportedBookmark,
    ImportedFolder, ImportedItem, SavedEntryId, SavedFolderId, SkipReason, TabError, TabKind,
    TabMetadata, TabModel, TabModelSnapshot, commit_bookmark_import, import_folder_title,
    parse_bookmark_html,
};

const CHROME: &str = include_str!("fixtures/bookmarks/chrome.html");
const FIREFOX: &str = include_str!("fixtures/bookmarks/firefox.html");

fn parse(fixture: &str) -> BookmarkImport {
    match parse_bookmark_html(fixture.replace("\r\n", "\n").as_bytes()) {
        Ok(import) => import,
        Err(error) => panic!("synthetic export failed to parse: {error}"),
    }
}

fn synthetic(number: u64) -> TabMetadata {
    TabMetadata {
        title: format!("Synthetic existing {number}"),
        address: format!("https://existing-{number}.example.test/"),
        favicon: None,
        badge: None,
    }
}

/// Lists the saved tree depth-first as `depth:kind:title[=address]`.
fn saved_outline(model: &TabModel) -> Vec<String> {
    let mut depths: HashMap<SavedFolderId, usize> = HashMap::new();
    let mut lines = Vec::new();
    for placement in model.saved_layout() {
        let depth = placement.parent.map_or(0, |parent| {
            depths.get(&parent).copied().unwrap_or_default() + 1
        });
        match placement.entry {
            SavedEntryId::Folder(id) => {
                depths.insert(id, depth);
                let folder = model.get_folder(id).expect("layout folder exists");
                lines.push(format!("{depth}:folder:{}", folder.title()));
            }
            SavedEntryId::Page(id) => {
                let page = model.get_saved(id).expect("layout page exists");
                let metadata = page.metadata();
                lines.push(format!(
                    "{depth}:page:{}={}",
                    metadata.title, metadata.address
                ));
            }
        }
    }
    lines
}

fn bookmark(title: &str, address: &str) -> ImportedItem {
    ImportedItem::Bookmark(ImportedBookmark {
        title: title.to_owned(),
        address: address.to_owned(),
        added: None,
    })
}

fn folder(title: &str, items: Vec<ImportedItem>) -> ImportedItem {
    ImportedItem::Folder(ImportedFolder {
        title: title.to_owned(),
        added: None,
        role: FolderRole::Ordinary,
        items,
    })
}

fn hand_made(items: Vec<ImportedItem>) -> BookmarkImport {
    BookmarkImport {
        items,
        summary: ImportSummary::default(),
    }
}

#[test]
fn chrome_export_becomes_one_unloaded_folder_in_export_order() {
    let mut model = TabModel::new();
    let commit =
        commit_bookmark_import(&mut model, &parse(CHROME), DEFAULT_IMPORT_FOLDER.to_owned())
            .expect("import commits");
    assert_eq!(
        saved_outline(&model),
        [
            "0:folder:Imported bookmarks",
            // The toolbar folder is an ordinary folder, not Favorites (2026-10-12).
            "1:folder:Bookmarks bar",
            "2:page:Alpha Synthetic=https://alpha.example.test/",
            "2:page:Beta & Gamma <Test>=https://beta.example.test/search?q=one&page=2",
            "2:folder:Reading 'list'",
            "3:page:Gamma article=https://gamma.example.test/article",
            "3:folder:Deeper",
            "4:page:Delta café=https://delta.example.test/",
            "1:folder:Other bookmarks",
            "2:page:Alpha again=https://alpha.example.test/",
            "2:folder:Empty folder",
        ]
    );
    assert_eq!(commit.tree.folders, 5);
    assert_eq!(commit.tree.pages, 5);
    assert!(commit.skipped.is_empty());
    // Nothing opened: imported pages are unloaded rows.
    assert!(model.tabs().is_empty());
    assert_eq!(model.active_id(), None);
    let snapshot = model.snapshot();
    snapshot
        .validate()
        .expect("imported tree is a valid snapshot");
    let restored = TabModel::from_snapshot(snapshot.clone()).expect("snapshot restores");
    assert_eq!(restored.snapshot(), snapshot);
}

#[test]
fn import_appends_after_existing_rows_and_leaves_open_tabs_alone() {
    let mut model = TabModel::new();
    let open = model
        .open(TabKind::Web, synthetic(1), Activation::Foreground)
        .expect("synthetic tab opens");
    model.save_page(open).expect("synthetic page saves");
    let ordinary = model
        .open(TabKind::Web, synthetic(2), Activation::Foreground)
        .expect("ordinary tab opens");
    let before = model.snapshot();

    let commit = commit_bookmark_import(
        &mut model,
        &parse(FIREFOX),
        "bookmarks-synthetic".to_owned(),
    )
    .expect("import commits");

    let after = model.snapshot();
    assert_eq!(after.tabs, before.tabs);
    assert_eq!(after.active_id, Some(ordinary));
    assert_eq!(after.saved_layout[0], before.saved_layout[0]);
    assert_eq!(
        after.saved_layout[1].entry,
        SavedEntryId::Folder(commit.tree.folder)
    );
    assert_eq!(after.saved_layout[1].parent, None);
    let outline = saved_outline(&model);
    assert_eq!(outline[1], "0:folder:bookmarks-synthetic");
    assert!(outline[2..].iter().all(|line| !line.starts_with("0:")));
}

#[test]
fn final_address_check_reports_refusals_and_writes_none_of_them() {
    let import = hand_made(vec![
        bookmark("Kept", "https://kept.example.test/"),
        folder(
            "Inner",
            vec![
                bookmark("Signed-in synthetic", "https://user:secret@example.test/"),
                bookmark("Script", "javascript:void(0)"),
                bookmark("Broken host", "https://a b.example.test/"),
                bookmark("Also kept", "HTTPS://Also.Example.TEST/Path"),
            ],
        ),
    ]);
    let mut model = TabModel::new();
    let commit = commit_bookmark_import(&mut model, &import, "Synthetic".to_owned())
        .expect("import commits");
    assert_eq!(
        commit
            .skipped
            .iter()
            .map(|rejected| (rejected.title.as_str(), rejected.reason.clone()))
            .collect::<Vec<_>>(),
        [
            ("Signed-in synthetic", SkipReason::CredentialsInAddress),
            ("Script", SkipReason::InvalidAddress),
            ("Broken host", SkipReason::InvalidAddress),
        ]
    );
    assert_eq!(
        saved_outline(&model),
        [
            "0:folder:Synthetic",
            "1:page:Kept=https://kept.example.test/",
            "1:folder:Inner",
            "2:page:Also kept=https://also.example.test/Path",
        ]
    );
    assert_eq!(commit.tree.pages, 2);
    assert!(!format!("{commit:?}").contains("Signed-in synthetic"));
}

#[test]
fn unnamed_folders_and_untitled_pages_get_visible_names() {
    let import = hand_made(vec![folder(
        "  ",
        vec![bookmark("", "https://untitled.example.test/")],
    )]);
    let mut model = TabModel::new();
    commit_bookmark_import(&mut model, &import, "Synthetic".to_owned()).expect("import commits");
    assert_eq!(
        saved_outline(&model),
        [
            "0:folder:Synthetic",
            "1:folder:Untitled folder",
            "2:page:https://untitled.example.test/=https://untitled.example.test/",
        ]
    );
}

#[test]
fn exhausted_identities_leave_the_model_unchanged() {
    for (snapshot, expected) in [
        (
            TabModelSnapshot {
                last_saved_page_id: u64::MAX - 1,
                ..TabModelSnapshot::default()
            },
            TabError::SavedPageIdExhausted,
        ),
        (
            TabModelSnapshot {
                last_saved_folder_id: u64::MAX - 1,
                ..TabModelSnapshot::default()
            },
            TabError::SavedFolderIdExhausted,
        ),
    ] {
        let mut model = TabModel::from_snapshot(snapshot.clone()).expect("synthetic model");
        let error = commit_bookmark_import(&mut model, &parse(CHROME), "Synthetic".to_owned())
            .expect_err("identities run out");
        assert_eq!(error, expected);
        assert_eq!(model.snapshot(), snapshot);
    }
}

#[test]
fn folder_titles_come_from_the_file_name_or_the_default() {
    for (stem, expected) in [
        (None, DEFAULT_IMPORT_FOLDER),
        (Some(""), DEFAULT_IMPORT_FOLDER),
        (Some(" \t"), DEFAULT_IMPORT_FOLDER),
        (Some("bookmarks_10_12_26"), "bookmarks_10_12_26"),
        (Some(" Synthetic\u{202E} export\u{0} "), "Synthetic export"),
    ] {
        assert_eq!(import_folder_title(stem), expected, "{stem:?}");
    }
    assert_eq!(
        import_folder_title(Some(&"x".repeat(5_000)))
            .chars()
            .count(),
        1_024
    );
}

#[test]
fn a_large_deep_import_commits_in_one_step() {
    // 40 folders nested 20 deep in two chains, 1,000 pages each: the parser's scale, quickly.
    let chain = |name: &str| {
        let mut items: Vec<ImportedItem> = Vec::new();
        for depth in (0..20).rev() {
            let mut level: Vec<ImportedItem> = (0..1_000)
                .map(|page| {
                    bookmark(
                        &format!("{name} {depth} {page}"),
                        &format!("https://{name}-{depth}.example.test/{page}"),
                    )
                })
                .collect();
            level.append(&mut items);
            items = vec![folder(&format!("{name} {depth}"), level)];
        }
        items
    };
    let mut items = chain("left");
    items.extend(chain("right"));
    let mut model = TabModel::new();
    let commit = commit_bookmark_import(&mut model, &hand_made(items), "Synthetic".to_owned())
        .expect("large import commits");
    assert_eq!(commit.tree.folders, 40);
    assert_eq!(commit.tree.pages, 40_000);
    let snapshot = model.snapshot();
    snapshot.validate().expect("large tree is a valid snapshot");
    assert_eq!(snapshot.saved_layout.len(), 40_041);
}
