//! Throwaway P1-06 harness: counts native paints and GPUI renders to compare idle repainting.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, WindowBounds, WindowHandle, WindowOptions, div, px, rgb, size,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, MSG, PM_REMOVE, SW_MINIMIZE, SW_RESTORE, SWP_NOMOVE, SWP_NOZORDER,
        SetWindowPos, SetWindowsHookExW, ShowWindow, WH_GETMESSAGE, WM_PAINT,
    },
};

thread_local! {
    static PAINTS: RefCell<HashMap<isize, u64>> = RefCell::new(HashMap::new());
}

// Count WM_PAINT messages removed from this thread's queue, per window.
unsafe extern "system" fn count_paints(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 == PM_REMOVE.0 as usize {
        let msg = unsafe { &*(lparam.0 as *const MSG) };
        if msg.message == WM_PAINT {
            PAINTS.with(|paints| *paints.borrow_mut().entry(msg.hwnd.0 as isize).or_default() += 1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn paints(hwnd: HWND) -> u64 {
    PAINTS.with(|paints| paints.borrow().get(&(hwnd.0 as isize)).copied().unwrap_or(0))
}

/// A static view that counts its renders and can ask for follow-up frames.
struct Probe {
    renders: Rc<Cell<u64>>,
    animate_remaining: u32,
    follow_up_from_render: bool,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        if self.animate_remaining > 0 {
            self.animate_remaining -= 1;
            window.request_animation_frame();
        }
        if self.follow_up_from_render {
            // Demand raised while drawing must survive the frame that raised it.
            self.follow_up_from_render = false;
            let entity = cx.entity();
            window.on_next_frame(move |_, cx| entity.update(cx, |_, cx| cx.notify()));
        }
        div()
            .size_full()
            .bg(rgb(0x202020))
            .text_color(rgb(0xe0e0e0))
            .child(format!("P1-06 idle probe, render {}", self.renders.get()))
    }
}

struct ProbeWindow {
    handle: WindowHandle<Probe>,
    view: Entity<Probe>,
    renders: Rc<Cell<u64>>,
    hwnd: HWND,
}

fn open_probe(cx: &mut App, x: f32) -> ProbeWindow {
    let renders = Rc::new(Cell::new(0));
    let probe_renders = renders.clone();
    let bounds = Bounds::new(gpui::point(px(x), px(80.)), size(px(420.), px(260.)));
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| Probe {
                    renders: probe_renders,
                    animate_remaining: 0,
                    follow_up_from_render: false,
                })
            },
        )
        .expect("probe window opens");
    let view = handle.update(cx, |_, _, cx| cx.entity()).expect("probe view");
    let hwnd = handle
        .update(cx, |_, window, _| match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => HWND(handle.hwnd.get() as *mut _),
            _ => panic!("probe window has no Win32 handle"),
        })
        .expect("probe hwnd");
    ProbeWindow {
        handle,
        view,
        renders,
        hwnd,
    }
}

async fn sleep(cx: &AsyncApp, duration: Duration) {
    cx.background_executor().timer(duration).await;
}

/// Waits until `done` holds, polling, and reports how long it took.
async fn wait_for(cx: &AsyncApp, limit: Duration, done: impl Fn() -> bool) -> Option<Duration> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return Some(start.elapsed());
        }
        sleep(cx, Duration::from_millis(5)).await;
    }
    done().then(|| start.elapsed())
}

/// Native paints and GPUI renders for one window over a quiet interval.
async fn idle_rate(cx: &AsyncApp, window: &ProbeWindow, seconds: f64) -> (f64, u64) {
    let paints_before = paints(window.hwnd);
    let renders_before = window.renders.get();
    sleep(cx, Duration::from_secs_f64(seconds)).await;
    let paint_rate = (paints(window.hwnd) - paints_before) as f64 / seconds;
    (paint_rate, window.renders.get() - renders_before)
}

struct Report {
    failures: u32,
}

impl Report {
    fn check(&mut self, name: &str, passed: bool, detail: String) {
        println!("{} {name}: {detail}", if passed { "PASS" } else { "FAIL" });
        if !passed {
            self.failures += 1;
        }
    }
}

fn notify(cx: &mut AsyncApp, window: &ProbeWindow) {
    window
        .view
        .update(cx, |_, cx| cx.notify())
        ;
}

