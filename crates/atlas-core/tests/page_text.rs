//! Proves the page-text eligibility rules (P1-25 U1–U7) with synthetic addresses only.

use std::time::{Duration, Instant};

use atlas_core::{
    DocumentFacts, Ineligible, ObservedResponse, PageTextPolicy, SAME_DOCUMENT_INTERVAL,
    SETTLE_DELAY, judge, same_document_due,
};

const PAGE: &str = "https://plain.example.test/article";

fn policy() -> PageTextPolicy {
    PageTextPolicy::new(&["owner.test"])
        .and_then(|policy| policy.with_banking(&["bank.test", "pay.test"]))
        .expect("synthetic policy")
}

fn html(address: &str, cache_control: &[&str]) -> ObservedResponse {
    ObservedResponse {
        address: address.to_owned(),
        status: 200,
        content_type: Some("text/html; charset=utf-8".to_owned()),
        cache_control: cache_control
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        destination: Some("document".to_owned()),
        method: "GET".to_owned(),
    }
}

fn facts<'a>(
    address: &'a str,
    redirects: &'a [String],
    responses: &'a [ObservedResponse],
) -> DocumentFacts<'a> {
    DocumentFacts {
        address,
        document_address: address,
        redirects,
        responses,
        unprompted_from: None,
        generation: 4,
    }
}

fn verdict(
    address: &str,
    redirects: &[String],
    responses: &[ObservedResponse],
) -> Result<(), Ineligible> {
    judge(Some(&policy()), facts(address, redirects, responses), 4).map(|_| ())
}

fn address_verdict(address: &str) -> Result<(), Ineligible> {
    policy().address_allowed(address)
}

// U1: Google's sites by registrable domain, never by substring.
#[test]
fn google_sites_are_excluded_and_lookalikes_are_not() {
    for address in [
        "https://google.com/",
        "https://www.google.co.uk/search",
        "https://mail.google.com/",
        "https://GOOGLE.COM./",
        "https://www.google.com../",
        "https://developer.android.com/",
        "https://sub.domain.goog/",
        "https://docs.google.com./d/1",
        "https://youtube.com/watch",
        "https://m.youtube.com/",
        "https://gmail.com/",
        "https://lh3.googleusercontent.com/a",
        "https://www.google.de/",
        "https://google.com.au/",
        "http://\u{ff47}\u{ff4f}\u{ff4f}\u{ff47}\u{ff4c}\u{ff45}.com/",
        "https://www.google:443/",
    ] {
        assert_eq!(
            address_verdict(address),
            Err(Ineligible::Google),
            "{address}"
        );
    }
    for address in [
        "https://notgoogle.com/",
        "https://google.com.example.test/",
        "https://googlex.com/",
        "https://notandroid.com/",
        "https://youtube.com.example.test/",
        "https://xn--ggle-0nda.test/",
        "https://example.test/google.com",
    ] {
        assert_eq!(address_verdict(address), Ok(()), "{address}");
    }
}

// U2: bank, payment and owner lists match a domain and its subdomains only.
#[test]
fn banking_payment_and_owner_lists_match_domains_and_subdomains_only() {
    for (address, reason) in [
        ("https://bank.test/", Ineligible::BankingOrPayment),
        ("https://login.bank.test/", Ineligible::BankingOrPayment),
        ("https://pay.test/checkout", Ineligible::BankingOrPayment),
        ("https://www.paypal.com/", Ineligible::BankingOrPayment),
        ("https://owner.test/", Ineligible::OwnerExcluded),
        ("https://deep.sub.owner.test/", Ineligible::OwnerExcluded),
    ] {
        assert_eq!(address_verdict(address), Err(reason), "{address}");
    }
    for address in [
        "https://notbank.test/",
        "https://bank.test.example/",
        "https://owner.test.example/",
        "https://example.test/bank.test",
        "https://paypal.com.example.test/",
    ] {
        assert_eq!(address_verdict(address), Ok(()), "{address}");
    }
}

// U3: only http and https pages are read.
#[test]
fn only_web_schemes_pass() {
    for address in [
        "file:///C:/synthetic.html",
        "data:text/html,synthetic",
        "blob:https://plain.example.test/1",
        "about:blank",
        "edge://settings",
        "javascript:1",
        "ftp://plain.example.test/",
        "not an address",
    ] {
        assert_eq!(
            address_verdict(address),
            Err(Ineligible::Scheme),
            "{address}"
        );
    }
    assert_eq!(address_verdict("http://plain.example.test/"), Ok(()));
    assert_eq!(address_verdict(PAGE), Ok(()));
}

