//! The synthetic pages for P1-25's F-tests; their own scripts report only through the title.

use super::server::{Reply, Role, Sites};

/// The `Cache-Control` spellings F2 serves, one header list per variant.
pub const NO_STORE_VARIANTS: &[&[&str]] = &[
    &["no-store"],
    &["No-Store"],
    &["private, no-store"],
    &["max-age=0", "no-store"],
    &["max-age=0 ,  no-store  "],
    &["no-store=\""],
    &["max-age=0 private"],
];

/// F1's expected text after whitespace normalisation.
pub const F1_EXPECTED: &str = "Synthetic heading Visible paragraph with bold words. \
    Shadow words Caf\u{e9} na\u{ef}ve fa\u{e7}ade \u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd} \
    \u{6771}\u{4eac}\u{306e}\u{5929}\u{6c17}";

const F1: &str = "<!doctype html><meta charset=utf-8><title>F1 synthetic</title>\
<link rel=icon href=data:,>\
<style>.gone{display:none}.ghost{visibility:hidden}</style>\
<h1>Synthetic heading</h1>\
<p>Visible paragraph with <b>bold</b> words.</p>\
<p class=gone>display-none-sentinel</p>\
<p class=ghost>visibility-hidden-sentinel</p>\
<script>var unused='script-sentinel';</script>\
<noscript>noscript-sentinel</noscript>\
<div id=host></div>\
<p lang=fr>Caf\u{e9} na\u{ef}ve fa\u{e7}ade</p>\
<p dir=rtl lang=he>\u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}</p>\
<p lang=ja>\u{6771}\u{4eac}\u{306e}\u{5929}\u{6c17}</p>\
<script>document.getElementById('host').attachShadow({mode:'open'}).innerHTML='<span>Shadow words</span>';</script>";

const FORM_PASSWORD: &str = "<!doctype html><meta charset=utf-8><title>form</title>\
<link rel=icon href=data:,><p>Account page</p><input id=u><input type=password id=p>\
<script>u.value='typed-user-sentinel';p.value='typed-password-sentinel';</script>";

const FORM_PLAIN: &str = "<!doctype html><meta charset=utf-8><title>form</title>\
<link rel=icon href=data:,><p>Comment page</p><input id=a value=attribute-value-sentinel>\
<textarea id=b></textarea><div id=c contenteditable></div>\
<select><option>option-sentinel</select>\
<script>a.value='typed-input-sentinel';b.value='typed-textarea-sentinel';\
c.textContent='typed-editable-sentinel';</script>";

const FORM_CARD: &str = "<!doctype html><meta charset=utf-8><title>form</title>\
<link rel=icon href=data:,><p>Checkout page</p><input autocomplete=cc-number id=n>\
<script>n.value='4000-synthetic-sentinel';</script>";

const PROBE: &str = r#"<!doctype html><meta charset=utf-8><title>waiting</title>
<link rel=icon href=data:,><p>Synthetic detection probe</p><script>
const results=[];
results.push('webdriver='+navigator.webdriver);
let consoleRead=false;
const marker=new Error('synthetic');
Object.defineProperty(marker,'stack',{get(){consoleRead=true;return '';}});
console.debug(marker);
const before=performance.now();
debugger;
const paused=performance.now()-before;
setTimeout(()=>{
  results.push('console='+consoleRead);
  results.push('debugger='+(paused>100?'slow':'fast'));
  const keys=Object.keys(window).sort();
  let hash=0;for(const c of keys.join(',')){hash=(hash*31+c.charCodeAt(0))|0;}
  results.push('globals='+keys.length+':'+hash);
  results.push('automation='+keys.filter(k=>/cdc_|webdriver|domAutomation/i.test(k)).length);
  document.title='probe '+results.join(';');
},100);
</script>"#;

const HISTORY: &str = "<!doctype html><meta charset=utf-8><title>history</title>\
<link rel=icon href=data:,><p>History page</p><script>\
addEventListener('load',()=>setTimeout(()=>{for(let i=1;i<=5;i++){history.pushState({},'','/history/'+i);}\
document.title='pushed';},3000));</script>";

const SW_PAGE: &str = "<!doctype html><meta charset=utf-8><title>sw pending</title>\
<link rel=icon href=data:,><p>Service worker host</p><script>\
navigator.serviceWorker.register('/sw.js').then(()=>navigator.serviceWorker.ready)\
.then(()=>{if(navigator.serviceWorker.controller){document.title='sw-controlled';}\
else{navigator.serviceWorker.addEventListener('controllerchange',()=>document.title='sw-controlled');}})\
.catch(()=>document.title='sw-failed');</script>";

const SW_SCRIPT: &str = "self.addEventListener('install',e=>self.skipWaiting());\
self.addEventListener('activate',e=>e.waitUntil(self.clients.claim()));\
self.addEventListener('fetch',e=>{if(new URL(e.request.url).pathname==='/sw-synth'){\
e.respondWith(new Response('<!doctype html><title>sw synthetic</title><p>sw-synthetic-sentinel</p>',\
{headers:{'Content-Type':'text/html'}}));}});";

