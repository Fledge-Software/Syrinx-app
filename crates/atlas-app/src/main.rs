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

use atlas_platform::{
    instance::{self, Claim, LinkRequest, PrimaryInstance},
    notice,
};
use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, px, size,
};
use gpui_component::Root;
use std::{
    ffi::{OsStr, OsString},
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

/// Profile folder under `%LOCALAPPDATA%`: the installed package (owner decision, 2026-10-12)
/// uses `Atlas\data`; development and preview runs keep `AtlasProductPreview`.
const DATA_FOLDER: &str = if cfg!(feature = "installed") {
    r"Atlas\data"
} else {
    "AtlasProductPreview"
};

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let smoke_requested = arguments.iter().any(|argument| argument == "--smoke");
    let options = match parse_arguments(arguments).and_then(run_mode) {
        Ok(RunMode::Browser(options)) => options,
        Ok(RunMode::PaletteSample) => return run_palette_sample(),
        Err(message) => {
            report_startup_problem(message, smoke_requested);
            return ExitCode::FAILURE;
        }
    };
    // A second launch on this profile hands its link to the running Atlas and exits.
    let mut instance = match claim_profile(&options) {
        Ok(Some(instance)) => instance,
        Ok(None) => return ExitCode::SUCCESS,
        Err(error) => {
            report_startup_problem(&error.to_string(), options.smoke);
            return ExitCode::FAILURE;
        }
    };
    let links = match instance.take_feed(options.link.clone()) {
        Ok(links) => links,
        Err(error) => {
            report_startup_problem(&error.to_string(), options.smoke);
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
    let mut links = Some(links);
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        theme::apply_component_theme(cx);
        ui::bind_keys(cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(820.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    appears_transparent: true,
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
                        links.take(),
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
    // Release the profile only now, after the store and pages have shut down.
    let links_dropped = instance.links_dropped();
    drop(instance);
    if links_dropped && !smoke {
        notice::show_warning(
            "Atlas was closing when a link arrived, so the link was not opened. Open it again.",
        );
    }
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
    /// A checked http(s) link to open, as Windows passes one when Atlas opens a link.
    link: Option<LinkRequest>,
}

/// Becomes this profile's running Atlas, or returns `None` once the running one has the link.
fn claim_profile(options: &Options) -> Result<Option<PrimaryInstance>, instance::InstanceError> {
    let request = options.link.clone().unwrap_or(LinkRequest::Activate);
    match instance::claim(&options.directory, &request, instance::FORWARD_PATIENCE)? {
        Claim::Primary(instance) => Ok(Some(instance)),
        Claim::Forwarded { .. } => Ok(None),
    }
}

/// A launch from Windows has no console, so its problems also appear in a message box.
fn report_startup_problem(message: &str, smoke: bool) {
    eprintln!("{message}");
    if !smoke {
        notice::show_warning(message);
    }
}

/// What the command line asked for, before a profile is chosen.
#[derive(Debug, Default, PartialEq)]
struct Arguments {
    palette_sample: bool,
    smoke: bool,
    test_directory: Option<PathBuf>,
    link: Option<LinkRequest>,
}

const USAGE: &str = "Atlas accepts --smoke, --test-data-dir <directory>, --palette-sample \
                     or -- followed by one http(s) link.";

/// Reads options, then at most one link after `--`. Everything after `--` is the link, so a
/// crafted link from a registered handler cannot add options such as `--test-data-dir`.
fn parse_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Arguments, &'static str> {
    let mut parsed = Arguments::default();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            parsed.link = Some(link_after_marker(arguments)?);
            break;
        } else if argument == "--palette-sample" {
            parsed.palette_sample = true;
        } else if argument == "--smoke" {
            parsed.smoke = true;
        } else if argument == "--test-data-dir" {
            let directory = arguments
                .next()
                .ok_or("The test profile needs a directory.")?;
            parsed.test_directory = Some(PathBuf::from(directory));
        } else {
            return Err(USAGE);
        }
    }
    let other_than_sample =
        parsed.smoke || parsed.test_directory.is_some() || parsed.link.is_some();
    if parsed.palette_sample && other_than_sample {
        return Err("--palette-sample takes no other arguments.");
    }
    if parsed.smoke && (parsed.test_directory.is_some() || parsed.link.is_some()) {
        return Err("The automatic smoke check always uses a fresh profile and no link.");
    }
    Ok(parsed)
}

/// Exactly one argument may follow `--`, and it must be an http(s) link.
fn link_after_marker(
    mut rest: impl Iterator<Item = OsString>,
) -> Result<LinkRequest, &'static str> {
    let link = rest.next().ok_or("A link must follow --.")?;
    if rest.next().is_some() {
        return Err("Atlas opens one link, and nothing may follow it.");
    }
    link_argument(&link)
}

fn run_mode(arguments: Arguments) -> Result<RunMode, &'static str> {
    if arguments.palette_sample {
        return Ok(RunMode::PaletteSample);
    }
    let Arguments {
        smoke,
        test_directory,
        link,
        ..
    } = arguments;
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
        .join(DATA_FOLDER)
    };
    if smoke {
        reserve_smoke_directory(&directory)?;
    }
    Ok(RunMode::Browser(Options {
        directory,
        smoke,
        link,
    }))
}

/// Accepts only an http(s) link; any other argument is refused, never opened or run.
fn link_argument(argument: &OsStr) -> Result<LinkRequest, &'static str> {
    let text = argument
        .to_str()
        .ok_or("Atlas opens only http and https links.")?;
    LinkRequest::open(text).map_err(|_| "Atlas opens only http and https links.")
}

fn reserve_smoke_directory(directory: &Path) -> Result<(), &'static str> {
    // Refuse collisions rather than letting a timed smoke run adopt an existing profile.
    std::fs::create_dir(directory).map_err(|_| "Cannot reserve a fresh synthetic smoke directory.")
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments, &'static str> {
        parse_arguments(arguments.iter().map(OsString::from))
    }

    #[test]
    fn a_link_comes_only_after_the_marker() {
        let parsed = parse(&["--", "https://Example.TEST/page"]).expect("link");
        assert_eq!(
            parsed.link,
            Some(LinkRequest::open("https://example.test/page").expect("web"))
        );
        let tested = parse(&[
            "--test-data-dir",
            r"C:\Synthetic",
            "--",
            "https://example.test/",
        ])
        .expect("test profile and link");
        assert_eq!(tested.test_directory, Some(PathBuf::from(r"C:\Synthetic")));
        assert!(tested.link.is_some());
        assert_eq!(parse(&["https://example.test/"]), Err(USAGE));
    }

    #[test]
    fn nothing_can_be_smuggled_after_the_link_marker() {
        for arguments in [
            &[
                "--",
                "https://example.test/",
                "--test-data-dir",
                r"C:\Elsewhere",
            ][..],
            &["--", "--test-data-dir", r"C:\Elsewhere"][..],
            &[
                "--",
                "https://example.test/",
                "https://second.example.test/",
            ][..],
            &["--", "javascript:alert(1)"][..],
            &["--", "file:///C:/Windows/System32/cmd.exe"][..],
            &["--"][..],
            &["--smoke", "--", "https://example.test/"][..],
            &["--palette-sample", "--", "https://example.test/"][..],
        ] {
            let parsed = parse(arguments);
            assert!(parsed.is_err(), "{arguments:?} gave {parsed:?}");
        }
    }

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
