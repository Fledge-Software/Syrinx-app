//! Proves the sidebar's folder edits and folder expansion survive a real worker restart (P1-21).

use syrinx_core::{FolderContents, SavedEntryId, SavedFolderId, TabError};

use super::*;

fn folder_edit(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    command: FolderCommand,
) -> SessionSnapshot {
    change(handle, events, SessionCommand::Folder(command))
}

fn only_folder(snapshot: &SessionSnapshot) -> SavedFolderId {
    assert_eq!(snapshot.model.saved_folders.len(), 1);
    snapshot.model.saved_folders[0].id
}

fn stored_settings(events: &mut UnboundedReceiver<SessionEvent>) -> BTreeMap<String, String> {
    // The usage status (AN-01) is reported first at start-up and has its own tests.
    loop {
        match next_any_event(events) {
            SessionEvent::Preferences { settings, .. } => return settings,
            SessionEvent::Usage { .. } => continue,
            event => panic!("expected synthetic preferences, got {event:?}"),
        }
    }
}

#[test]
fn folder_edits_and_expansion_survive_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.to_owned(),
        },
    );
    let saved = change(&handle, &mut events, SessionCommand::SaveActive);
    let page = SavedEntryId::Page(saved.model.saved_pages[0].id);
    let live = saved.model.tabs[0].id;

    let created = folder_edit(
        &handle,
        &mut events,
        FolderCommand::Create {
            title: "Synthetic work".to_owned(),
            parent: None,
        },
    );
    let folder = only_folder(&created);
    folder_edit(
        &handle,
        &mut events,
        FolderCommand::Move {
            entry: page,
            parent: Some(folder),
            index: 0,
        },
    );
    let renamed = folder_edit(
        &handle,
        &mut events,
        FolderCommand::Rename {
            id: folder,
            title: "Synthetic renamed".to_owned(),
        },
    );

    // An unnamed folder is refused as a recoverable error and changes nothing.
    send(
        &handle,
        SessionCommand::Folder(FolderCommand::Rename {
            id: folder,
            title: "  ".to_owned(),
        }),
    );
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::Model(TabError::EmptyFolderTitle),
            fatal: false
        }
    ));
    send(
        &handle,
        SessionCommand::SaveSettings(vec![(
            "sidebar.expanded_folders".to_owned(),
            folder.get().to_string(),
        )]),
    );
    stop(handle, &mut events);

    // The tree, the open page's association and the stored expansion all come back.
    let (handle, mut events) = fixture.start();
    let settings = stored_settings(&mut events);
    assert_eq!(
        settings.get("sidebar.expanded_folders"),
        Some(&folder.get().to_string())
    );
    let restored = state(&mut events);
    assert_eq!(restored.model, renamed.model);
    assert_eq!(restored.model.saved_folders[0].title, "Synthetic renamed");
    assert_eq!(restored.model.saved_layout[1].parent, Some(folder));
    assert_eq!(restored.model.tabs[0].id, live);

    // The default removal keeps the page, lifted to the top level.
    let lifted = folder_edit(&handle, &mut events, FolderCommand::Remove { id: folder });
    assert!(lifted.model.saved_folders.is_empty());
    assert_eq!(lifted.model.saved_layout.len(), 1);
    assert_eq!(lifted.model.saved_layout[0].parent, None);
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model, lifted.model);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn deleting_a_folder_with_contents_keeps_its_open_page_as_an_ordinary_tab() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: SAVED_ADDRESS.to_owned(),
        },
    );
    let saved = change(&handle, &mut events, SessionCommand::SaveActive);
    let page = SavedEntryId::Page(saved.model.saved_pages[0].id);
    let live = saved.model.tabs[0].id;
    let created = folder_edit(
        &handle,
        &mut events,
        FolderCommand::Create {
            title: "Synthetic doomed".to_owned(),
            parent: None,
        },
    );
    let folder = only_folder(&created);
    folder_edit(
        &handle,
        &mut events,
        FolderCommand::Move {
            entry: page,
            parent: Some(folder),
            index: 0,
        },
    );
    // A confirmation made before the folder changed is refused, recoverably and without loss.
    send(
        &handle,
        SessionCommand::Folder(FolderCommand::DeleteWithContents {
            id: folder,
            expected: FolderContents::default(),
        }),
    );
    assert!(matches!(
        next_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::Model(TabError::FolderContentsChanged(_)),
            fatal: false
        }
    ));
    let deleted = folder_edit(
        &handle,
        &mut events,
        FolderCommand::DeleteWithContents {
            id: folder,
            expected: [page].into_iter().collect(),
        },
    );
    assert!(deleted.model.saved_pages.is_empty());
    assert!(deleted.model.saved_layout.is_empty());
    assert_eq!(deleted.model.tabs[0].id, live);
    assert_eq!(deleted.model.tabs[0].saved_page_id, None);
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.model, deleted.model);
    stop(handle, &mut events);
    fixture.cleanup();
}