/// A 1x1 transparent PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// Chooses the reply for one request.
pub fn route(role: Role, path: &str, sites: &Sites) -> Reply {
    // A query only separates runs in the hit counter; it never changes the page.
    let path = path.split('?').next().unwrap_or(path);
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["landing"] => landing(role),
        ["f1"] => Reply::html(F1),
        ["nostore", variant] => no_store(variant),
        ["go", code, target, rest @ ..] => redirect(code, target, rest, sites, false),
        ["gons", code, target, rest @ ..] => redirect(code, target, rest, sites, true),
        ["navigate", mode] => self_navigation(mode, sites),
        ["signin", mode] => sign_in_hop(mode, sites),
        ["link-out"] => link_out(sites),
        ["popup-out"] => popup_out(sites),
        ["relay"] => relay(sites),
        ["postto", code, target] => post_to(code, target, sites),
        ["posted"] => Reply::html(
            "<!doctype html><meta charset=utf-8><title>posted</title>\
             <link rel=icon href=data:,><p>posted-sentinel</p>",
        ),
        ["race", delay] => race(delay, sites),
        ["frames"] => frames(sites),
        ["frame-child"] => Reply::html(format!(
            "<!doctype html><title>frame</title><p>frame-sentinel-{}</p>",
            role.name()
        )),
        ["form-password"] => Reply::html(FORM_PASSWORD),
        ["form-plain"] => Reply::html(FORM_PLAIN),
        ["form-card"] => Reply::html(FORM_CARD),
        ["pdf"] => Reply::typed("application/pdf", b"%PDF-1.4\n%synthetic\n".to_vec()),
        ["image"] => Reply::typed("image/png", PNG.to_vec()),
        ["text"] => Reply::typed("text/plain", b"plain-text-sentinel".to_vec()),
        // F20: successful responses that commit no document, leaving the previous one shown.
        ["nocontent"] => Reply::html("").status("204 No Content"),
        ["resetcontent"] => Reply::html("").status("205 Reset Content"),
        ["missing"] => Reply::html("<p>missing-sentinel</p>").status("404 Not Found"),
        ["broken"] => Reply::html("<p>broken-sentinel</p>").status("500 Internal Server Error"),
        ["history", ..] => Reply::html(HISTORY),
        ["probe"] => Reply::html(PROBE),
        ["sw-page"] => Reply::html(SW_PAGE),
        ["sw.js"] => Reply::typed("text/javascript", SW_SCRIPT.as_bytes().to_vec()),
        ["sw-synth"] => Reply::html("<p>sw-network-sentinel</p>"),
        ["cacheable"] => Reply::html("<title>cacheable</title><p>Cacheable page</p>")
            .header("Cache-Control", "max-age=600"),
        ["big", count] => big(count),
        _ => Reply::html("<p>unknown</p>").status("404 Not Found"),
    }
}

fn bad_request() -> Reply {
    Reply::html("<p>bad request</p>").status("400 Bad Request")
}

fn landing(role: Role) -> Reply {
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>landing {0}</title>\
         <link rel=icon href=data:,><p>landing-sentinel-{0}</p>",
        role.name()
    ))
}

fn no_store(variant: &str) -> Reply {
    let Some(values) = variant
        .parse::<usize>()
        .ok()
        .and_then(|index| NO_STORE_VARIANTS.get(index))
    else {
        return bad_request();
    };
    values.iter().fold(
        Reply::html("<title>no-store</title><p>no-store-sentinel</p>"),
        |reply, value| reply.header("Cache-Control", value),
    )
}

fn redirect(code: &str, target: &str, rest: &[&str], sites: &Sites, hop_no_store: bool) -> Reply {
    let status = match code {
        "301" => "301 Moved Permanently",
        "302" => "302 Found",
        "303" => "303 See Other",
        "307" => "307 Temporary Redirect",
        "308" => "308 Permanent Redirect",
        _ => return bad_request(),
    };
    let Some(location) = sites.resolve(target, &rest.join("/")) else {
        return bad_request();
    };
    let reply = Reply::html("<p>redirecting</p>")
        .status(status)
        .header("Location", &location);
    if hop_no_store {
        return reply.header("Cache-Control", "no-store");
    }
    reply
}

/// F7: the page leaves for the bank 500 ms after loading, by timer, `location.assign` or
/// meta refresh.
fn self_navigation(mode: &str, sites: &Sites) -> Reply {
    let bank = sites.url(Role::Bank, "/landing");
    let navigation = match mode {
        "timer" => format!(
            "<script>addEventListener('load',()=>setTimeout(()=>{{location.href='{bank}';}},500));</script>"
        ),
        "assign" => format!(
            "<script>addEventListener('load',()=>setTimeout(()=>location.assign('{bank}'),500));</script>"
        ),
        _ => format!("<meta http-equiv=refresh content=\"0.5;url={bank}\">"),
    };
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>leaving</title><link rel=icon href=data:,>\
         <p>Leaving page</p>{navigation}"
    ))
}

