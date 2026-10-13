//! Decides, before any access, whether Atlas may read a page's text (D35, P1-25); only
//! `judge` creates the `EligibleDocument` the platform's capture call requires.

mod cache_control;
mod sites;

use std::{
    fmt,
    time::{Duration, Instant},
};

use cache_control::Storing;

/// How long a finished page may run its own scripts before its text is read.
pub const SETTLE_DELAY: Duration = Duration::from_secs(2);
/// The shortest gap between two captures of one tab after History API address changes.
pub const SAME_DOCUMENT_INTERVAL: Duration = Duration::from_secs(30);
/// The most text kept from one document, in bytes of UTF-8.
pub const TEXT_LIMIT_BYTES: usize = 256 * 1024;

/// What Atlas may read: capture is on, plus the built-in and owner exclusion lists.
#[derive(Clone, Debug)]
pub struct PageTextPolicy {
    banking: Vec<String>,
    owner: Vec<String>,
    settle: Duration,
}

/// A site in an exclusion list that is not a host name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyError;

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("An excluded site is not a valid host name.")
    }
}

impl std::error::Error for PolicyError {}

impl PageTextPolicy {
    /// Capture switched on, with the built-in lists and the owner's excluded sites.
    pub fn new<S: AsRef<str>>(owner_exclusions: &[S]) -> Result<Self, PolicyError> {
        let banking = sites::BANKING_DOMAINS
            .iter()
            .map(|domain| (*domain).to_owned())
            .collect();
        Ok(Self {
            banking,
            owner: normalized_hosts(owner_exclusions)?,
            settle: SETTLE_DELAY,
        })
    }

    /// Adds sites to the banking and payment list; the native harness uses this so its
    /// synthetic bank runs through the same path as a listed one.
    pub fn with_banking<S: AsRef<str>>(mut self, domains: &[S]) -> Result<Self, PolicyError> {
        self.banking.extend(normalized_hosts(domains)?);
        Ok(self)
    }

    /// Replaces the settle delay; the native race tests shorten it.
    pub fn with_settle_delay(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    pub fn settle_delay(&self) -> Duration {
        self.settle
    }

    /// Steps 2 to 5: an address Atlas may read at all, before headers are considered.
    pub fn address_allowed(&self, address: &str) -> Result<(), Ineligible> {
        let host = web_host(address)?;
        if sites::is_google(&host) {
            return Err(Ineligible::Google);
        }
        if listed(&self.banking, &host) {
            return Err(Ineligible::BankingOrPayment);
        }
        if listed(&self.owner, &host) {
            return Err(Ineligible::OwnerExcluded);
        }
        Ok(())
    }
}

/// A response WebView2 reported for the tab, read only for status and headers.
#[derive(Clone, Eq, PartialEq)]
pub struct ObservedResponse {
    pub address: String,
    pub status: u16,
    pub content_type: Option<String>,
    /// Every `Cache-Control` header value, in order.
    pub cache_control: Vec<String>,
    /// The request's `Sec-Fetch-Dest`, which tells a page from a frame or a subresource.
    pub destination: Option<String>,
    /// The request's method; anything but `GET` came from a form submission.
    pub method: String,
}

impl fmt::Debug for ObservedResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObservedResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// What the platform observed about one top-level document, gathered before access.
#[derive(Clone, Copy)]
pub struct DocumentFacts<'a> {
    /// The address the tab shows now; after a History API change it differs from
    /// `document_address`.
    pub address: &'a str,
    /// The address whose response produced the document.
    pub document_address: &'a str,
    /// Every address the navigation requested before the final one, in order.
    pub redirects: &'a [String],
    /// Every response observed during the navigation, in any order.
    pub responses: &'a [ObservedResponse],
    /// The document that was committed when the navigation began, when the owner did not
    /// start it (WebView2's `IsUserInitiated` was false).
    pub unprompted_from: Option<&'a str>,
    /// The tab's navigation generation when these facts were gathered.
    pub generation: u64,
}

