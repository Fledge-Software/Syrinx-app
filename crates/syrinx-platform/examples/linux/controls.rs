//! Exercises native Find, zoom, mute and cancel-only printing through the page protocol.

#[path = "controls/find_observation.rs"]
mod find_observation;
#[path = "controls/overlap_observation.rs"]
mod overlap_observation;

use find_observation::FindObservation;
use overlap_observation::{OVERLAP, OverlapObservation};

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

const PAGE: &str = r#"<!doctype html><title>controls-ready</title><link rel=icon href=data:,>
<p><span data-occurrence=1>syntheticneedle</span>
<span data-occurrence=2>syntheticneedle</span>
<span data-occurrence=3>SYNTHETICNEEDLE</span></p><script>
let operation=0,armed=false,baseline=null;
function selectedOccurrence(){
  const selection=getSelection();
  if(!selection||selection.isCollapsed||selection.toString().toLowerCase()!=='syntheticneedle')return null;
  const owner=node=>(node?.nodeType===Node.TEXT_NODE?node.parentElement:node)?.closest('[data-occurrence]');
  const first=owner(selection.anchorNode),last=owner(selection.focusNode);
  return first&&first===last?Number(first.dataset.occurrence):null;
}
addEventListener('keydown',event=>{
  if(event.key!=='r'&&event.key!=='p')return;
  event.preventDefault();
  armed=false;
  if(event.key==='r')getSelection().removeAllRanges();
  baseline=selectedOccurrence();
  if(event.key==='p'&&baseline===null){document.title='fixture-find-error:no-selection';return;}
  operation+=1;armed=true;
  document.title='fixture-find-armed:'+operation;
});
document.addEventListener('selectionchange',()=>{
  const occurrence=selectedOccurrence();
  if(!armed||occurrence===null||occurrence===baseline)return;
  armed=false;
  document.title='fixture-find-selected:'+operation+':'+occurrence;
});
</script>"#;

/// Reports every selected occurrence itself, so overlapping Find needs no armed key or focus.
const OVERLAP_PAGE: &str = r#"<!doctype html><title>overlap-ready</title><link rel=icon href=data:,>
<p><span data-occurrence=1>overlapneedle</span>
<span data-occurrence=2>overlapneedle</span>
<span data-occurrence=3>OVERLAPNEEDLE</span></p><p>oo ova</p><script>
document.addEventListener('selectionchange',()=>{
  const selection=getSelection();
  if(!selection||selection.toString().toLowerCase()!=='overlapneedle')return;
  const owner=selection.anchorNode?.parentElement?.closest('[data-occurrence]');
  if(owner)document.title='overlap-selected:'+owner.dataset.occurrence;
});
</script>"#;

pub fn response(route: &str) -> Option<&'static str> {
    match route {
        "/controls" => Some(PAGE),
        "/controls-overlap" => Some(OVERLAP_PAGE),
        _ => None,
    }
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
    find_cases(&mut fixture)?;
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
    overlapping_find(&mut fixture, &server)?;
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!(
        "Linux controls: Find counts/steps/wrap/overlap/chrome focus, zoom, shown/hidden mute and print cancellation passed."
    );
    Ok(())
}

/// Reset each initial query, then compare native selected spans with literal step sequences.
fn find_cases(fixture: &mut Fixture) -> Result<(), String> {
    let mut operation = 0;
    for (request, text, sensitive, count) in [
        (1, "syntheticneedle", false, 3),
        (2, "syntheticneedle", true, 2),
        (3, "absent-synthetic", false, 0),
        (4, "", false, 0),
    ] {
        let observed_operation = if count > 0 {
            Some(arm_selection(fixture, &mut operation, true)?)
        } else {
            None
        };
        action(
            fixture,
            PageAction::Find {
                request,
                text: text.into(),
                case_sensitive: sensitive,
            },
        )?;
        find_result(
            fixture,
            request,
            count,
            if count == 0 { -1 } else { 1 },
            observed_operation,
        )?;
        let (next, previous): (&[i32], &[i32]) = match count {
            3 => (&[2, 3, 1], &[3, 2, 1]),
            2 => (&[2, 1], &[2, 1]),
            _ => (&[], &[]),
        };
        for (forward, expected) in [(true, next), (false, previous)] {
            for &occurrence in expected {
                let operation = arm_selection(fixture, &mut operation, false)?;
                action(
                    fixture,
                    if forward {
                        PageAction::FindNext
                    } else {
                        PageAction::FindPrevious
                    },
                )?;
                find_result(fixture, request, count, occurrence, Some(operation))?;
            }
        }
        action(fixture, PageAction::StopFind)?;
        fixture.barrier()?;
        action(fixture, PageAction::FindNext)?;
        action(fixture, PageAction::FindPrevious)?;
        fixture.barrier_checked(require_stopped_find)?;
        fixture.watch_for(
            "StopFind retires match reports",
            Duration::from_millis(100),
            require_stopped_find,
        )?;
    }
    Ok(())
}

