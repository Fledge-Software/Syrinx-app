//! The F-tests from `docs/page-text-indexing.md`, grouped by browser run.

use std::time::{Duration, Instant};

use syrinx_platform::browser::{BrowserCommand, BrowserEvent, DiscardReason, Ineligible};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::{
        GUITHREADINFO, GW_CHILD, GetGUIThreadInfo, GetWindow, GetWindowThreadProcessId, IsChild,
        PostMessageW, WM_KEYDOWN, WM_KEYUP,
    },
};

use super::{
    Browser, CaseResult, Options, Seen, combine, pages, server::Role, server::Server,
    synthetic_policy,
};

const OUTCOME_TIMEOUT: Duration = Duration::from_secs(20);

/// Runs every requested case; each run gets a fresh browser and profile.
pub fn run_all(options: &Options, server: &Server) -> Result<Vec<CaseResult>, String> {
    let mut results = Vec::new();
    main_run(options, server, &mut results)?;
    if options.wants("F8") {
        race_run(options, server, &mut results)?;
    }
    if options.wants("F17") {
        suspension_run(server, &mut results)?;
    }
    if options.wants("F19") || options.wants("F14") {
        off_run(options, server, &mut results)?;
    }
    Ok(results)
}

/// Runs `cases` in one browser, then records any worker errors as their own failure.
fn with_browser(
    label: &'static str,
    policy: Option<syrinx_platform::browser::PageTextPolicy>,
    results: &mut Vec<CaseResult>,
    cases: impl FnOnce(&mut Browser, &mut Vec<CaseResult>) -> Result<(), String>,
) -> Result<(), String> {
    let mut browser = Browser::start(policy)?;
    let outcome = cases(&mut browser, results);
    if !browser.log.errors.is_empty() {
        results.push(CaseResult::new(
            label,
            false,
            format!("worker errors: {}", browser.log.errors.join("; ")),
        ));
    }
    combine(outcome, browser.stop())
}

fn main_run(
    options: &Options,
    server: &Server,
    results: &mut Vec<CaseResult>,
) -> Result<(), String> {
    // The production settle delay, so timing cases measure what Syrinx would do.
    let policy = synthetic_policy(syrinx_core::SETTLE_DELAY)?;
    with_browser("main run", Some(policy), results, |browser, results| {
        // An uncaptured tab keeps this run's WebView2 processes alive for F18's port checks.
        let quiet = browser.open(&server.sites.url(Role::Owner, "/landing"), false)?;
        browser.wait_finals(quiet, 1, OUTCOME_TIMEOUT)?;
        let listening_before = listening_ports(browser)?;
        type Case = fn(&mut Browser, &Server) -> Result<CaseResult, String>;
        let cases: [(&str, Case); 23] = [
            ("F1", f1),
            ("F2", f2),
            ("F3", f3),
            ("F4", f4),
            ("F5", f5),
            ("F6", f6),
            ("F7", f7),
            ("F9", f9),
            ("F10", f10),
            ("F11", f11),
            ("F12", f12),
            ("F15", f15),
            ("F16", f16),
            ("F20", f20),
            ("F21", f21),
            ("F22", f22),
            ("F23", f23),
            ("F24", f24),
            ("F25", f25),
            ("F26", f26),
            ("F27", f27),
            ("F28", f28),
            ("F13", f13),
        ];
        for (name, case) in cases {
            if options.wants(name) {
                results.push(case(browser, server)?);
            }
        }
        if options.wants("F14") {
            results.push(f14_on(browser, server)?);
        }
        if options.wants("F18") {
            results.push(f18(browser, &listening_before)?);
        }
        browser.close(quiet)
    })
}

// ---- Helpers --------------------------------------------------------------------------

/// Opens `url` in a new shown tab and waits for its first final outcome.
fn visit(browser: &mut Browser, url: &str) -> Result<(u64, Vec<Seen>), String> {
    let id = browser.open(url, true)?;
    browser.wait_finals(id, 1, OUTCOME_TIMEOUT)?;
    browser.pump_for(Duration::from_millis(300))?;
    let seen = browser.log.outcomes(id);
    browser.close(id)?;
    Ok((id, seen))
}

/// Like `visit`, but a page that never produces an outcome is reported, not an error.
fn visit_quietly(browser: &mut Browser, url: &str) -> Result<Vec<Seen>, String> {
    let id = browser.open(url, true)?;
    if browser.wait_finals(id, 1, Duration::from_secs(8)).is_err() {
        browser.pump_for(Duration::from_millis(300))?;
    }
    let seen = browser.log.outcomes(id);
    browser.close(id)?;
    Ok(seen)
}

fn requested(seen: &[Seen]) -> bool {
    seen.iter().any(|seen| matches!(seen, Seen::Requested(_)))
}

