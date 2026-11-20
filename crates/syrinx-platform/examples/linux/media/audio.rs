//! Requires native audio start/stop reports while the authored synthetic tone stays muted.

use super::super::{
    fixture::{Fixture, reserve_profile},
    lifecycle::require,
    server::Server,
};
use std::time::Duration;
use syrinx_platform::browser::{BrowserCommand, BrowserEvent};

const TONE: &str = r#"<!doctype html><title>tone-ready</title><link rel=icon href=data:,><p>Enter starts a muted synthetic tone; S stops it.</p><script>
let context,tone;
addEventListener('keydown',event=>{
if(event.key==='Enter'&&!tone){context=new AudioContext();tone=new OscillatorNode(context,{frequency:30});
const gain=new GainNode(context,{gain:0.05});tone.connect(gain).connect(context.destination);tone.start();
context.resume().then(()=>document.title='tone-playing',()=>document.title='tone-error');}
if(event.key==='s'&&tone){tone.stop();tone=null;context.close().then(()=>document.title='tone-stopped',()=>document.title='tone-error');}
});
</script>"#;

pub fn response(route: &str) -> Option<&'static str> {
    (route == "/synthetic-tone").then_some(TONE)
}

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.parent.show()?;
    fixture.create(1600, server.url("/synthetic-tone"))?;
    fixture.loaded(1600)?;
    fixture.focus_page(1600)?;
    let child = fixture.only_child()?;
    fixture.send(BrowserCommand::SetMuted {
        id: 1600,
        muted: true,
    })?;
    fixture.wait("mute before synthetic tone", |event, _| {
        matches!(
            event,
            BrowserEvent::AudioChanged {
                id: 1600,
                muted: true,
                ..
            }
        )
    })?;
    fixture.parent.key(child, 0xff0d)?;
    transition(&mut fixture, true, "tone-playing")?;
    fixture.send(BrowserCommand::ShowActive(None))?;
    fixture.barrier_checked(require_playing_muted)?;
    require(
        !fixture.parent.visible(child)?,
        "Playing tone page did not hide",
    )?;
    fixture.watch_for(
        "muted playing page stays audible while hidden",
        Duration::from_secs(15),
        require_playing_muted,
    )?;
    fixture.send(BrowserCommand::ShowActive(Some(1600)))?;
    fixture.focus_page(1600)?;
    fixture.parent.key(child, u32::from(b's'))?;
    transition(&mut fixture, false, "tone-stopped")?;
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!(
        "Linux audio: muted authored tone start/stop and native AudioChanged.playing reports passed."
    );
    Ok(())
}

fn transition(fixture: &mut Fixture, playing: bool, title: &str) -> Result<(), String> {
    let (mut authored, mut native) = (false, false);
    fixture.wait_checked("muted native audio transition", |event, _| {
        match event {
            BrowserEvent::Title {
                id: 1600,
                title: actual,
            } => {
                require(
                    actual != "tone-error",
                    "Authored AudioContext start/stop failed",
                )?;
                authored |= actual == title;
            }
            BrowserEvent::AudioChanged {
                id: 1600,
                muted,
                playing: actual,
            } => {
                require(*muted, "Synthetic tone unexpectedly became unmuted")?;
                native |= *actual == playing;
            }
            _ => {}
        }
        Ok(authored && native)
    })
}

fn require_playing_muted(event: &BrowserEvent) -> Result<(), String> {
    if let BrowserEvent::AudioChanged {
        id: 1600,
        muted,
        playing,
    } = event
    {
        require(
            *playing,
            "Muted page reported quiet while its tone still played hidden",
        )?;
        require(*muted, "Hidden synthetic tone became unmuted")?;
    }
    Ok(())
}
