//! The built-in site lists whose pages Atlas never reads (D35, P1-25 steps 3 and 4).

/// Google-owned domains, each with all of its subdomains. The owner cannot remove these, but
/// reviews the list before P1-26 (proposal question 4).
/// Google's country domains (`google.de`, `google.co.uk`) are matched by `is_google_country`.
const GOOGLE_DOMAINS: &[&str] = &[
    "google",
    "goog",
    "android.com",
    "chrome.com",
    "gvt1.com",
    "gvt2.com",
    "googleblog.com",
    "youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "ytimg.com",
    "googlevideo.com",
    "gmail.com",
    "googlemail.com",
    "googleusercontent.com",
    "googleapis.com",
    "gstatic.com",
    "ggpht.com",
    "withgoogle.com",
    "goo.gl",
    "g.co",
    "g.page",
    "blogger.com",
    "blogspot.com",
    "appspot.com",
    "googlesource.com",
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "googletagmanager.com",
    "google-analytics.com",
    "recaptcha.net",
];

/// Second-level labels Google uses under a country code, as in `google.co.uk`.
const GOOGLE_COUNTRY_SECOND_LEVELS: &[&str] = &["co", "com"];

/// A starter list of payment processors and banks. The owner reviews it before P1-26
/// (proposal question 5); the password and card-field guard covers banks missing here.
pub(super) const BANKING_DOMAINS: &[&str] = &[
    "paypal.com",
    "paypal.me",
    "stripe.com",
    "squareup.com",
    "wise.com",
    "venmo.com",
    "klarna.com",
    "adyen.com",
    "checkout.com",
    "braintreegateway.com",
    "revolut.com",
    "coinbase.com",
    "chase.com",
    "bankofamerica.com",
    "wellsfargo.com",
    "citi.com",
    "citibank.com",
    "capitalone.com",
    "americanexpress.com",
    "discover.com",
    "usbank.com",
    "pnc.com",
    "schwab.com",
    "fidelity.com",
    "vanguard.com",
    "ally.com",
    "hsbc.com",
    "hsbc.co.uk",
    "barclays.co.uk",
    "lloydsbank.com",
    "natwest.com",
    "santander.co.uk",
    "monzo.com",
    "starlingbank.com",
];

/// Whether `host` is one of Google's sites. `host` is already lower-case, punycode and
/// without a trailing dot.
pub(super) fn is_google(host: &str) -> bool {
    is_google_country(host)
        || GOOGLE_DOMAINS
            .iter()
            .any(|domain| within_domain(host, domain))
}

/// `google.<anything>` and `google.co.<cc>` / `google.com.<cc>`, with any subdomains.
fn is_google_country(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    let count = labels.len();
    // `google.de`, `www.google.com`, `google.cat`.
    if count >= 2 && labels[count - 2] == "google" {
        return true;
    }
    // `google.co.uk`, `mail.google.com.au`.
    count >= 3
        && labels[count - 3] == "google"
        && GOOGLE_COUNTRY_SECOND_LEVELS.contains(&labels[count - 2])
        && is_country_code(labels[count - 1])
}

fn is_country_code(label: &str) -> bool {
    label.len() == 2 && label.bytes().all(|byte| byte.is_ascii_lowercase())
}

/// Whether `host` is `domain` itself or one of its subdomains.
pub(super) fn within_domain(host: &str, domain: &str) -> bool {
    let Some(prefix) = host.strip_suffix(domain) else {
        return false;
    };
    prefix.is_empty() || prefix.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_matching_needs_a_label_boundary() {
        assert!(within_domain("bank.test", "bank.test"));
        assert!(within_domain("www.bank.test", "bank.test"));
        assert!(!within_domain("notbank.test", "bank.test"));
        assert!(!within_domain("bank.test.example", "bank.test"));
    }

    #[test]
    fn country_rule_needs_google_in_the_registrable_position() {
        for host in [
            "google.de",
            "google.co.uk",
            "www.google.com.au",
            "google.cat",
        ] {
            assert!(is_google(host), "{host}");
        }
        for host in [
            "google.com.example.test",
            "google.co.example",
            "google.co.uk.example.test",
            "googlex.com",
            "notgoogle.com",
        ] {
            assert!(!is_google(host), "{host}");
        }
    }
}