fn captured(seen: &[Seen]) -> Option<&super::Capture> {
    seen.iter().find_map(|seen| match seen {
        Seen::Captured(capture) => Some(capture),
        _ => None,
    })
}

fn has_ineligible(seen: &[Seen], wanted: &[Ineligible]) -> bool {
    seen.iter()
        .any(|seen| matches!(seen, Seen::Ineligible(reason) if wanted.contains(reason)))
}

/// A short summary with paths only, for the report table.
fn describe(seen: &[Seen]) -> String {
    seen.iter()
        .map(|seen| match seen {
            Seen::Requested(address) => format!("Requested({})", path_of(address)),
            Seen::Captured(capture) => format!(
                "Captured({}, {} docs, {} B)",
                path_of(&capture.address),
                capture.documents,
                capture.response_bytes
            ),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(" > ")
}

fn path_of(address: &str) -> &str {
    address.splitn(4, '/').nth(3).map_or(address, |path| path)
}

// ---- Main run -------------------------------------------------------------------------

fn f1(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let (_, seen) = visit(browser, &server.sites.url(Role::Plain, "/f1"))?;
    let Some(capture) = captured(&seen) else {
        return Ok(CaseResult::new("F1", false, describe(&seen)));
    };
    let matched = capture.text == pages::F1_EXPECTED;
    let detail = if matched {
        format!("text matched exactly; {}", describe(&seen))
    } else {
        format!("expected {:?}, got {:?}", pages::F1_EXPECTED, capture.text)
    };
    Ok(CaseResult::new("F1", matched, detail))
}

fn f2(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for (index, values) in pages::NO_STORE_VARIANTS.iter().enumerate() {
        let (_, seen) = visit(
            browser,
            &server.sites.url(Role::Plain, &format!("/nostore/{index}")),
        )?;
        let ok = !requested(&seen)
            && has_ineligible(
                &seen,
                &[Ineligible::NoStore, Ineligible::MalformedCacheControl],
            );
        passed &= ok;
        details.push(format!("{values:?}: {}", describe(&seen)));
    }
    Ok(CaseResult::new("F2", passed, details.join("; ")))
}

fn f3(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for (role, reason) in [
        (Role::Bank, Ineligible::BankingOrPayment),
        (Role::Pay, Ineligible::BankingOrPayment),
        (Role::Owner, Ineligible::OwnerExcluded),
    ] {
        let (_, seen) = visit(browser, &server.sites.url(role, "/landing"))?;
        passed &= !requested(&seen) && has_ineligible(&seen, &[reason]);
        details.push(format!("{}: {}", role.name(), describe(&seen)));
    }
    Ok(CaseResult::new("F3", passed, details.join("; ")))
}

fn f4(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for code in ["301", "302", "307", "308"] {
        let url = server
            .sites
            .url(Role::Plain, &format!("/go/{code}/bank/landing"));
        let (_, seen) = visit(browser, &url)?;
        passed &= !requested(&seen) && has_ineligible(&seen, &[Ineligible::BankingOrPayment]);
        details.push(format!("{code}: {}", describe(&seen)));
    }
    Ok(CaseResult::new("F4", passed, details.join("; ")))
}

fn f5(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for code in ["302", "307"] {
        let url = server
            .sites
            .url(Role::Bank, &format!("/go/{code}/plain/landing"));
        let (_, seen) = visit(browser, &url)?;
        passed &=
            !requested(&seen) && has_ineligible(&seen, &[Ineligible::RedirectThroughExcluded]);
        details.push(format!("{code}: {}", describe(&seen)));
    }
    Ok(CaseResult::new("F5", passed, details.join("; ")))
}

fn f6(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let hop = server.sites.url(Role::Plain, "/gons/302/plain/landing");
    let (_, hop_seen) = visit(browser, &hop)?;
    let hop_ok =
        captured(&hop_seen).is_some_and(|capture| capture.text == "landing-sentinel-plain");
    let last = server.sites.url(Role::Plain, "/go/302/plain/nostore/0");
    let (_, last_seen) = visit(browser, &last)?;
    let last_ok = !requested(&last_seen) && has_ineligible(&last_seen, &[Ineligible::NoStore]);
    Ok(CaseResult::new(
        "F6",
        hop_ok && last_ok,
        format!(
            "no-store on hop: {}; no-store on final: {}",
            describe(&hop_seen),
            describe(&last_seen)
        ),
    ))
}

fn f7(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for mode in ["timer", "assign", "meta"] {
        let url = server.sites.url(Role::Plain, &format!("/navigate/{mode}"));
        let (_, seen) = visit(browser, &url)?;
        passed &= !requested(&seen) && has_ineligible(&seen, &[Ineligible::BankingOrPayment]);
        details.push(format!("{mode}: {}", describe(&seen)));
    }
    Ok(CaseResult::new("F7", passed, details.join("; ")))
}

fn f9(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let (_, seen) = visit(browser, &server.sites.url(Role::Plain, "/frames"))?;
    let Some(capture) = captured(&seen) else {
        return Ok(CaseResult::new("F9", false, describe(&seen)));
    };
    let passed = capture.text == "Frame host page";
    Ok(CaseResult::new(
        "F9",
        passed,
        format!(
            "text {:?}; snapshot held {} documents (root + same-process frames); {}",
            capture.text,
            capture.documents,
            describe(&seen)
        ),
    ))
}

fn f10(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let (_, password) = visit(browser, &server.sites.url(Role::Plain, "/form-password"))?;
    let password_ok = password
        .iter()
        .any(|seen| matches!(seen, Seen::Discarded(DiscardReason::SignInOrPayment)))
        && captured(&password).is_none();
    let (_, plain) = visit(browser, &server.sites.url(Role::Plain, "/form-plain"))?;
    let plain_ok = captured(&plain).is_some_and(|capture| {
        capture.text == "Comment page" && !capture.text.contains("sentinel")
    });
    let plain_text = captured(&plain).map(|capture| capture.text.clone());
    Ok(CaseResult::new(
        "F10",
        password_ok && plain_ok,
        format!(
            "password page: {}; fields page: {} text {plain_text:?}",
            describe(&password),
            describe(&plain)
        ),
    ))
}

fn f11(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let (_, seen) = visit(browser, &server.sites.url(Role::Plain, "/form-card"))?;
    let passed = seen
        .iter()
        .any(|seen| matches!(seen, Seen::Discarded(DiscardReason::SignInOrPayment)))
        && captured(&seen).is_none();
    Ok(CaseResult::new("F11", passed, describe(&seen)))
}

fn f12(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for path in ["/pdf", "/image", "/text", "/missing", "/broken"] {
        let seen = visit_quietly(browser, &server.sites.url(Role::Plain, path))?;
        passed &= !requested(&seen);
        let summary = if seen.is_empty() {
            "no outcome".to_owned()
        } else {
            describe(&seen)
        };
        details.push(format!("{path}: {summary}"));
    }
    Ok(CaseResult::new("F12", passed, details.join("; ")))
}

/// F20 (added in review): a navigation that never commits must not read the excluded page
/// still on screen.
fn f20(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for path in ["/nocontent", "/resetcontent"] {
        let id = browser.open(&server.sites.url(Role::Bank, "/landing"), true)?;
        browser.wait_finals(id, 1, OUTCOME_TIMEOUT)?;
        let known = browser.log.outcomes(id).len();
        browser.navigate(id, &server.sites.url(Role::Plain, path))?;
        browser.pump_for(Duration::from_secs(5))?;
        let after = browser.log.outcomes(id)[known..].to_vec();
        let shown = browser.log.urls.get(&id).cloned().unwrap_or_default();
        browser.close(id)?;
        passed &= !requested(&after);
        let summary = if after.is_empty() {
            "no outcome".to_owned()
        } else {
            describe(&after)
        };
        details.push(format!("{path}: {summary}; tab shows {}", path_of(&shown)));
    }
    Ok(CaseResult::new("F20", passed, details.join("; ")))
}

/// F21: a bank page's own script sends it to a plain page; that page is not read.
fn f21(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let wanted = [Ineligible::LeftExcluded];
    let hop = Hop {
        name: "F21",
        start: (Role::Bank, "/signin/script"),
        reached: "/landing?via=script",
    };
    hop_case(browser, server, hop, &wanted)
}

/// F22: a bank page's auto-submitting form posts to a plain page; that page is not read.
fn f22(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let wanted = [Ineligible::LeftExcluded, Ineligible::FormPost];
    let hop = Hop {
        name: "F22",
        start: (Role::Bank, "/signin/form"),
        reached: "/posted",
    };
    hop_case(browser, server, hop, &wanted)
}

/// F24: a form POST on an ordinary site is skipped too.
fn f24(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let hop = Hop {
        name: "F24",
        start: (Role::Plain, "/signin/form"),
        reached: "/posted",
    };
    hop_case(browser, server, hop, &[Ineligible::FormPost])
}

/// A page that moves on by itself, and the plain-site path it should reach.
struct Hop {
    name: &'static str,
    start: (Role, &'static str),
    reached: &'static str,
}

fn hop_case(
    browser: &mut Browser,
    server: &Server,
    hop: Hop,
    wanted: &[Ineligible],
) -> Result<CaseResult, String> {
    let before = server.hits(Role::Plain, hop.reached)?;
    let id = browser.open(&server.sites.url(hop.start.0, hop.start.1), true)?;
    // The page leaves 300 ms after loading; allow the next page's settle delay and a margin.
    browser.pump_for(Duration::from_secs(6))?;
    let seen = browser.log.outcomes(id);
    let shown = browser.log.urls.get(&id).cloned().unwrap_or_default();
    browser.close(id)?;
    let reached = server.hits(Role::Plain, hop.reached)? > before;
    let passed = reached && !requested(&seen) && has_ineligible(&seen, wanted);
    Ok(CaseResult::new(
        hop.name,
        passed,
        format!(
            "{}; reached {}: {reached}; tab shows {}",
            describe(&seen),
            hop.reached,
            path_of(&shown)
        ),
    ))
}

/// F23: the owner follows a link out of a bank page (a synthetic Enter key on a focused
/// link). Only Syrinx's own navigations leave an excluded site cleanly, so it is not read.
fn f23(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let id = browser.open(&server.sites.url(Role::Bank, "/link-out"), true)?;
    browser.wait_finals(id, 1, OUTCOME_TIMEOUT)?;
    browser.send(BrowserCommand::Focus { id })?;
    browser.pump_for(Duration::from_millis(500))?;
    if let Err(error) = press_enter(browser) {
        browser.close(id)?;
        return Ok(CaseResult::new("F23", false, error));
    }
    let followed = browser.wait_finals(id, 2, OUTCOME_TIMEOUT);
    browser.pump_for(Duration::from_millis(300))?;
    let seen = browser.log.outcomes(id);
    let shown = browser.log.urls.get(&id).cloned().unwrap_or_default();
    browser.close(id)?;
    let reached = path_of(&shown).starts_with("landing?via=link");
    let passed = followed.is_ok()
        && reached
        && !requested(&seen)
        && has_ineligible(&seen, &[Ineligible::LeftExcluded]);
    Ok(CaseResult::new(
        "F23",
        passed,
        format!("{}; tab shows {}", describe(&seen), path_of(&shown)),
    ))
}

/// F25: a form `POST` answered by 302, 303 or 307, to the same site and to another; the
/// final page (a `GET` for 302 and 303) is still a form result.
fn f25(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for (code, target) in [
        ("302", Role::Plain),
        ("303", Role::Plain),
        ("307", Role::Plain),
        ("302", Role::Plain2),
        ("303", Role::Plain2),
        ("307", Role::Plain2),
    ] {
        let before = server.hits(target, "/landing")?;
        let path = format!("/postto/{code}/{}", target.name());
        let id = browser.open(&server.sites.url(Role::Plain, &path), true)?;
        browser.pump_for(Duration::from_secs(6))?;
        let seen = browser.log.outcomes(id);
        browser.close(id)?;
        let reached = server.hits(target, "/landing")? > before;
        passed &= reached && !requested(&seen) && has_ineligible(&seen, &[Ineligible::FormPost]);
        details.push(format!(
            "{code} to {}: {}; reached landing: {reached}",
            target.name(),
            describe(&seen)
        ));
    }
    Ok(CaseResult::new("F25", passed, details.join("; ")))
}

/// F26: the bank's script leaves for a relay page whose own script moves on again before
/// its settle delay; the page after the relay is skipped as tainted, and a Syrinx navigation
/// afterwards is read.
fn f26(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let id = browser.open(&server.sites.url(Role::Bank, "/signin/chain"), true)?;
    browser.pump_for(Duration::from_secs(7))?;
    let chain = browser.log.outcomes(id);
    let shown = browser.log.urls.get(&id).cloned().unwrap_or_default();
    let known = chain.len();
    browser.navigate(id, &server.sites.url(Role::Plain, "/landing?via=syrinx"))?;
    let read = browser.wait_finals(
        id,
        chain.iter().filter(|seen| seen.is_final()).count() + 1,
        OUTCOME_TIMEOUT,
    );
    browser.pump_for(Duration::from_millis(300))?;
    let after = browser.log.outcomes(id)[known..].to_vec();
    browser.close(id)?;
    let skipped = chain
        .iter()
        .filter(|seen| matches!(seen, Seen::Ineligible(Ineligible::LeftExcluded)))
        .count();
    let passed = skipped >= 1
        && !requested(&chain)
        && path_of(&shown).starts_with("landing?via=relay")
        && read.is_ok()
        && captured(&after).is_some();
    Ok(CaseResult::new(
        "F26",
        passed,
        format!(
            "chain: {}; tab shows {}; then Syrinx navigation: {}",
            describe(&chain),
            path_of(&shown),
            describe(&after)
        ),
    ))
}

/// F27: a popup opened from a bank page starts tainted. The same popup from a plain page is
/// not tainted (whatever else it is judged), so the taint comes from the opener.
fn f27(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut passed = true;
    let mut details = Vec::new();
    for (role, clean_opener) in [(Role::Bank, false), (Role::Plain, true)] {
        let opener = browser.open(&server.sites.url(role, "/popup-out"), true)?;
        browser.wait_finals(opener, 1, OUTCOME_TIMEOUT)?;
        browser.send(BrowserCommand::Focus { id: opener })?;
        browser.pump_for(Duration::from_millis(500))?;
        let popup = press_enter(browser).and_then(|()| adopt_popup(browser, opener));
        let popup = match popup {
            Ok(popup) => popup,
            Err(error) => {
                browser.close(opener)?;
                passed = false;
                details.push(format!("{}: {error}", role.name()));
                continue;
            }
        };
        let judged = browser.wait_finals(popup, 1, OUTCOME_TIMEOUT);
        browser.pump_for(Duration::from_millis(300))?;
        let seen = browser.log.outcomes(popup);
        browser.close(popup)?;
        browser.close(opener)?;
        let ok = if clean_opener {
            !seen.is_empty() && !has_ineligible(&seen, &[Ineligible::LeftExcluded])
        } else {
            !requested(&seen) && has_ineligible(&seen, &[Ineligible::LeftExcluded])
        };
        passed &= judged.is_ok() && ok;
        details.push(format!("from {}: {}", role.name(), describe(&seen)));
    }
    Ok(CaseResult::new("F27", passed, details.join("; ")))
}

/// F28: the bank's script reaches a plain page (tainted); a Syrinx navigation clears the
/// tab; Back into the tainted entry is still not read.
fn f28(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let id = browser.open(&server.sites.url(Role::Bank, "/signin/script"), true)?;
    browser.pump_for(Duration::from_secs(6))?;
    let finals = browser.log.finals(id);
    browser.navigate(id, &server.sites.url(Role::Plain, "/landing?via=syrinx"))?;
    let cleared = browser.wait_finals(id, finals + 1, OUTCOME_TIMEOUT);
    let known = browser.log.outcomes(id).len();
    browser.send(BrowserCommand::Back { id })?;
    browser.pump_for(Duration::from_secs(6))?;
    let back = browser.log.outcomes(id)[known..].to_vec();
    let shown = browser.log.urls.get(&id).cloned().unwrap_or_default();
    let all = browser.log.outcomes(id);
    browser.close(id)?;
    let passed = cleared.is_ok()
        && captured(&all[..known]).is_some()
        && path_of(&shown).starts_with("landing?via=script")
        && !requested(&back)
        && has_ineligible(&back, &[Ineligible::LeftExcluded]);
    Ok(CaseResult::new(
        "F28",
        passed,
        format!(
            "before Back: {}; after Back: {}; tab shows {}",
            describe(&all[..known]),
            describe(&back),
            path_of(&shown)
        ),
    ))
}

/// Waits for `opener`'s popup request and adopts it as a new shown tab, as Syrinx does.
fn adopt_popup(browser: &mut Browser, opener: u64) -> Result<u64, String> {
    let mut request = None;
    browser.wait("popup request", OUTCOME_TIMEOUT, |_, event| {
        if let Some(BrowserEvent::PopupRequested {
            id, request: asked, ..
        }) = event
            && *id == opener
        {
            request = Some(*asked);
        }
        request.is_some()
    })?;
    let request = request.ok_or("Synthetic popup request was lost.")?;
    let id = browser.next_id;
    browser.next_id += 1;
    browser.send(BrowserCommand::AdoptPopup { request, id })?;
    browser.send(BrowserCommand::ShowActive(Some(id)))?;
    Ok(id)
}

/// Posts Enter to the window that has keyboard focus inside the harness window.
fn press_enter(browser: &Browser) -> Result<(), String> {
    let child = unsafe { GetWindow(browser.parent, GW_CHILD) }
        .map_err(|_| "Synthetic page window is missing.".to_owned())?;
    let thread = unsafe { GetWindowThreadProcessId(child, None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread, &mut info) }
        .map_err(|_| "Synthetic focus query failed.".to_owned())?;
    if !unsafe { IsChild(browser.parent, info.hwndFocus) }.as_bool() {
        return Err("Synthetic key target is outside the harness window.".into());
    }
    for (message, flags) in [(WM_KEYDOWN, 1), (WM_KEYUP, 0xC000_0001)] {
        unsafe { PostMessageW(Some(info.hwndFocus), message, WPARAM(0x0D), LPARAM(flags)) }
            .map_err(|_| "Synthetic Enter key failed.".to_owned())?;
    }
    Ok(())
}

fn f13(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let id = browser.open(&server.sites.url(Role::Plain, "/history"), true)?;
    browser.wait_finals(id, 1, OUTCOME_TIMEOUT)?;
    browser.wait_title(id, "pushed", Duration::from_secs(10))?;
    // The interval starts at the first capture; allow it, the settle delay and a margin.
    let second = browser.wait_finals(id, 2, Duration::from_secs(40));
    browser.pump_for(Duration::from_secs(3))?;
    let timed = browser.log.text.get(&id).cloned().unwrap_or_default();
    browser.close(id)?;
    let requests: Vec<(Instant, String)> = timed
        .iter()
        .filter_map(|(at, seen)| match seen {
            Seen::Requested(address) => Some((*at, address.clone())),
            _ => None,
        })
        .collect();
    let gap = match requests.as_slice() {
        [first, second, ..] => Some(second.0.duration_since(first.0)),
        _ => None,
    };
    let passed = second.is_ok()
        && requests.len() == 2
        && requests[1].1.ends_with("/history/5")
        && gap.is_some_and(|gap| gap >= Duration::from_millis(29_500));
    let seen: Vec<Seen> = timed.into_iter().map(|(_, seen)| seen).collect();
    Ok(CaseResult::new(
        "F13",
        passed,
        format!(
            "{} requests, gap {gap:?}; {}",
            requests.len(),
            describe(&seen)
        ),
    ))
}

fn f15(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let probe = server.sites.url(Role::Probe, "/probe");
    let captured_tab = browser.open(&server.sites.url(Role::Plain, "/f1"), true)?;
    browser.wait_finals(captured_tab, 1, OUTCOME_TIMEOUT)?;
    let captured_first = captured(&browser.log.outcomes(captured_tab)).is_some();
    browser.navigate(captured_tab, &probe)?;
    let after_capture = browser.wait_title(captured_tab, "probe ", Duration::from_secs(15))?;
    let fresh_tab = browser.open(&probe, true)?;
    let fresh = browser.wait_title(fresh_tab, "probe ", Duration::from_secs(15))?;
    browser.pump_for(Duration::from_secs(3))?;
    let probe_calls = browser
        .log
        .requests(captured_tab)
        .iter()
        .chain(browser.log.requests(fresh_tab).iter())
        .filter(|address| address.starts_with(server.sites.base(Role::Probe)))
        .count();
    browser.close(captured_tab)?;
    browser.close(fresh_tab)?;
    let passed = captured_first && after_capture == fresh && probe_calls == 0;
    Ok(CaseResult::new(
        "F15",
        passed,
        format!(
            "captured first: {captured_first}; after capture: {after_capture:?}; fresh: {fresh:?}; calls to probe: {probe_calls}"
        ),
    ))
}

fn f16(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let mut details = Vec::new();
    let mut ten_thousand_ok = false;
    for count in [1_000, 10_000, 100_000] {
        let id = browser.open(
            &server.sites.url(Role::Plain, &format!("/big/{count}")),
            true,
        )?;
        browser.wait(
            &format!("big page {count} load"),
            Duration::from_secs(60),
            |log, _| {
                log.completed
                    .get(&id)
                    .is_some_and(|completed| *completed > 0)
            },
        )?;
        browser.pump_for(Duration::from_millis(1_500))?;
        let before = browser.log.titles.get(&id).cloned();
        browser.wait_finals(id, 1, Duration::from_secs(90))?;
        browser.pump_for(Duration::from_secs(1))?;
        let after = browser.log.titles.get(&id).cloned();
        let seen = browser.log.outcomes(id);
        browser.close(id)?;
        let measured = captured(&seen).map_or_else(
            || describe(&seen),
            |capture| {
                format!(
                    "{} B response, call {:?}, parse {:?}, {} chars",
                    capture.response_bytes,
                    capture.call_time,
                    capture.parse_time,
                    capture.text.chars().count()
                )
            },
        );
        if count == 10_000 {
            ten_thousand_ok = captured(&seen).is_some() && before == after;
        }
        details.push(format!(
            "{count} nodes: {measured}; page title before {before:?}, after {after:?}"
        ));
    }
    Ok(CaseResult::new("F16", ten_thousand_ok, details.join("; ")))
}

/// F14 with the response subscription: service worker, back/forward cache and HTTP cache.
fn f14_on(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let worker = service_worker(browser, server)?;
    let caches = cache_sequence(browser, server, "on", true)?;
    Ok(CaseResult::recorded(
        "F14 (subscribed)",
        format!("service worker: {worker}; {caches}"),
    ))
}

fn service_worker(browser: &mut Browser, server: &Server) -> Result<String, String> {
    let id = browser.open(&server.sites.url(Role::Plain, "/sw-page"), true)?;
    let controlled = browser.wait_title(id, "sw-controlled", Duration::from_secs(15));
    browser.wait_finals(id, 1, OUTCOME_TIMEOUT)?;
    browser.navigate(id, &server.sites.url(Role::Plain, "/sw-synth"))?;
    let synthetic = browser.wait_finals(id, 2, OUTCOME_TIMEOUT);
    browser.pump_for(Duration::from_millis(300))?;
    let seen = browser.log.outcomes(id);
    let network_hits = server.hits(Role::Plain, "/sw-synth")?;
    browser.close(id)?;
    Ok(format!(
        "controlled {:?}; synthetic page outcome {:?}; network hits for it {network_hits}; {}",
        controlled.is_ok(),
        synthetic.is_ok(),
        describe(&seen)
    ))
}

/// Visits a cacheable page, leaves, returns by Back, navigates to it again and reloads;
/// reports each step's outcome and how often the server was asked.
fn cache_sequence(
    browser: &mut Browser,
    server: &Server,
    tag: &str,
    capture_on: bool,
) -> Result<String, String> {
    let landing = format!("/landing?b={tag}");
    let cacheable = format!("/cacheable?c={tag}");
    let id = browser.open(&server.sites.url(Role::Plain, &landing), true)?;
    let mut steps = vec![("first visit", step(browser, id, capture_on)?)];
    browser.navigate(id, &server.sites.url(Role::Plain, &cacheable))?;
    steps.push(("cacheable", step(browser, id, capture_on)?));
    browser.send(BrowserCommand::Back { id })?;
    steps.push(("back", step(browser, id, capture_on)?));
    browser.navigate(id, &server.sites.url(Role::Plain, &cacheable))?;
    steps.push(("cacheable again", step(browser, id, capture_on)?));
    browser.send(BrowserCommand::Reload { id })?;
    steps.push(("reload", step(browser, id, capture_on)?));
    browser.close(id)?;
    let hits = format!(
        "server hits: landing {}, cacheable {}",
        server.hits(Role::Plain, &landing)?,
        server.hits(Role::Plain, &cacheable)?
    );
    let steps: Vec<String> = steps
        .into_iter()
        .map(|(name, outcome)| format!("{name}: {outcome}"))
        .collect();
    Ok(format!("{}; {hits}", steps.join(", ")))
}

/// Waits for the next load (and, with capture on, its outcome); returns the new outcomes.
fn step(browser: &mut Browser, id: u64, capture_on: bool) -> Result<String, String> {
    let completed = browser.log.completed.get(&id).copied().unwrap_or(0);
    let known = browser.log.outcomes(id).len();
    browser.wait(&format!("tab {id} load"), OUTCOME_TIMEOUT, |log, _| {
        log.completed.get(&id).is_some_and(|now| *now > completed)
    })?;
    if capture_on {
        let finals = browser.log.finals(id);
        if browser
            .wait_finals(id, finals + 1, Duration::from_secs(6))
            .is_err()
        {
            return Ok("no outcome".into());
        }
    } else {
        browser.pump_for(Duration::from_millis(2_500))?;
    }
    let seen = browser.log.outcomes(id);
    Ok(describe(&seen[known..]))
}

fn f18(browser: &mut Browser, before: &str) -> Result<CaseResult, String> {
    let after = listening_ports(browser)?;
    // Zero processes would make the port check vacuous, so it fails the case.
    let checked =
        |query: &str| !query.starts_with("processes=0 ") && query.ends_with("listening=0");
    let passed = checked(before) && checked(&after);
    Ok(CaseResult::new(
        "F18",
        passed,
        format!(
            "host call works with devtools disabled (see F1); before captures: {before}; after: {after}; F12 key not exercised"
        ),
    ))
}

/// Counts TCP listening sockets owned by this run's WebView2 processes, found by profile path.
fn listening_ports(browser: &Browser) -> Result<String, String> {
    let profile = browser.profile.display().to_string().replace('\'', "''");
    let script = format!(
        "$p=@(Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'msedgewebview2.exe' -and \
         [string]$_.CommandLine -like '*{profile}*' }} | ForEach-Object {{ $_.ProcessId }}); \
         $l=@(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object {{ $p -contains $_.OwningProcess }}); \
         'processes=' + $p.Count + ' listening=' + $l.Count"
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|error| super::server::io_error("run listening-port query", error))?;
    if !output.status.success() {
        return Err(format!(
            "Listening-port query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

// ---- Race run (F8) --------------------------------------------------------------------

fn race_run(
    options: &Options,
    server: &Server,
    results: &mut Vec<CaseResult>,
) -> Result<(), String> {
    let policy = synthetic_policy(Duration::from_millis(100))?;
    with_browser("race run", Some(policy), results, |browser, results| {
        results.push(f8(browser, server, options.race_runs)?);
        Ok(())
    })
}

/// Tallies, over many runs, what happened to a capture raced by a navigation to the bank.
#[derive(Default)]
struct RaceTally {
    captured_plain: u32,
    discarded_mismatch: u32,
    superseded: u32,
    cancelled: u32,
    bank_requests: u32,
    bank_text: u32,
    other: u32,
}

fn f8(browser: &mut Browser, server: &Server, runs: u32) -> Result<CaseResult, String> {
    let bank = server.sites.base(Role::Bank).to_owned();
    let mut tally = RaceTally::default();
    let mut seed: u32 = 0x2545_f491;
    let mut tab = None;
    for _ in 0..runs {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let delay = 50 + (seed >> 16) % 200;
        let url = server.sites.url(Role::Plain, &format!("/race/{delay}"));
        let id = match tab {
            Some(id) => {
                browser.navigate(id, &url)?;
                id
            }
            None => browser.open(&url, true)?,
        };
        tab = Some(id);
        let known = browser.log.outcomes(id).len();
        browser.wait("race run to reach the bank", OUTCOME_TIMEOUT, |log, _| {
            log.outcomes(id)[known..]
                .iter()
                .any(|seen| matches!(seen, Seen::Ineligible(Ineligible::BankingOrPayment)))
        })?;
        browser.pump_for(Duration::from_millis(150))?;
        tally_run(&mut tally, &browser.log.outcomes(id)[known..], &bank);
    }
    if let Some(id) = tab {
        browser.close(id)?;
    }
    let passed = tally.bank_requests == 0 && tally.bank_text == 0;
    Ok(CaseResult::new(
        "F8",
        passed,
        format!(
            "{runs} runs: captured race page {}, discarded at step 9 {}, superseded in flight {}, \
             cancelled before dispatch {}, other {}, bank calls {}, bank text stored {}",
            tally.captured_plain,
            tally.discarded_mismatch,
            tally.superseded,
            tally.cancelled,
            tally.other,
            tally.bank_requests,
            tally.bank_text
        ),
    ))
}

fn tally_run(tally: &mut RaceTally, seen: &[Seen], bank: &str) {
    for seen in seen {
        match seen {
            Seen::Requested(address) if address.starts_with(bank) => tally.bank_requests += 1,
            Seen::Captured(capture)
                if capture.address.starts_with(bank)
                    || capture.text.contains("landing-sentinel-bank") =>
            {
                tally.bank_text += 1
            }
            Seen::Captured(_) => tally.captured_plain += 1,
            Seen::Discarded(DiscardReason::AddressMismatch) => tally.discarded_mismatch += 1,
            Seen::Superseded => tally.superseded += 1,
            Seen::Cancelled => tally.cancelled += 1,
            Seen::Requested(_) | Seen::Ineligible(Ineligible::BankingOrPayment) => {}
            _ => tally.other += 1,
        }
    }
}

// ---- Suspension run (F17) -------------------------------------------------------------

fn suspension_run(server: &Server, results: &mut Vec<CaseResult>) -> Result<(), String> {
    // Settle past the five-second hidden-tab grace, so the capture falls due while suspended.
    let policy = synthetic_policy(Duration::from_secs(7))?;
    with_browser(
        "suspension run",
        Some(policy),
        results,
        |browser, results| {
            results.push(f17(browser, server)?);
            Ok(())
        },
    )
}

fn f17(browser: &mut Browser, server: &Server) -> Result<CaseResult, String> {
    let id = browser.open(&server.sites.url(Role::Plain, "/landing"), false)?;
    browser.wait(
        "hidden tab suspension",
        Duration::from_secs(20),
        |log, _| log.suspended.get(&id).copied() == Some(true),
    )?;
    // The capture fell due at seven seconds; give it time to (wrongly) run.
    browser.pump_for(Duration::from_secs(5))?;
    let while_suspended = browser.log.outcomes(id);
    browser.send(BrowserCommand::ShowActive(Some(id)))?;
    browser.wait_finals(id, 1, OUTCOME_TIMEOUT)?;
    browser.pump_for(Duration::from_millis(300))?;
    let after = browser.log.outcomes(id);
    browser.close(id)?;
    let passed = while_suspended.is_empty() && captured(&after).is_some();
    Ok(CaseResult::new(
        "F17",
        passed,
        format!(
            "while suspended: {}; after showing: {}",
            if while_suspended.is_empty() {
                "nothing".to_owned()
            } else {
                describe(&while_suspended)
            },
            describe(&after)
        ),
    ))
}

// ---- Capture-off run (F19 and F14's unsubscribed half) --------------------------------

fn off_run(
    options: &Options,
    server: &Server,
    results: &mut Vec<CaseResult>,
) -> Result<(), String> {
    with_browser("capture-off run", None, results, |browser, results| {
        if options.wants("F14") {
            let caches = cache_sequence(browser, server, "off", false)?;
            results.push(CaseResult::recorded("F14 (not subscribed)", caches));
        }
        if options.wants("F19") {
            let id = browser.open(&server.sites.url(Role::Plain, "/f1"), true)?;
            browser.pump_for(Duration::from_secs(5))?;
            browser.close(id)?;
            let events = browser.log.page_text_events;
            results.push(CaseResult::new(
                "F19",
                events == 0,
                format!("{events} page-text events over the whole capture-off run"),
            ));
        }
        Ok(())
    })
}
