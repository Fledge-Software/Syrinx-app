//! The Content Security Policy Syrinx sends with every file a plugin view loads (research
//! note 10, "Content Security Policy and network"), and the per-load style nonce in it.

/// Fills a buffer from the operating system's random source; the application passes it in,
/// since this crate links no random number generator.
pub type RandomFill = fn(&mut [u8]) -> Result<(), String>;

/// Random bytes in one nonce.
const NONCE_BYTES: usize = 16;

/// A fresh value per load for `style-src 'nonce-…'`, which CodeMirror's `cspNonce` facet uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nonce(String);

impl Nonce {
    /// Sixteen random bytes as lowercase hex, a valid CSP `base64-value`.
    pub fn generate(fill: RandomFill) -> Result<Self, String> {
        let mut bytes = [0_u8; NONCE_BYTES];
        fill(&mut bytes)?;
        let hex = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(Self(hex))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The policy: nothing but the view's own files, styles with this load's nonce, no network
/// beyond its own origin and no frames, forms or plugins.
pub fn content_security_policy(nonce: &Nonce) -> String {
    format!(
        "default-src 'none'; script-src 'self'; style-src 'self' 'nonce-{}'; \
         img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; worker-src 'self'; \
         frame-src 'none'; child-src 'none'; object-src 'none'; base-uri 'none'; \
         form-action 'none'; frame-ancestors 'none'",
        nonce.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::{Nonce, content_security_policy};

    fn fixed(bytes: &mut [u8]) -> Result<(), String> {
        bytes.fill(0xab);
        Ok(())
    }

    fn broken(_: &mut [u8]) -> Result<(), String> {
        Err("no random source".into())
    }

    #[test]
    fn a_nonce_is_sixteen_bytes_of_lowercase_hex() {
        let nonce = Nonce::generate(fixed).expect("the nonce is made");
        assert_eq!(nonce.as_str(), "ab".repeat(16));
    }

    #[test]
    fn a_failed_random_source_makes_no_nonce() {
        assert_eq!(Nonce::generate(broken), Err("no random source".into()));
    }

    #[test]
    fn the_policy_is_exactly_note_ten_s() {
        let nonce = Nonce::generate(fixed).expect("the nonce is made");
        let expected = "default-src 'none'; script-src 'self'; style-src 'self' 'nonce-abababababababababababababababab'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; worker-src 'self'; frame-src 'none'; child-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
        assert_eq!(content_security_policy(&nonce), expected);
    }
}
