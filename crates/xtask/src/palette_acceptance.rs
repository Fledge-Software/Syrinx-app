//! Runs P1-33's palette acceptance in the real app against synthetic loopback pages.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// The only pages the server answers; each reports its own focus in its title.
const LABELS: [&str; 2] = ["focus-page", "enter-target"];
/// A connection that sends nothing, such as a speculative preconnect, is dropped after this.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_LIMIT: usize = 8 * 1024;

pub fn run(root: &Path) -> Result<(), String> {
    // Build first, so the run never races a build of the same executable.
    super::run_cargo(
        root,
        &[
            "build",
            "--package",
            "syrinx-app",
            "--profile",
            "local",
            "--offline",
            "--locked",
        ],
    )?;
    let output = root.join("target").join("p1-33");
    fs::create_dir_all(&output)
        .map_err(|error| format!("Cannot create {}: {error}", output.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "The system clock is unavailable.".to_owned())?
        .as_secs();
    let report = output.join(format!("palette-acceptance-{stamp}.json"));
    let summary = output.join(format!("palette-acceptance-{stamp}-summary.md"));

    let base = serve_pages()?;
    let status = launch(root, &report, &base)?;
    let text = fs::read_to_string(&report)
        .map_err(|error| format!("The app wrote no report ({status}): {error}"))?;
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|error| format!("The acceptance report is not valid JSON: {error}"))?;
    let rendered = render_summary(&parsed);
    fs::write(&summary, &rendered)
        .map_err(|error| format!("Cannot write {}: {error}", summary.display()))?;
    println!("{rendered}");
    println!(
        "Report: {}\nSummary: {}",
        report.display(),
        summary.display()
    );
    if !status.success() || parsed["passed"] != Value::Bool(true) {
        return Err(format!("Palette acceptance failed ({status})."));
    }
    Ok(())
}

/// Starts the visible app on a fresh synthetic profile; a hidden launch cannot test focus.
fn launch(root: &Path, report: &Path, base: &str) -> Result<std::process::ExitStatus, String> {
    let executable: PathBuf = root.join("target").join("local").join("syrinx.exe");
    Command::new(&executable)
        .arg("--test-palette-acceptance")
        .arg(report)
        .arg(base)
        .current_dir(root)
        .status()
        .map_err(|error| format!("Could not start {}: {error}", executable.display()))
}

/// Serves the synthetic pages on an ephemeral loopback port for the life of this process.
fn serve_pages() -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("Cannot open the synthetic page server: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("Cannot read the synthetic page port: {error}"))?
        .port();
    thread::Builder::new()
        .name("p1-33-pages".to_owned())
        .spawn(move || accept(listener))
        .map_err(|error| format!("Cannot start the synthetic page server: {error}"))?;
    Ok(format!("http://127.0.0.1:{port}"))
}

fn accept(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // One thread per connection, so an idle preconnect cannot stall the pages.
                let spawned = thread::Builder::new()
                    .name("p1-33-page".to_owned())
                    .spawn(move || answer(stream));
                if let Err(error) = spawned {
                    eprintln!("Synthetic page connection not served: {error}");
                }
            }
            Err(error) => eprintln!("Synthetic page server accept failed: {error}"),
        }
    }
}

fn answer(mut stream: TcpStream) {
    if let Err(error) = respond(&mut stream) {
        eprintln!("Synthetic page request failed: {error}");
    }
}

fn respond(stream: &mut TcpStream) -> std::io::Result<()> {
    // An accepted socket inherits the listener's mode; make it blocking before timed reads.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let request = read_request(stream)?;
    let response = match requested_label(&request) {
        Some(label) => {
            let body = page(label);
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Cache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }
        None => {
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
        }
    };
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|end| end == b"\r\n\r\n") && request.len() < REQUEST_LIMIT {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    Ok(request)
}

/// `GET /page/<label>` for one of the fixed labels; anything else is not found.
fn requested_label(request: &[u8]) -> Option<&'static str> {
    LABELS.into_iter().find(|label| {
        let line = format!("GET /page/{label} ");
        request.starts_with(line.as_bytes())
    })
}

/// The page's own script writes `document.hasFocus()` into its title; Syrinx injects nothing.
fn page(label: &str) -> String {
    format!(
        r#"<!doctype html><meta charset=utf-8><title>syrinx-p133-{label} focus:pending</title>
<link rel=icon href=data:,><h1>Synthetic P1-33 page: {label}</h1>
<input aria-label="Synthetic input"><script>
let last = '';
const report = () => {{
  const state = document.hasFocus() ? 'yes' : 'no';
  if (state !== last) {{ last = state; document.title = 'syrinx-p133-{label} focus:' + state; }}
}};
addEventListener('focus', report); addEventListener('blur', report);
setInterval(report, 50); report();
</script>"#
    )
}