async fn run_scenarios(cx: &mut AsyncApp, a: ProbeWindow, b: ProbeWindow, expect_idle: bool) -> u32 {
    let mut report = Report { failures: 0 };
    let quick = Duration::from_millis(500);

    // 1. Initial paint of both windows.
    let first = wait_for(cx, Duration::from_secs(3), || a.renders.get() > 0 && b.renders.get() > 0).await;
    report.check("initial paint (two windows)", first.is_some(), format!("{first:?}"));
    sleep(cx, Duration::from_secs(2)).await;

    // 2. Idle paint rate: the measurement that distinguishes the builds.
    let (rate_a, renders_a) = idle_rate(cx, &a, 3.0).await;
    let rate_b = paints(b.hwnd);
    println!("INFO idle window A: {rate_a:.1} WM_PAINT/s, {renders_a} GPUI renders over 3 s");
    let _ = rate_b;
    if expect_idle {
        report.check("idle paints below 1/s", rate_a < 1.0, format!("{rate_a:.1}/s"));
    }
    report.check("no idle GPUI renders", renders_a == 0, format!("{renders_a}"));

    // 3. Notify after idle renders only the notified window.
    let (a0, b0) = (a.renders.get(), b.renders.get());
    notify(cx, &a);
    let took = wait_for(cx, quick, || a.renders.get() > a0).await;
    sleep(cx, Duration::from_millis(100)).await;
    report.check(
        "idle-to-notify renders A only",
        took.is_some() && b.renders.get() == b0,
        format!("A after {took:?}, B renders +{}", b.renders.get() - b0),
    );

    // 4. Callback-only demand: a next-frame callback without a dirty view.
    let ran = Rc::new(Cell::new(false));
    let flag = ran.clone();
    a.handle
        .update(cx, |_, window, _| window.on_next_frame(move |_, _| flag.set(true)))
        .expect("schedule callback");
    let took = wait_for(cx, quick, || ran.get()).await;
    report.check("next-frame callback runs while idle", took.is_some(), format!("{took:?}"));

    // 5. Demand raised during drawing survives that frame.
    let start = a.renders.get();
    a.view
        .update(cx, |probe, cx| {
            probe.follow_up_from_render = true;
            cx.notify();
        })
        ;
    let took = wait_for(cx, quick, || a.renders.get() >= start + 2).await;
    report.check("demand raised mid-frame gets a frame", took.is_some(), format!("{took:?}"));

    // 6. Animation runs to completion, then returns to idle.
    let start = a.renders.get();
    a.view
        .update(cx, |probe, cx| {
            probe.animate_remaining = 60;
            cx.notify();
        })
        ;
    let took = wait_for(cx, Duration::from_secs(5), || a.renders.get() >= start + 61).await;
    report.check("60-frame animation completes", took.is_some(), format!("{took:?}"));
    sleep(cx, Duration::from_millis(300)).await;
    let (rate, renders) = idle_rate(cx, &a, 1.5).await;
    report.check("animation returns to no renders", renders == 0, format!("{renders} renders"));
    if expect_idle {
        report.check("animation returns to idle paints", rate < 1.0, format!("{rate:.1}/s"));
    }

    // 7. Notify the second window while the first stays idle.
    let (a0, b0) = (a.renders.get(), b.renders.get());
    notify(cx, &b);
    let took = wait_for(cx, quick, || b.renders.get() > b0).await;
    report.check(
        "second window notify",
        took.is_some() && a.renders.get() == a0,
        format!("B after {took:?}, A renders +{}", a.renders.get() - a0),
    );

    // 8. Work requested while minimized is delivered after restore.
    unsafe {
        let _ = ShowWindow(a.hwnd, SW_MINIMIZE);
    }
    sleep(cx, Duration::from_millis(400)).await;
    let a0 = a.renders.get();
    notify(cx, &a);
    sleep(cx, Duration::from_millis(400)).await;
    let while_minimized = a.renders.get() - a0;
    unsafe {
        let _ = ShowWindow(a.hwnd, SW_RESTORE);
    }
    let took = wait_for(cx, Duration::from_secs(2), || a.renders.get() > a0).await;
    report.check(
        "minimize/restore delivers pending work",
        took.is_some(),
        format!("{while_minimized} renders while minimized, restored after {took:?}"),
    );
    sleep(cx, Duration::from_millis(500)).await;

    // 9. A native resize produces a new frame.
    let a0 = a.renders.get();
    unsafe {
        SetWindowPos(a.hwnd, None, 0, 0, 520, 320, SWP_NOMOVE | SWP_NOZORDER).expect("resize");
    }
    let took = wait_for(cx, quick, || a.renders.get() > a0).await;
    report.check("native resize renders", took.is_some(), format!("{took:?}"));

    // 10. Final idle state.
    sleep(cx, Duration::from_millis(500)).await;
    let (rate, renders) = idle_rate(cx, &a, 2.0).await;
    println!("INFO final idle window A: {rate:.1} WM_PAINT/s, {renders} renders");
    report.check("final idle without renders", renders == 0, format!("{renders}"));
    if expect_idle {
        report.check("final idle paints below 1/s", rate < 1.0, format!("{rate:.1}/s"));
    }
    report.failures
}

async fn run_idle(cx: &mut AsyncApp, a: ProbeWindow, seconds: f64) {
    sleep(cx, Duration::from_secs(3)).await;
    let (rate, renders) = idle_rate(cx, &a, seconds).await;
    println!("IDLE {seconds} s: {rate:.2} WM_PAINT/s, {renders} GPUI renders");
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mode = arguments.first().cloned().unwrap_or_default();
    let seconds: f64 = arguments.get(1).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let expect_idle = arguments.iter().any(|a| a == "--expect-idle");

    let hook: HHOOK = unsafe {
        SetWindowsHookExW(WH_GETMESSAGE, Some(count_paints), None, GetCurrentThreadId())
            .expect("message hook installs")
    };
    let _ = hook;
    let failures = Rc::new(Cell::new(0));
    let result = failures.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        let a = open_probe(cx, 80.);
        let mode = mode.clone();
        let b = (mode == "scenarios").then(|| open_probe(cx, 540.));
        cx.spawn(async move |cx| {
            match b {
                Some(b) => result.set(run_scenarios(cx, a, b, expect_idle).await),
                None => run_idle(cx, a, seconds).await,
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    println!("FAILURES {}", failures.get());
    std::process::exit(if failures.get() == 0 { 0 } else { 1 });
}
