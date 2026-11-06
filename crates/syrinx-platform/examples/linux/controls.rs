//! Exercises native Find, zoom, mute and cancel-only printing through the page protocol.

use super::{
    fixture::{Fixture, LIMIT, reserve_profile},
    lifecycle::require,
    server::Server,
};
use std::{
    thread,
    time::{Duration, Instant},
};
use syrinx_platform::browser::{self, BrowserCommand, BrowserEvent, PageAction};

pub fn response(route: &str) -> Option<&'static str> {
    (route == "/controls").then_some("<!doctype html><title>controls-ready</title><link rel=icon href=data:,><p>syntheticneedle syntheticneedle SYNTHETICNEEDLE</p><input autofocus aria-label='Synthetic input'>")
}

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.parent.show()?;
    fixture.create(3000, server.url("/controls"))?;
    fixture.loaded(3000)?;
    fixture.focus_page(3000)?;
    require(
        !browser::chrome_has_keyboard_focus(fixture.parent.handle()),
        "Focused page still reports chrome focus",
    )?;
    fixture.parent.focus()?;
    growing_find(&mut fixture)?;
    fixture.focus_page(3000)?;
    for (request, text, sensitive, count) in [
        (1, "syntheticneedle", false, 3),
        (2, "syntheticneedle", true, 2),
        (3, "absent-synthetic", false, 0),
        (4, "", false, 0),
    ] {
        action(
            &mut fixture,
            PageAction::Find {
                request,
                text: text.into(),
                case_sensitive: sensitive,
            },
        )?;
        find_result(
            &mut fixture,
            request,
            count,
            if count == 0 { -1 } else { 1 },
        )?;
        if count > 1 {
            for active in (2..=count).chain(std::iter::once(1)) {
                action(&mut fixture, PageAction::FindNext)?;
                find_result(&mut fixture, request, count, active)?;
            }
            for active in (1..=count).rev() {
                action(&mut fixture, PageAction::FindPrevious)?;
                find_result(&mut fixture, request, count, active)?;
            }
        }
        action(&mut fixture, PageAction::StopFind)?;
        fixture.barrier()?;
        action(&mut fixture, PageAction::FindNext)?;
        action(&mut fixture, PageAction::FindPrevious)?;
        fixture.barrier_checked(require_stopped_find)?;
        fixture.watch_for(
            "StopFind retires match reports",
            Duration::from_millis(100),
            require_stopped_find,
        )?;
    }
    for (command, expected) in [
        (PageAction::ZoomIn, 1.1),
        (PageAction::ZoomOut, 1.),
        (PageAction::ZoomIn, 1.1),
        (PageAction::ZoomReset, 1.),
    ] {
        action(&mut fixture, command)?;
        fixture.wait("zoom readback", |event, _| {
            matches!(event, BrowserEvent::ZoomChanged { id: 3000, factor }
                if (factor - expected).abs() < 0.001)
        })?;
    }
    mute_states(&mut fixture)?;
    cancel_print(&mut fixture)?;
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!(
        "Linux controls: Find counts/steps/wrap/chrome focus, zoom, shown/hidden mute and print cancellation passed."
    );
    Ok(())
}

fn growing_find(fixture: &mut Fixture) -> Result<(), String> {
    let parent = fixture.parent.handle();
    for (request, text) in (10..).zip(["s", "sy", "syn", "synthetic", "syntheticneedle"]) {
        action(
            fixture,
            PageAction::Find {
                request,
                text: text.into(),
                case_sensitive: false,
            },
        )?;
        fixture.wait_checked("growing Find keeps chrome keys", |event, _| {
            require(
                browser::chrome_has_keyboard_focus(parent),
                "Find moved keyboard focus out of chrome",
            )?;
            Ok(
                matches!(event, BrowserEvent::FindResult { id: 3000, request: actual, .. }
                if *actual == request),
            )
        })?;
    }
    fixture.barrier()?;
    require(
        browser::chrome_has_keyboard_focus(parent),
        "Settled Find stole chrome focus",
    )?;
    action(fixture, PageAction::StopFind)
}

fn find_result(fixture: &mut Fixture, request: u64, count: i32, active: i32) -> Result<(), String> {
    fixture.wait_checked("native Find result", |event, _| {
        let BrowserEvent::FindResult {
            id: 3000,
            request: actual,
            count: found,
            active: selected,
        } = event
        else {
            return Ok(false);
        };
        if *actual != request {
            return Ok(false);
        }
        require(
            (*found, *selected) == (count, active),
            &format!("Find expected {active} of {count}, received {selected} of {found}"),
        )?;
        Ok(true)
    })
}

fn mute_states(fixture: &mut Fixture) -> Result<(), String> {
    for shown in [true, false, true] {
        fixture.send(BrowserCommand::ShowActive(shown.then_some(3000)))?;
        fixture.barrier()?;
        for muted in [true, false] {
            fixture.send(BrowserCommand::SetMuted { id: 3000, muted })?;
            fixture.wait("native mute readback", |event, _| {
                matches!(event, BrowserEvent::AudioChanged { id: 3000, muted: actual, .. }
                    if *actual == muted)
            })?;
        }
    }
    fixture.focus_page(3000)
}

fn cancel_print(fixture: &mut Fixture) -> Result<(), String> {
    require(
        !fixture.parent.print_dialog_open()?,
        "A print dialog existed before the fixture print command",
    )?;
    action(fixture, PageAction::Print)?;
    let deadline = Instant::now() + LIMIT;
    while Instant::now() < deadline {
        if fixture.parent.print_dialog_open()? {
            // Require the command acknowledgement while the native modal loop is open.
            fixture.barrier()?;
            require(
                fixture.parent.print_dialog_open()?,
                "Print dialog closed before its reentry check",
            )?;
            require(
                fixture.parent.cancel_print_dialog()?,
                "Owned print dialog vanished before cancellation",
            )?;
            fixture.barrier()?;
            require(
                !fixture.parent.print_dialog_open()?,
                "Print dialog remained open after cancellation",
            )?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("Owned GTK print dialog did not appear for cancellation".into())
}

fn action(fixture: &mut Fixture, action: PageAction) -> Result<(), String> {
    fixture.send(BrowserCommand::PageAction { id: 3000, action })
}

fn require_stopped_find(event: &BrowserEvent) -> Result<(), String> {
    require(
        !matches!(event, BrowserEvent::FindResult { id: 3000, .. }),
        "Stopped Find still emitted a match report",
    )
}