/// A readable summary of the report's verdicts and percentiles.
fn render_summary(report: &Value) -> String {
    let display = &report["display"];
    let source = if display["measured"] == Value::Bool(true) {
        "measured"
    } else {
        "assumed 60 Hz"
    };
    let mut lines = vec![
        "# P1-33 palette acceptance".to_owned(),
        String::new(),
        format!(
            "Overall: **{}**. Refresh interval {} ms ({source}).",
            verdict(&report["passed"]),
            milliseconds(&display["interval_ms"])
        ),
    ];
    if let Some(problem) = display["problem"].as_str() {
        lines.push(format!("\nThe refresh interval was not read: {problem}"));
    }
    if let Some(aborted) = report["aborted"].as_str() {
        lines.push(format!("\nThe run stopped early: {aborted}"));
    }
    opening_lines(&mut lines, &report["open"]);
    for size in report["ranking"].as_array().into_iter().flatten() {
        ranking_lines(&mut lines, size);
    }
    lines.push("\n## Focus and stacking checks\n".to_owned());
    for check in report["checks"].as_array().into_iter().flatten() {
        lines.push(format!(
            "- {} **{}**: {}",
            text(&check["name"]),
            verdict(&check["passed"]),
            text(&check["detail"])
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn opening_lines(lines: &mut Vec<String>, open: &Value) {
    lines.push("\n## Opening (Ctrl K action to first presented frame)\n".to_owned());
    lines.push(TABLE_HEADER.to_owned());
    lines.push(stats_row("show requested", &open["show_requested"]));
    lines.push(stats_row("show started", &open["show_started"]));
    lines.push(stats_row("first render", &open["first_render"]));
    lines.push(stats_row("presented", &open["presented"]));
    lines.push(format!(
        "\nBudget: {} ({} ms): **{}**.",
        text(&open["budget"]),
        milliseconds(&open["budget_ms"]),
        verdict(&open["passed"])
    ));
}

fn ranking_lines(lines: &mut Vec<String>, size: &Value) {
    lines.push(format!(
        "\n## Ranking with {} synthetic rows ({} in the catalogue)\n",
        size["synthetic_rows"], size["catalogue_rows"]
    ));
    lines.push(TABLE_HEADER.to_owned());
    lines.push(stats_row("worker (D48)", &size["worker"]));
    lines.push(stats_row("query round trip", &size["round_trip"]));
    lines.push(stats_row("key to echo frame", &size["echo"]));
    lines.push(stats_row("key to rows frame", &size["composed"]));
    lines.push(format!(
        "\nKeystrokes {}, timeouts {}, superseded {}. D48 (5 ms p95, worker): **{}**. \
         Composed budget ({} ms at p95): **{}**.",
        size["keystrokes"],
        size["timeouts"],
        size["superseded"],
        verdict(&size["d48_passed"]),
        milliseconds(&size["composed_budget_ms"]),
        verdict(&size["composed_passed"])
    ));
    let burst = &size["burst"];
    lines.push(format!(
        "\nBack-to-back typing at one key per {} ms ({} words, no budget): last key to rows \
         p50 {} ms, p95 {} ms, max {} ms; {} of {} requests superseded; {} words without \
         final rows.",
        milliseconds(&burst["cadence_ms"]),
        burst["words"],
        milliseconds(&burst["final_rows"]["p50_ms"]),
        milliseconds(&burst["final_rows"]["p95_ms"]),
        milliseconds(&burst["final_rows"]["max_ms"]),
        burst["superseded"],
        burst["queries"],
        burst["missing_final_rows"]
    ));
}

const TABLE_HEADER: &str = "| stage | n | p50 ms | p95 ms | max ms |\n|---|---|---|---|---|";

fn stats_row(label: &str, stats: &Value) -> String {
    if stats.is_null() {
        return format!("| {label} | 0 | – | – | – |");
    }
    format!(
        "| {label} | {} | {} | {} | {} |",
        stats["count"],
        milliseconds(&stats["p50_ms"]),
        milliseconds(&stats["p95_ms"]),
        milliseconds(&stats["max_ms"])
    )
}

/// A number to three decimals, or `?` when the report lacks it.
fn milliseconds(value: &Value) -> String {
    value
        .as_f64()
        .map_or_else(|| "?".to_owned(), |value| format!("{value:.3}"))
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("?")
}

fn verdict(value: &Value) -> &'static str {
    match value {
        Value::Bool(true) => "pass",
        Value::Bool(false) => "FAIL",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_fixed_synthetic_pages_are_served() {
        assert_eq!(
            requested_label(b"GET /page/focus-page HTTP/1.1\r\n\r\n"),
            Some("focus-page")
        );
        assert_eq!(
            requested_label(b"GET /page/enter-target HTTP/1.1\r\n\r\n"),
            Some("enter-target")
        );
        for request in [
            &b"GET /page/focus-pages HTTP/1.1\r\n\r\n"[..],
            b"GET /favicon.ico HTTP/1.1\r\n\r\n",
            b"POST /page/focus-page HTTP/1.1\r\n\r\n",
            b"",
        ] {
            assert_eq!(requested_label(request), None);
        }
    }

    #[test]
    fn the_page_reports_focus_only_through_its_own_title() {
        let html = page("focus-page");
        assert!(html.contains("document.hasFocus()"));
        assert!(html.contains("syrinx-p133-focus-page focus:"));
        assert!(!html.contains("http"), "the page loads nothing else");
    }

    #[test]
    fn the_summary_marks_failures_and_missing_values() {
        let report = serde_json::json!({
            "passed": false,
            "aborted": "stopped",
            "display": {"interval_ms": 16.667, "measured": true},
            "open": {"budget": "one frame", "budget_ms": 16.667, "passed": false,
                     "presented": {"count": 2, "p50_ms": 20.0, "p95_ms": 30.0, "max_ms": 30.0},
                     "first_render": null},
            "ranking": [],
            "checks": [{"name": "popup_not_topmost", "passed": true, "detail": "ok"}]
        });
        let text = render_summary(&report);
        assert!(text.contains("Overall: **FAIL**"));
        assert!(text.contains("The run stopped early: stopped"));
        assert!(text.contains("| presented | 2 | 20.000 | 30.000 | 30.000 |"));
        assert!(text.contains("| first render | 0 |"));
        assert!(text.contains("- popup_not_topmost **pass**: ok"));
    }
}
