//! Proves a hidden page that plays sound stays awake, and sleeps `AUDIO_GRACE` after it stops.

use super::*;
use syrinx_platform::browser::AUDIO_GRACE;
use windows::Win32::UI::WindowsAndMessaging::{SW_SHOW, ShowWindow};

/// Loading, starting the tone, holding it, stopping it and the audio grace add up to about 90 s.
pub(super) const DEADLINE: Duration = Duration::from_secs(150);
/// How long the hidden, playing page is watched for a suspension that must not come.
const PLAYING_HOLD: Duration = Duration::from_secs(15);
/// How late after the audio grace the suspension may still arrive before the suite fails.
const SLEEP_MARGIN: Duration = Duration::from_secs(30);
/// `stopped` is taken after the `tone-stopped` title round-trips through `context.close()` and
/// the event channel, so the runtime may have reported quiet up to this much earlier, and the
/// sleep may then come this much sooner than `stopped + AUDIO_GRACE`.
const QUIET_REPORT_SLACK: Duration = Duration::from_secs(2);
const TONE_ID: u64 = 1600;

/// A faint 30 Hz tone at about -29 dBFS: loud enough for WebView2's audibility detection, below
/// what most speakers reproduce. It stands in for muting the web view, which needs a mute
/// command Syrinx does not have yet (#95). Enter starts it, so the posted key press gives
/// Chromium's autoplay rule its user gesture; S stops it.
const TONE: &str = r#"<!doctype html><meta charset=utf-8><title>tone-ready</title>
<link rel=icon href=data:,><p>Enter: start a faint synthetic tone. S: stop it.</p><script>
let context,tone;
addEventListener('keydown',event=>{
if(event.key==='Enter'&&!tone){context=new AudioContext();tone=new OscillatorNode(context,{frequency:30});
const gain=new GainNode(context,{gain:0.05});tone.connect(gain).connect(context.destination);tone.start();
context.resume().then(()=>document.title='tone-playing',()=>document.title='tone-error');}
if(event.key==='s'&&tone){tone.stop();tone=null;
context.close().then(()=>document.title='tone-stopped',()=>document.title='tone-error');}
});
</script>"#;

pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    request.starts_with(b"GET /synthetic-tone ").then_some(TONE)
}

pub(super) fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    while !matches!(checked_event(events, deadline)?, BrowserEvent::Ready) {}
    let base = url
        .strip_suffix("/close")
        .ok_or("Synthetic fixture URL shape changed.")?;
    let child = open_tone_page(
        host,
        events,
        parent,
        &format!("{base}/synthetic-tone"),
        deadline,
    )?;
    start_tone(events, child, deadline)?;
    hold_hidden_while_playing(host, events, deadline)?;
    let stopped = stop_tone(host, events, child, deadline)?;
    sleep_after_the_grace(host, events, stopped, deadline)?;
    send(host, BrowserCommand::Close { id: TONE_ID })?;
    lifecycle::barrier(host, events, deadline)?;
    lifecycle::require_children(parent, &[])
}