/// Why a document is not read. Its title and address still reach tier 1 search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ineligible {
    /// Step 1: capture is switched off.
    CaptureOff,
    /// Step 2: not an `http` or `https` address with a host.
    Scheme,
    /// Step 3: one of Google's sites.
    Google,
    /// Step 4: a listed bank or payment site.
    BankingOrPayment,
    /// Step 5: a site the owner excluded.
    OwnerExcluded,
    /// Step 6: the navigation passed through an excluded site.
    RedirectThroughExcluded,
    /// Step 6: an excluded site's own script or form started the navigation, as a sign-in
    /// hop does; a link the owner follows out of the site is still eligible.
    LeftExcludedUnprompted,
    /// Step 6: the document, or a hop before it, came from a form submission (a `POST`).
    FormPost,
    /// Step 7: no response was observed for the document, so `no-store` is unproven.
    NoResponse,
    /// Step 7: the document's response was not a success.
    Status,
    /// Step 7: the document is not HTML.
    ContentType,
    /// Step 7: the response forbids storing.
    NoStore,
    /// Step 7: a `Cache-Control` header could not be read.
    MalformedCacheControl,
    /// Step 8: the tab navigated after the facts were gathered.
    Stale,
}

/// A document judged readable. Only `judge` creates one.
#[derive(Clone, Eq, PartialEq)]
pub struct EligibleDocument {
    address: String,
    generation: u64,
}

impl fmt::Debug for EligibleDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EligibleDocument")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl EligibleDocument {
    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Step 8: nothing has changed in the tab since this document was judged.
    pub fn is_current(&self, generation: u64) -> bool {
        self.generation == generation
    }

    /// Step 9: a snapshot's root address is this document's, ignoring any fragment.
    pub fn matches_snapshot(&self, snapshot_address: &str) -> bool {
        without_fragment(snapshot_address)
            .is_some_and(|snapshot| Some(snapshot) == without_fragment(&self.address))
    }
}

/// Steps 1 to 8 in order, stopping at the first that fails. `policy` is `None` when capture
/// is switched off; `current_generation` is read just before the call.
pub fn judge(
    policy: Option<&PageTextPolicy>,
    facts: DocumentFacts<'_>,
    current_generation: u64,
) -> Result<EligibleDocument, Ineligible> {
    let policy = policy.ok_or(Ineligible::CaptureOff)?;
    policy.address_allowed(facts.address)?;
    policy.address_allowed(facts.document_address)?;
    redirects_allowed(policy, facts.redirects)?;
    opener_allowed(policy, facts.unprompted_from)?;
    no_form_post(facts.responses)?;
    response_allows_storing(facts.document_address, facts.responses)?;
    if facts.generation != current_generation {
        return Err(Ineligible::Stale);
    }
    Ok(EligibleDocument {
        address: facts.address.to_owned(),
        generation: facts.generation,
    })
}

/// When a History API address change may next be captured: after the settle delay, and no
/// sooner than `SAME_DOCUMENT_INTERVAL` after the tab's last capture.
pub fn same_document_due(last_capture: Option<Instant>, now: Instant, settle: Duration) -> Instant {
    let settled = now + settle;
    last_capture.map_or(settled, |last| settled.max(last + SAME_DOCUMENT_INTERVAL))
}

/// Step 6: an excluded hop makes the whole chain ineligible.
fn redirects_allowed(policy: &PageTextPolicy, redirects: &[String]) -> Result<(), Ineligible> {
    if redirects
        .iter()
        .any(|hop| policy.address_allowed(hop).is_err())
    {
        return Err(Ineligible::RedirectThroughExcluded);
    }
    Ok(())
}

/// Step 6: a navigation the owner did not start may not leave an excluded site. A page
/// that is not a web address (a blank tab) is not a site, so leaving it is allowed.
fn opener_allowed(
    policy: &PageTextPolicy,
    unprompted_from: Option<&str>,
) -> Result<(), Ineligible> {
    let Some(opener) = unprompted_from else {
        return Ok(());
    };
    match policy.address_allowed(opener) {
        Ok(()) | Err(Ineligible::Scheme) => Ok(()),
        Err(_) => Err(Ineligible::LeftExcludedUnprompted),
    }
}

/// Step 6: any hop sent with a method other than `GET` makes the document a form result.
fn no_form_post(responses: &[ObservedResponse]) -> Result<(), Ineligible> {
    if responses
        .iter()
        .any(|response| !response.method.eq_ignore_ascii_case("GET"))
    {
        return Err(Ineligible::FormPost);
    }
    Ok(())
}