// U4: any excluded hop makes the final document ineligible.
#[test]
fn a_redirect_through_an_excluded_site_is_never_read() {
    let responses = [html(PAGE, &[])];
    for hop in [
        "https://accounts.google.com/signin",
        "https://bank.test/return",
        "https://owner.test/sso",
        "file:///C:/synthetic.html",
    ] {
        let redirects = vec![
            "https://plain.example.test/start".to_owned(),
            hop.to_owned(),
        ];
        assert_eq!(
            verdict(PAGE, &redirects, &responses),
            Err(Ineligible::RedirectThroughExcluded),
            "{hop}"
        );
    }
    let ordinary = vec!["https://other.example.test/".to_owned()];
    assert_eq!(verdict(PAGE, &ordinary, &responses), Ok(()));
    // The reverse direction fails on the final address itself.
    let bank = "https://bank.test/";
    assert_eq!(
        verdict(bank, &ordinary, &[html(bank, &[])]),
        Err(Ineligible::BankingOrPayment)
    );
}

// U4: sign-in hops (owner's decision, 2026-10-12). A navigation an excluded site's script or
// form started is skipped; a link the owner follows out of the site is not.
#[test]
fn a_navigation_an_excluded_site_started_by_itself_is_never_read() {
    let responses = [html(PAGE, &[])];
    let judged = |unprompted_from| {
        let facts = DocumentFacts {
            unprompted_from,
            ..facts(PAGE, &[], &responses)
        };
        judge(Some(&policy()), facts, 4).map(|_| ())
    };
    for opener in [
        "https://bank.test/login",
        "https://accounts.google.com/signin",
        "https://owner.test/sso",
    ] {
        assert_eq!(
            judged(Some(opener)),
            Err(Ineligible::LeftExcludedUnprompted),
            "{opener}"
        );
    }
    // Leaving an ordinary page or a blank tab by script is allowed, as is a user's link
    // out of the bank (no unprompted opener is recorded then).
    assert_eq!(judged(Some("https://other.example.test/")), Ok(()));
    assert_eq!(judged(Some("about:blank")), Ok(()));
    assert_eq!(judged(None), Ok(()));
}

// U4: any form submission, on any site, makes the document ineligible.
#[test]
fn a_document_produced_by_a_form_post_is_never_read() {
    let posted = ObservedResponse {
        method: "POST".to_owned(),
        ..html(PAGE, &[])
    };
    assert_eq!(
        verdict(PAGE, &[], std::slice::from_ref(&posted)),
        Err(Ineligible::FormPost)
    );
    // A POST answered with a redirect to a plain GET page is still a form result.
    let start = "https://plain.example.test/submit";
    let hop = ObservedResponse {
        address: start.to_owned(),
        status: 303,
        ..posted
    };
    let redirects = vec![start.to_owned()];
    assert_eq!(
        verdict(PAGE, &redirects, &[hop, html(PAGE, &[])]),
        Err(Ineligible::FormPost)
    );
    let lower = ObservedResponse {
        method: "get".to_owned(),
        ..html(PAGE, &[])
    };
    assert_eq!(verdict(PAGE, &[], &[lower]), Ok(()));
}

// U5: every spelling of no-store, and malformed headers, fail closed.
#[test]
fn cache_control_no_store_and_malformed_values_are_ineligible() {
    for (values, expected) in [
        (&["no-store"][..], Err(Ineligible::NoStore)),
        (&["No-Store"], Err(Ineligible::NoStore)),
        (&["private, no-store"], Err(Ineligible::NoStore)),
        (&["max-age=0", "no-store"], Err(Ineligible::NoStore)),
        (&["  no-store\t"], Err(Ineligible::NoStore)),
        (&["no-store=\""], Err(Ineligible::MalformedCacheControl)),
        (
            &["max-age=0 private"],
            Err(Ineligible::MalformedCacheControl),
        ),
        (&["max-age=0"], Ok(())),
        (&["no-cache"], Ok(())),
        (&["private, max-age=0, no-cache"], Ok(())),
        (&[], Ok(())),
    ] {
        assert_eq!(
            verdict(PAGE, &[], &[html(PAGE, values)]),
            expected,
            "{values:?}"
        );
    }
}

// U5 and F6: only the final document's headers count, not a redirect hop's.
#[test]
fn no_store_on_a_redirect_hop_does_not_make_the_final_page_ineligible() {
    let start = "https://plain.example.test/start";
    let mut hop = html(start, &["no-store"]);
    hop.status = 302;
    let redirects = vec![start.to_owned()];
    assert_eq!(
        verdict(PAGE, &redirects, &[hop.clone(), html(PAGE, &[])]),
        Ok(())
    );
    assert_eq!(
        verdict(PAGE, &redirects, &[hop, html(PAGE, &["no-store"])]),
        Err(Ineligible::NoStore)
    );
}

