//! Proves the shared browser protocol without a web view: navigation and zoom rules, command
//! ordering and acknowledgement, bounds and diagnostics.

use std::{collections::HashMap, sync::mpsc};

use futures::channel::mpsc::unbounded;

use super::*;

#[test]
fn page_navigation_refuses_non_web_schemes_and_embedded_nul() {
    for url in [
        "file:///synthetic.txt",
        "javascript:1",
        "data:text/plain,synthetic",
        "about:settings",
        "https://safe.example.test/\0https://other.example.test/",
        "example.test",
        "",
    ] {
        assert_eq!(validate_url(url), Err(BrowserError::InvalidUrl));
    }
    for url in [
        "http://synthetic.example.test/",
        "https://synthetic.example.test/caf%C3%A9",
        "about:blank",
    ] {
        assert_eq!(validate_url(url), Ok(()));
    }
}

#[test]
fn zoom_steps_boundaries_and_reset_are_predictable() {
    assert_eq!(next_zoom(1., &PageAction::ZoomIn), 1.1);
    assert_eq!(next_zoom(1.1, &PageAction::ZoomOut), 1.);
    assert_eq!(next_zoom(1.13, &PageAction::ZoomIn), 1.25);
    assert_eq!(next_zoom(1.13, &PageAction::ZoomOut), 1.1);
    assert_eq!(next_zoom(5., &PageAction::ZoomIn), 5.);
    assert_eq!(next_zoom(0.25, &PageAction::ZoomOut), 0.25);
    assert_eq!(next_zoom(3., &PageAction::ZoomReset), 1.);
    assert_eq!(next_zoom(1., &PageAction::SetZoom(1.25)), 1.25);
    assert_eq!(next_zoom(1., &PageAction::SetZoom(9.)), 5.);
    assert_eq!(next_zoom(1.5, &PageAction::SetZoom(f64::NAN)), 1.);
}

#[test]
fn creation_acknowledgment_follows_apply_and_navigation_stays_in_order() {
    let (commands, incoming) = mpsc::channel();
    let (events, mut received) = unbounded();
    for command in [
        BrowserCommand::Create {
            id: 7,
            url: "https://first.example.test/".into(),
        },
        BrowserCommand::Navigate {
            id: 7,
            url: "https://second.example.test/".into(),
        },
        BrowserCommand::Shutdown,
        BrowserCommand::Reload { id: 7 },
    ] {
        commands.send(command).expect("queue synthetic command");
    }

    let mut applied = Vec::new();
    assert!(!drain_commands(
        &incoming,
        &EventSink(events, None),
        |command| {
            match command {
                BrowserCommand::Create { id: 7, url } => {
                    assert!(received.try_recv().is_err(), "no premature acknowledgment");
                    assert_eq!(url, "https://first.example.test/");
                    applied.push("create");
                }
                BrowserCommand::Navigate { id: 7, url } => {
                    assert!(matches!(
                        received.try_recv(),
                        Ok(BrowserEvent::Created { id: 7 })
                    ));
                    assert_eq!(url, "https://second.example.test/");
                    applied.push("navigate");
                }
                _ => panic!("shutdown must stop command application"),
            }
            Ok(())
        }
    ));
    assert_eq!(applied, ["create", "navigate"]);
    assert!(matches!(
        incoming.try_recv(),
        Ok(BrowserCommand::Reload { id: 7 })
    ));
    assert!(received.try_recv().is_err());
}

#[test]
fn a_reopened_tab_is_acknowledged_as_created_like_any_other() {
    let (commands, incoming) = mpsc::channel();
    let (events, mut received) = unbounded();
    commands
        .send(BrowserCommand::Reopen {
            id: 9,
            url: "https://reopened.example.test/".into(),
        })
        .expect("queue synthetic reopening");
    assert!(drain_commands(
        &incoming,
        &EventSink(events, None),
        |command| match command {
            BrowserCommand::Reopen { id: 9, .. } => Ok(()),
            _ => panic!("unexpected synthetic command"),
        }
    ));
    assert!(matches!(
        received.try_recv(),
        Ok(BrowserEvent::Created { id: 9 })
    ));
}

#[test]
fn failed_creation_is_not_acknowledged_and_later_commands_still_run() {
    let (commands, incoming) = mpsc::channel();
    let (events, mut received) = unbounded();
    commands
        .send(BrowserCommand::Create {
            id: 8,
            url: "https://failed.example.test/".into(),
        })
        .expect("queue synthetic creation");
    commands
        .send(BrowserCommand::Navigate {
            id: 8,
            url: "https://retry.example.test/".into(),
        })
        .expect("queue synthetic navigation");
    let mut applied = 0;
    assert!(drain_commands(
        &incoming,
        &EventSink(events, None),
        |command| {
            applied += 1;
            match command {
                BrowserCommand::Create { .. } => Err(BrowserError::CreatePage),
                BrowserCommand::Navigate { .. } => Err(BrowserError::UnknownTab),
                _ => panic!("unexpected synthetic command"),
            }
        }
    ));
    assert_eq!(applied, 2);
    assert!(matches!(
        received.try_recv(),
        Ok(BrowserEvent::CreationFailed {
            id: 8,
            error: BrowserError::CreatePage,
        })
    ));
    assert!(matches!(
        received.try_recv(),
        Ok(BrowserEvent::Error {
            tab: Some(8),
            error: BrowserError::UnknownTab,
        })
    ));
    assert!(received.try_recv().is_err());
}