/// Step 7: every response that could have produced the document is a cacheable HTML success.
/// Responses from frames and subresources are ignored; none at all fails closed.
fn response_allows_storing(
    document_address: &str,
    responses: &[ObservedResponse],
) -> Result<(), Ineligible> {
    let document = without_fragment(document_address);
    let mut matching = responses.iter().filter(|response| {
        is_document_destination(response) && without_fragment(&response.address) == document
    });
    let first = matching.next().ok_or(Ineligible::NoResponse)?;
    std::iter::once(first)
        .chain(matching)
        .try_for_each(response_verdict)
}

fn response_verdict(response: &ObservedResponse) -> Result<(), Ineligible> {
    if !(200..300).contains(&response.status) {
        return Err(Ineligible::Status);
    }
    if !is_html(response.content_type.as_deref()) {
        return Err(Ineligible::ContentType);
    }
    match cache_control::storing(&response.cache_control) {
        Storing::Allowed => Ok(()),
        Storing::NoStore => Err(Ineligible::NoStore),
        Storing::Malformed => Err(Ineligible::MalformedCacheControl),
    }
}

/// A missing `Sec-Fetch-Dest` cannot tell a page from a frame, so it still counts.
fn is_document_destination(response: &ObservedResponse) -> bool {
    response
        .destination
        .as_deref()
        .is_none_or(|destination| destination.eq_ignore_ascii_case("document"))
}

fn is_html(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return false;
    };
    let essence = content_type.split(';').next().unwrap_or_default().trim();
    essence.eq_ignore_ascii_case("text/html")
        || essence.eq_ignore_ascii_case("application/xhtml+xml")
}

/// Step 2: the lower-case, punycode host of an `http`/`https` address, without a trailing dot.
fn web_host(address: &str) -> Result<String, Ineligible> {
    let parsed = url::Url::parse(address).map_err(|_| Ineligible::Scheme)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Ineligible::Scheme);
    }
    let host = parsed.host_str().ok_or(Ineligible::Scheme)?;
    let host = host.trim_end_matches('.');
    if host.is_empty() {
        return Err(Ineligible::Scheme);
    }
    Ok(host.to_owned())
}

fn listed(domains: &[String], host: &str) -> bool {
    domains
        .iter()
        .any(|domain| sites::within_domain(host, domain))
}

/// Normalizes list entries the same way addresses are, so `Bank.TEST.` matches `bank.test`.
fn normalized_hosts<S: AsRef<str>>(entries: &[S]) -> Result<Vec<String>, PolicyError> {
    entries
        .iter()
        .map(|entry| normalized_host(entry.as_ref()))
        .collect()
}

/// A bare host name; a path, port, query or user name makes the entry invalid.
fn normalized_host(entry: &str) -> Result<String, PolicyError> {
    let parsed = url::Url::parse(&format!("http://{entry}/")).map_err(|_| PolicyError)?;
    let bare = parsed.path() == "/"
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none();
    if !bare {
        return Err(PolicyError);
    }
    web_host(parsed.as_str()).map_err(|_| PolicyError)
}

fn without_fragment(address: &str) -> Option<String> {
    let mut parsed = url::Url::parse(address).ok()?;
    parsed.set_fragment(None);
    Some(parsed.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_entries_are_normalized_like_addresses() {
        let policy = PageTextPolicy::new(&["Owner.TEST."]).expect("synthetic owner list");
        assert_eq!(
            policy.address_allowed("https://www.owner.test/"),
            Err(Ineligible::OwnerExcluded)
        );
        assert!(PageTextPolicy::new(&["has space.test"]).is_err());
        assert!(PageTextPolicy::new(&["a/b"]).is_err());
    }

    #[test]
    fn document_debug_output_omits_the_address() {
        let sentinel = "https://private-debug-sentinel.example.test/";
        let document = EligibleDocument {
            address: sentinel.into(),
            generation: 3,
        };
        assert!(!format!("{document:?}").contains("sentinel"));
        let response = ObservedResponse {
            address: sentinel.into(),
            status: 200,
            content_type: None,
            cache_control: Vec::new(),
            destination: None,
            method: "GET".into(),
        };
        assert!(!format!("{response:?}").contains("sentinel"));
    }
}