// U6: missing responses, failures and non-HTML documents are ineligible.
#[test]
fn missing_failed_and_non_html_responses_are_ineligible() {
    assert_eq!(verdict(PAGE, &[], &[]), Err(Ineligible::NoResponse));
    let elsewhere = html("https://plain.example.test/other", &[]);
    assert_eq!(
        verdict(PAGE, &[], &[elsewhere]),
        Err(Ineligible::NoResponse)
    );
    let mut frame = html(PAGE, &[]);
    frame.destination = Some("iframe".to_owned());
    assert_eq!(verdict(PAGE, &[], &[frame]), Err(Ineligible::NoResponse));
    for status in [199, 304, 404, 500] {
        let mut response = html(PAGE, &[]);
        response.status = status;
        assert_eq!(
            verdict(PAGE, &[], &[response]),
            Err(Ineligible::Status),
            "{status}"
        );
    }
    for content_type in [
        Some("application/pdf"),
        Some("image/png"),
        Some("text/plain"),
        Some("text/htmlx"),
        None,
    ] {
        let mut response = html(PAGE, &[]);
        response.content_type = content_type.map(str::to_owned);
        assert_eq!(
            verdict(PAGE, &[], &[response]),
            Err(Ineligible::ContentType),
            "{content_type:?}"
        );
    }
    let mut xhtml = html(PAGE, &[]);
    xhtml.content_type = Some("Application/XHTML+XML".to_owned());
    assert_eq!(verdict(PAGE, &[], &[xhtml]), Ok(()));
    // A missing destination still counts, and one bad duplicate fails the document.
    let mut unknown = html(PAGE, &[]);
    unknown.destination = None;
    assert_eq!(verdict(PAGE, &[], &[unknown.clone()]), Ok(()));
    assert_eq!(
        verdict(PAGE, &[], &[unknown, html(PAGE, &["no-store"])]),
        Err(Ineligible::NoStore)
    );
}

// U6: a fragment does not separate a document from its response.
#[test]
fn a_fragment_in_the_address_still_matches_its_response() {
    let address = "https://plain.example.test/article#part-2";
    assert_eq!(verdict(address, &[], &[html(PAGE, &[])]), Ok(()));
}

// U7: a generation change between scheduling and dispatch cancels.
#[test]
fn a_generation_change_cancels_the_capture() {
    let responses = [html(PAGE, &[])];
    let facts = facts(PAGE, &[], &responses);
    assert_eq!(
        judge(Some(&policy()), facts, 5).map(|_| ()),
        Err(Ineligible::Stale)
    );
    let document = judge(Some(&policy()), facts, 4).expect("eligible synthetic page");
    assert!(document.is_current(4));
    assert!(!document.is_current(5));
    assert!(document.matches_snapshot(PAGE));
    assert!(document.matches_snapshot("https://plain.example.test/article#top"));
    assert!(!document.matches_snapshot("https://bank.test/"));
    assert!(!document.matches_snapshot("not an address"));
}

// Step 1: capture off reads nothing, whatever the page.
#[test]
fn capture_off_is_checked_first() {
    let responses = [html(PAGE, &[])];
    assert_eq!(
        judge(None, facts(PAGE, &[], &responses), 4).map(|_| ()),
        Err(Ineligible::CaptureOff)
    );
}

// Same-document changes recheck the new address but keep the original response.
#[test]
fn a_history_api_change_rechecks_the_new_address_with_the_original_response() {
    let responses = [html(PAGE, &[])];
    let mut same_document = facts("https://plain.example.test/article/2", &[], &responses);
    same_document.document_address = PAGE;
    assert_eq!(judge(Some(&policy()), same_document, 4).map(|_| ()), Ok(()));
    same_document.address = "https://bank.test/";
    assert_eq!(
        judge(Some(&policy()), same_document, 4).map(|_| ()),
        Err(Ineligible::BankingOrPayment)
    );
}

#[test]
fn same_document_captures_wait_for_the_interval() {
    let now = Instant::now();
    assert_eq!(
        same_document_due(None, now, SETTLE_DELAY),
        now + SETTLE_DELAY
    );
    let recent = now - Duration::from_secs(5);
    assert_eq!(
        same_document_due(Some(recent), now, SETTLE_DELAY),
        recent + SAME_DOCUMENT_INTERVAL
    );
    let long_ago = now - Duration::from_secs(300);
    assert_eq!(
        same_document_due(Some(long_ago), now, SETTLE_DELAY),
        now + SETTLE_DELAY
    );
}