/// F21 and F22: an excluded page sends itself on by script or by an auto-submitting form,
/// as a sign-in hop does. Served from the plain site, the form case is F24's plain POST.
fn sign_in_hop(mode: &str, sites: &Sites) -> Reply {
    let landing = sites.url(Role::Plain, "/landing?via=script");
    let posted = sites.url(Role::Plain, "/posted");
    let relay = sites.url(Role::Plain, "/relay");
    let leaving = match mode {
        "chain" => format!(
            "<script>addEventListener('load',()=>setTimeout(()=>{{location.href='{relay}';}},300));</script>"
        ),
        "script" => format!(
            "<script>addEventListener('load',()=>setTimeout(()=>{{location.href='{landing}';}},300));</script>"
        ),
        "form" => format!(
            "<form method=post action='{posted}'><input type=hidden name=token value=synthetic></form>\
             <script>addEventListener('load',()=>setTimeout(()=>document.forms[0].submit(),300));</script>"
        ),
        _ => return bad_request(),
    };
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>hop</title><link rel=icon href=data:,>\
         <p>Hop page</p>{leaving}"
    ))
}

/// F23: a link out of the page, focused by the page's own script so a synthetic Enter
/// key follows it as the owner would.
fn link_out(sites: &Sites) -> Reply {
    let landing = sites.url(Role::Plain, "/landing?via=link");
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>link-out</title><link rel=icon href=data:,>\
         <p><a id=out href='{landing}'>Continue</a></p>\
         <script>out.focus();addEventListener('focus',()=>out.focus());</script>"
    ))
}

/// F26: an ordinary page that moves on by its own script, as an OAuth callback does.
fn relay(sites: &Sites) -> Reply {
    let landing = sites.url(Role::Plain, "/landing?via=relay");
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>relay</title><link rel=icon href=data:,>\
         <p>Relay page</p>\
         <script>addEventListener('load',()=>setTimeout(()=>{{location.href='{landing}';}},300));</script>"
    ))
}

/// F27: a focused button that opens a plain page in a popup when a synthetic Enter clicks it.
fn popup_out(sites: &Sites) -> Reply {
    let landing = sites.url(Role::Plain, "/landing?via=popup");
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>popup-out</title><link rel=icon href=data:,>\
         <p><button id=go onclick=\"window.open('{landing}')\">Open</button></p>\
         <script>go.focus();addEventListener('focus',()=>go.focus());</script>"
    ))
}

/// F25: an auto-submitting form `POST` answered by a redirect (`code`) to `target`'s landing.
fn post_to(code: &str, target: &str, sites: &Sites) -> Reply {
    if !matches!(code, "302" | "303" | "307") || Role::ALL.iter().all(|role| role.name() != target)
    {
        return bad_request();
    }
    let action = sites.url(Role::Plain, &format!("/go/{code}/{target}/landing"));
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>post</title><link rel=icon href=data:,>\
         <form method=post action='{action}'><input type=hidden name=token value=synthetic></form>\
         <script>addEventListener('load',()=>setTimeout(()=>document.forms[0].submit(),300));</script>"
    ))
}

/// F8: the page leaves for the bank a given number of milliseconds after loading.
fn race(delay: &str, sites: &Sites) -> Reply {
    let Ok(delay) = delay.parse::<u32>() else {
        return bad_request();
    };
    let bank = sites.url(Role::Bank, "/landing");
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>race</title><link rel=icon href=data:,>\
         <p>Race page</p><script>addEventListener('load',()=>setTimeout(()=>location.assign('{bank}'),{delay}));</script>"
    ))
}

fn frames(sites: &Sites) -> Reply {
    let same = sites.url(Role::Plain, "/frame-child");
    let cross = sites.url(Role::Plain2, "/frame-child");
    let bank = sites.url(Role::Bank, "/frame-child");
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>frames</title><link rel=icon href=data:,>\
         <p>Frame host page</p><iframe src='{same}'></iframe><iframe src='{cross}'></iframe>\
         <iframe src='{bank}'></iframe>"
    ))
}

/// F16: a page with about `count` nodes that reports its longest task in the title.
fn big(count: &str) -> Reply {
    let Ok(count) = count.parse::<usize>() else {
        return bad_request();
    };
    let mut body = String::with_capacity(count * 16);
    for index in 0..count / 2 {
        body.push_str(&format!("<p>Word {index}</p>"));
    }
    Reply::html(format!(
        "<!doctype html><meta charset=utf-8><title>big</title><link rel=icon href=data:,>\
         <script>let longest=0,tasks=0;new PerformanceObserver(list=>{{for(const entry of list.getEntries())\
         {{tasks++;longest=Math.max(longest,entry.duration);}}document.title='big longest='+Math.round(longest)+' tasks='+tasks;}})\
         .observe({{type:'longtask',buffered:true}});</script>{body}"
    ))
}
