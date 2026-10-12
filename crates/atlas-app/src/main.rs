//! Opens the integrated browser and keeps its native workers alive through orderly shutdown.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod appearance;
mod navigation;
mod palette;
mod palette_sample;
mod session;
mod smoke_watchdog;
mod theme;
mod ui;

use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, px, size,
};
use gpui_component::{Root, Theme, ThemeMode, ThemeToken};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(not(target_os = "windows"))]
compile_error!("Atlas v1 supports Windows 11 only.");

fn main() -> ExitCode {
    let options = match run_mode() {
        Ok(RunMode::Browser(options)) => options,
        Ok(RunMode::PaletteSample) => return run_palette_sample(),
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    // The GUI's orderly timeout cannot run while startup or its event thread is stalled.
    let watchdog = if options.smoke {
        match smoke_watchdog::SmokeWatchdog::start() {
            Ok(watchdog) => Some(watchdog),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let outcome = Arc::new(AtomicU8::new(0));
    let result = outcome.clone();
    let smoke = options.smoke;
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        // Root must not paint over the native page's rectangle (the Phase 0 stacking trap).
        Theme::global_mut(cx).tokens.background =
            ThemeToken::new(gpui::transparent_black(), gpui::transparent_black().into());
        ui::bind_keys(cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(820.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Atlas · integrated preview".into()),
                    ..Default::default()
                }),
                window_background: WindowBackgroundAppearance::MicaBackdrop,
                window_min_size: Some(size(px(800.), px(500.))),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| {
                    ui::BrowserView::new(
                        options.directory,
                        options.smoke,
                        result.clone(),
                        window,
                        cx,
                    )
                });
                let close_view = view.clone();
                window.on_window_should_close(cx, move |_, cx| {
                    close_view.update(cx, |view, cx| view.request_close(cx));
                    false
                });
                if options.smoke {
                    let timed_view = view.downgrade();
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(Duration::from_secs(20))
                            .await;
                        if timed_view
                            .update(cx, |view, cx| view.smoke_timeout(cx))
                            .is_err()
                        {
                            eprintln!("Smoke window closed before its timeout check.");
                        }
                    })
                    .detach();
                }
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        if opened.is_err() {
            result.store(2, Ordering::Release);
            eprintln!("Cannot open the Atlas window.");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
    if let Some(watchdog) = watchdog
        && let Err(error) = watchdog.cancel()
    {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    if outcome.load(Ordering::Acquire) == 2 || smoke && outcome.load(Ordering::Acquire) != 1 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_palette_sample() -> ExitCode {
    // Development review window for P1-07's tokens; it opens no profile, page or worker.
    let failed = Arc::new(AtomicU8::new(0));
    let result = failed.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(message) = palette_sample::open(cx) {
            eprintln!("{message}");
            result.store(1, Ordering::Release);
            cx.quit();
        }
    });
    if failed.load(Ordering::Acquire) != 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// What this launch does: the browser, or the development-only token review window.
enum RunMode {
    Browser(Options),
    PaletteSample,
}

struct Options {
    directory: PathBuf,
    smoke: bool,
}

fn run_mode() -> Result<RunMode, &'static str> {
    let mut smoke = false;
    let mut test_directory = None;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--palette-sample" {
            if arguments.next().is_some() || smoke || test_directory.is_some() {
                return Err("--palette-sample takes no other arguments.");
            }
            return Ok(RunMode::PaletteSample);
        } else if argument == "--smoke" {
            smoke = true;
        } else if argument == "--test-data-dir" {
            test_directory = Some(PathBuf::from(
                arguments
                    .next()
                    .ok_or("The test profile needs a directory.")?,
            ));
        } else {
            return Err("Atlas accepts --smoke, --test-data-dir <directory> or --palette-sample.");
        }
    }
    if smoke && test_directory.is_some() {
        return Err("The automatic smoke check always uses a fresh profile.");
    }
    let directory = if smoke {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "The system clock is unavailable.")?
            .as_nanos();
        std::env::temp_dir().join(format!("atlas-window-smoke-{}-{nonce}", std::process::id()))
    } else if let Some(directory) = test_directory {
        directory
    } else {
        PathBuf::from(
            std::env::var_os("LOCALAPPDATA").ok_or("Windows local app data is unavailable.")?,
        )
        .join("AtlasProductPreview")
    };
    if smoke {
        reserve_smoke_directory(&directory)?;
    }
    Ok(RunMode::Browser(Options { directory, smoke }))
}

fn reserve_smoke_directory(directory: &Path) -> Result<(), &'static str> {
    // Refuse collisions rather than letting a timed smoke run adopt an existing profile.
    std::fs::create_dir(directory).map_err(|_| "Cannot reserve a fresh synthetic smoke directory.")
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use super::*;

    #[test]
    fn smoke_reservation_refuses_existing_directory_without_changing_contents() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("synthetic clock follows epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "atlas-smoke-reservation-test-{}-{nonce}",
            std::process::id()
        ));
        reserve_smoke_directory(&directory).expect("exclusively reserve test-owned directory");
        let sentinel = directory.join("synthetic-sentinel.txt");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&sentinel)
            .expect("create test-owned sentinel");
        file.write_all(b"synthetic contents must remain unchanged")
            .expect("write synthetic sentinel");
        drop(file);

        assert_eq!(
            reserve_smoke_directory(&directory),
            Err("Cannot reserve a fresh synthetic smoke directory.")
        );
        assert_eq!(
            fs::read(&sentinel).expect("read preserved synthetic sentinel"),
            b"synthetic contents must remain unchanged"
        );
        let entries = fs::read_dir(&directory)
            .expect("inspect test-owned directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read every test-owned entry");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "synthetic-sentinel.txt");

        // Only the exclusive reservation above grants ownership of these two paths.
        fs::remove_file(sentinel).expect("remove test-owned sentinel");
        fs::remove_dir(directory).expect("remove empty test-owned directory");
    }
}