/// Creates and shows the tone page in the card, waits for its load and focuses it for the keys.
fn open_tone_page(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<HWND, String> {
    send(
        host,
        BrowserCommand::Resize(PageBounds {
            x: 12,
            y: 24,
            width: 480,
            height: 320,
        }),
    )?;
    send(
        host,
        BrowserCommand::Create {
            id: TONE_ID,
            url: url.into(),
        },
    )?;
    // A shown page runs its script on time; a hidden page's timers run seconds late (trap).
    unsafe {
        let _previous_visibility = ShowWindow(parent, SW_SHOW);
    }
    send(host, BrowserCommand::ShowActive(Some(TONE_ID)))?;
    while !matches!(
        checked_event(events, deadline)?,
        BrowserEvent::NavigationCompleted {
            id: TONE_ID,
            success: true,
            ..
        }
    ) {}
    send(host, BrowserCommand::Focus { id: TONE_ID })?;
    lifecycle::barrier(host, events, deadline)?;
    lifecycle::only_child(parent)
}

/// Posts Enter and waits for the page to report its tone playing.
fn start_tone(
    events: &mut UnboundedReceiver<BrowserEvent>,
    child: HWND,
    deadline: Instant,
) -> Result<(), String> {
    suspension::key(child, 0x0D)?;
    wait_title(events, "tone-playing", deadline)?;
    println!("Synthetic tone started: a faint 30 Hz tone most speakers cannot reproduce.");
    Ok(())
}

/// Hides the playing page and fails on any suspension within `PLAYING_HOLD`.
fn hold_hidden_while_playing(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<(), String> {
    send(host, BrowserCommand::ShowActive(None))?;
    let hidden = Instant::now();
    let until = (hidden + PLAYING_HOLD).min(deadline);
    while let Some(event) = next_event_until(events, until)? {
        if is_suspended(&event) {
            return Err(suspended_while_playing(hidden.elapsed()));
        }
    }
    println!(
        "Synthetic tone page stayed awake for {} s while hidden and playing.",
        PLAYING_HOLD.as_secs()
    );
    Ok(())
}

/// Names the two readings of an early suspension: at about five seconds WebView2 never
/// reported the tone as playing audio; later, Syrinx's playback rule failed.
fn suspended_while_playing(after: Duration) -> String {
    format!(
        "Synthetic tone page was suspended {:.1} s after hiding while its tone played; about \
         5 s means WebView2 never reported the tone as playing audio (check the machine has an \
         audio output device), later means the playback rule failed.",
        after.as_secs_f64()
    )
}

/// Shows the page again, posts S and waits for the tone to stop; returns when it did.
fn stop_tone(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    child: HWND,
    deadline: Instant,
) -> Result<Instant, String> {
    send(host, BrowserCommand::ShowActive(Some(TONE_ID)))?;
    send(host, BrowserCommand::Focus { id: TONE_ID })?;
    lifecycle::barrier(host, events, deadline)?;
    suspension::key(child, 0x53)?;
    wait_title(events, "tone-stopped", deadline)?;
    Ok(Instant::now())
}

/// Hides the quiet page and requires its suspension no sooner than the audio grace.
fn sleep_after_the_grace(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    stopped: Instant,
    deadline: Instant,
) -> Result<(), String> {
    send(host, BrowserCommand::ShowActive(None))?;
    let until = (stopped + AUDIO_GRACE + SLEEP_MARGIN).min(deadline);
    loop {
        let Some(event) = next_event_until(events, until)? else {
            return Err(format!(
                "Synthetic tone page was not suspended within {} s of its tone stopping.",
                (AUDIO_GRACE + SLEEP_MARGIN).as_secs()
            ));
        };
        if !is_suspended(&event) {
            continue;
        }
        let slept_after = stopped.elapsed();
        if slept_after + QUIET_REPORT_SLACK < AUDIO_GRACE {
            return Err(format!(
                "Synthetic tone page slept {:.1} s after its tone stopped, sooner than the {} s \
                 audio grace.",
                slept_after.as_secs_f64(),
                AUDIO_GRACE.as_secs()
            ));
        }
        println!(
            "Synthetic tone page slept {:.1} s after its tone stopped (audio grace {} s).",
            slept_after.as_secs_f64(),
            AUDIO_GRACE.as_secs()
        );
        return Ok(());
    }
}

fn is_suspended(event: &BrowserEvent) -> bool {
    matches!(
        event,
        BrowserEvent::Suspended {
            id: TONE_ID,
            suspended: true
        }
    )
}

/// Waits for the tone page to report `expected`, failing at once on `tone-error`.
fn wait_title(
    events: &mut UnboundedReceiver<BrowserEvent>,
    expected: &str,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        match checked_event(events, deadline)? {
            BrowserEvent::Title { id: TONE_ID, title } if title == expected => return Ok(()),
            BrowserEvent::Title { id: TONE_ID, title } if title == "tone-error" => {
                return Err("Synthetic tone page could not start or stop its tone.".into());
            }
            _ => {}
        }
    }
}

/// The next current event, or `None` once `until` has passed, which is the hold's own end
/// rather than a failure. Only the wait's deadline message means that; an unexpected quit or
/// a closed event channel stays an error even in the hold's last tick.
fn next_event_until(
    events: &mut UnboundedReceiver<BrowserEvent>,
    until: Instant,
) -> Result<Option<BrowserEvent>, String> {
    if Instant::now() >= until {
        return Ok(None);
    }
    match next_event(events, until) {
        Ok(event) => {
            check_event(&event)?;
            Ok(Some(event))
        }
        Err(error) if error == EVENT_DEADLINE => Ok(None),
        Err(error) => Err(error),
    }
}