/// The authored key handler only arms observation; it never chooses the next match.
fn arm_selection(fixture: &mut Fixture, operation: &mut u64, reset: bool) -> Result<u64, String> {
    *operation += 1;
    let expected = format!("fixture-find-armed:{operation}");
    fixture.parent.key(
        fixture.only_child()?,
        u32::from(if reset { b'r' } else { b'p' }),
    )?;
    fixture.wait_checked("authored Find selection observer armed", |event, _| {
        let BrowserEvent::Title { id: 3000, title } = event else {
            return Ok(false);
        };
        require(
            !title.starts_with("fixture-find-error:"),
            "Authored Find observer has no native selection to step from",
        )?;
        Ok(title == &expected)
    })?;
    Ok(*operation)
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

fn find_result(
    fixture: &mut Fixture,
    request: u64,
    count: i32,
    active: i32,
    operation: Option<u64>,
) -> Result<(), String> {
    let mut observed = FindObservation::new(request, count, active, operation);
    fixture.wait_checked("native Find result", |event, _| observed.observe(event))
}

/// Send Find operations in one burst; WebKit's replies must reach only their own request (#245).
fn overlapping_find(fixture: &mut Fixture, server: &Server) -> Result<(), String> {
    fixture.create(OVERLAP, server.url("/controls-overlap"))?;
    fixture.loaded(OVERLAP)?;
    let find = |request, text: &str, case_sensitive| PageAction::Find {
        request,
        text: text.into(),
        case_sensitive,
    };
    let only = [find(50, "OVERLAPNEEDLE", true)];
    burst(fixture, only, 50, &[(1, 1)], Some(&[3]), &[])?;
    // A step sent before the new search answers steps through the new query's three matches.
    unselect(fixture)?;
    let early_next = [find(51, "overlapneedle", false), PageAction::FindNext];
    burst(
        fixture,
        early_next,
        51,
        &[(3, 1), (3, 2)],
        Some(&[1, 2]),
        &[],
    )?;
    // Typing faster than WebKit answers, through a query with no match, ends on the last query.
    // WebKit starts each search from the current selection and GTK drops its index, so no
    // occurrence is asserted; each query's count differs, so a misplaced reply shows.
    let typing = [
        find(52, "o", false),
        find(53, "ov", false),
        find(54, "overlapx", false),
        find(55, "overlap", false),
    ];
    let earlier = [(52, 6), (53, 4), (54, 0)];
    burst(fixture, typing, 55, &[(3, 1)], None, &earlier)?;
    unselect(fixture)?;
    let stepping = [
        find(56, "overlapneedle", false),
        PageAction::FindNext,
        PageAction::FindNext,
        PageAction::FindPrevious,
    ];
    let reports = [(3, 1), (3, 2), (3, 3), (3, 2)];
    burst(fixture, stepping, 56, &reports, Some(&[1, 2, 3, 2]), &[])?;
    // StopFind retires replies still in flight, so they cannot reach the next search.
    let stopped = [
        find(57, "ov", false),
        PageAction::FindNext,
        PageAction::StopFind,
        find(58, "overlapneedle", false),
    ];
    burst(fixture, stopped, 58, &[(3, 1)], None, &[(57, 4)])?;
    action_on(fixture, OVERLAP, PageAction::StopFind)?;
    fixture.send(BrowserCommand::Close { id: OVERLAP })?;
    fixture.barrier()
}

/// Reload the overlap page so the next search starts with no selection, at the first match.
fn unselect(fixture: &mut Fixture) -> Result<(), String> {
    fixture.send(BrowserCommand::Reload { id: OVERLAP })?;
    fixture.loaded(OVERLAP)
}

/// Send `actions` back to back, then require the final request's reports and selections.
fn burst(
    fixture: &mut Fixture,
    actions: impl IntoIterator<Item = PageAction>,
    request: u64,
    reports: &[(i32, i32)],
    selections: Option<&[u32]>,
    earlier: &[(u64, i32)],
) -> Result<(), String> {
    for action in actions {
        action_on(fixture, OVERLAP, action)?;
    }
    let mut overlap = OverlapObservation::new(request, reports, selections, earlier);
    fixture.wait_checked("overlapping Find results", |event, _| {
        overlap.observe(event)?;
        Ok(overlap.complete())
    })?;
    fixture.barrier_checked(|event| overlap.observe(event))?;
    fixture.watch_for(
        "no late overlapping Find result",
        Duration::from_millis(200),
        |event| overlap.observe(event),
    )?;
    require(
        overlap.complete(),
        &format!("Overlapping Find did not settle: {}", overlap.describe()),
    )
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
    action_on(fixture, 3000, action)
}

fn action_on(fixture: &mut Fixture, id: u64, action: PageAction) -> Result<(), String> {
    fixture.send(BrowserCommand::PageAction { id, action })
}

fn require_stopped_find(event: &BrowserEvent) -> Result<(), String> {
    require(
        !matches!(event, BrowserEvent::FindResult { id: 3000, .. }),
        "Stopped Find still emitted a match report",
    )
}
