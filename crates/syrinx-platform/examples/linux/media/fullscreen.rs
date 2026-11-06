//! Verifies page and chrome fullscreen exits restore the exact native page rectangle.

use super::super::{
    fixture::{Fixture, reserve_profile},
    lifecycle::{require, require_geometry},
    server::Server,
};
use std::{thread, time::Duration};
use syrinx_platform::browser::{BrowserCommand, BrowserEvent, PageBounds};

const VIDEO: &str = r#"<!doctype html><title>video-ready</title><link rel=icon href=data:,><style>body{margin:0}video{width:100%;height:100%;object-fit:contain}</style>
<video id=v muted autoplay></video><script>
const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;
const ctx=canvas.getContext('2d');ctx.fillStyle='#258';ctx.fillRect(0,0,640,360);
ctx.fillStyle='white';ctx.font='32px sans-serif';ctx.fillText('Syrinx synthetic video',100,180);
v.srcObject=canvas.captureStream(0);
addEventListener('keydown',e=>{if(e.key==='Enter')v.requestFullscreen().catch(()=>document.title='fullscreen-error');if(e.key==='x')document.exitFullscreen().catch(()=>document.title='fullscreen-error');});
addEventListener('fullscreenchange',()=>document.title=document.fullscreenElement?'video-full':'video-exited');
</script>"#;

pub fn response(route: &str) -> Option<&'static str> {
    (route == "/synthetic-video").then_some(VIDEO)
}

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.parent.show()?;
    let card = PageBounds {
        x: 12,
        y: 24,
        width: 480,
        height: 320,
    };
    fixture.send(BrowserCommand::Resize(card))?;
    fixture.create(1500, server.url("/synthetic-video"))?;
    fixture.loaded(1500)?;
    fixture.focus_page(1500)?;
    let child = fixture.only_child()?;
    let original = fixture.parent.geometry(child)?;
    require_geometry(
        original,
        (12, 24, 480, 320),
        "Initial video rectangle is wrong",
    )?;
    for exit in 0..4 {
        // WebKit may throttle immediate fullscreen re-entry, just as WebView2 does.
        thread::sleep(Duration::from_millis(1500));
        fixture.focus_page(1500)?;
        fixture.parent.key(child, 0xff0d)?;
        wait_fullscreen(&mut fixture, true)?;
        fixture.send(BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }))?;
        fixture.barrier()?;
        require_geometry(
            fixture.parent.geometry(child)?,
            (0, 0, 640, 480),
            "Fullscreen page did not fill the requested viewport",
        )?;
        match exit {
            0 => fixture.parent.key(child, 0xff1b)?,
            1 => {
                fixture.parent.key(child, 0xffc8)?;
                fixture.wait("page F11 fullscreen shortcut", |event, _| {
                    matches!(
                        event,
                        BrowserEvent::Shortcut(
                            syrinx_platform::browser::BrowserShortcut::Fullscreen
                        )
                    )
                })?;
                fixture.send(BrowserCommand::ExitFullscreen { id: 1500 })?;
            }
            2 => fixture.parent.key(child, u32::from(b'x'))?,
            _ => {
                fixture.parent.focus()?;
                require(
                    syrinx_platform::browser::chrome_has_keyboard_focus(fixture.parent.handle()),
                    "Fullscreen chrome exit did not start with chrome focus",
                )?;
                fixture.send(BrowserCommand::ExitFullscreen { id: 1500 })?;
            }
        }
        wait_fullscreen(&mut fixture, false)?;
        fixture.send(BrowserCommand::Resize(card))?;
        fixture.barrier()?;
        require_geometry(
            fixture.parent.geometry(child)?,
            original,
            "Fullscreen exit did not restore exact card bounds",
        )?;
    }
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!(
        "Linux fullscreen: page Escape, page F11 shortcut, page script and chrome-focused exit restored exact bounds."
    );
    Ok(())
}

fn wait_fullscreen(fixture: &mut Fixture, expected: bool) -> Result<(), String> {
    fixture.wait_checked("synthetic video fullscreen transition", |event, _| {
        require(
            !matches!(event,BrowserEvent::Title{id:1500,title} if title=="fullscreen-error"),
            "Authored synthetic fullscreen request failed",
        )?;
        Ok(matches!(event,BrowserEvent::Fullscreen{id:1500,active} if *active==expected))
    })
}