#[test]
fn popup_acknowledgment_follows_native_adoption_and_reports_failure() {
    for result in [Ok(()), Err(BrowserError::Popup)] {
        let (commands, incoming) = mpsc::channel();
        let (events, mut received) = unbounded();
        commands
            .send(BrowserCommand::AdoptPopup { request: 41, id: 8 })
            .expect("queue synthetic popup adoption");
        assert!(drain_commands(
            &incoming,
            &EventSink(events, None),
            |command| {
                assert!(matches!(
                    command,
                    BrowserCommand::AdoptPopup { request: 41, id: 8 }
                ));
                assert!(received.try_recv().is_err(), "no premature acknowledgment");
                result
            }
        ));
        match result {
            Ok(()) => assert!(matches!(
                received.try_recv(),
                Ok(BrowserEvent::Created { id: 8 })
            )),
            Err(_) => assert!(matches!(
                received.try_recv(),
                Ok(BrowserEvent::CreationFailed {
                    id: 8,
                    error: BrowserError::Popup,
                })
            )),
        }
        assert!(received.try_recv().is_err());
    }
}

#[test]
fn disconnect_drains_existing_commands_then_ends_the_host_loop() {
    let (commands, incoming) = mpsc::channel();
    let (events, mut received) = unbounded();
    commands
        .send(BrowserCommand::Close { id: 9 })
        .expect("queue synthetic close");
    drop(commands);
    let mut closed = false;
    assert!(!drain_commands(
        &incoming,
        &EventSink(events, None),
        |command| {
            assert!(matches!(command, BrowserCommand::Close { id: 9 }));
            closed = true;
            Ok(())
        }
    ));
    assert!(closed);
    assert!(received.try_recv().is_err());
}

#[test]
fn set_muted_routes_to_its_own_tab() {
    for (id, muted) in [(7, true), (8, false)] {
        assert_eq!(BrowserCommand::SetMuted { id, muted }.tab_id(), Some(id));
    }
}

#[test]
fn physical_bounds_reject_coordinate_overflow_without_clipping() {
    for (x, y, width, height) in [
        (0, 0, 2_147_483_648, 1),
        (0, 0, 1, u32::MAX),
        (i32::MAX, 0, 1, 1),
        (0, i32::MAX, 1, 1),
    ] {
        assert_eq!(
            PageBounds {
                x,
                y,
                width,
                height
            }
            .validate(),
            Err(BrowserError::InvalidBounds)
        );
    }
    for bounds in [
        PageBounds::default(),
        PageBounds {
            x: i32::MIN,
            y: -1,
            width: 2_147_483_647,
            height: 1,
        },
        PageBounds {
            x: i32::MAX,
            y: i32::MAX,
            width: 0,
            height: 0,
        },
    ] {
        assert_eq!(bounds.validate(), Ok(()));
    }
}

#[test]
fn browser_diagnostics_do_not_include_page_content() {
    let sentinel = "https://private-debug-sentinel.example.test/";
    for debug in [
        format!(
            "{:?}",
            BrowserEvent::PopupRequested {
                id: 1,
                request: 41,
                url: sentinel.into()
            }
        ),
        format!(
            "{:?}",
            BrowserCommand::Create {
                id: 1,
                url: sentinel.into()
            }
        ),
        format!(
            "{:?}",
            BrowserCommand::Navigate {
                id: 1,
                url: sentinel.into()
            }
        ),
        format!(
            "{:?}",
            BrowserEvent::Title {
                id: 1,
                title: sentinel.into()
            }
        ),
        format!(
            "{:?}",
            BrowserEvent::Url {
                id: 1,
                url: sentinel.into()
            }
        ),
    ] {
        assert!(!debug.contains(sentinel));
    }
}

#[test]
fn resume_failure_leaves_previous_page_visible_and_reports_failure() {
    let mut visible = HashMap::from([(7, true), (8, false), (9, false)]);
    let mut selection = Some(7);
    let result = show_active(
        [7, 8, 9].into_iter(),
        Some(8),
        &mut selection,
        |id, show| {
            if id == 8 && show {
                return Err(BrowserError::Suspension);
            }
            visible.insert(id, show);
            Ok(())
        },
    );
    assert_eq!(result, Err(BrowserError::Suspension));
    assert_eq!(
        selection,
        Some(8),
        "recovery must not retain the old active ID"
    );
    assert_eq!(visible, HashMap::from([(7, true), (8, false), (9, false)]));
    show_active(
        [7, 8, 9].into_iter(),
        Some(8),
        &mut selection,
        |id, show| {
            visible.insert(id, show);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(visible, HashMap::from([(7, false), (8, true), (9, false)]));
}
