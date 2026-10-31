//! Reads the words of a page's HTML exactly as the server sent it, for engines with no
//! host-side DOM snapshot (WebKitGTK); nothing runs inside the page (P1-25 steps 10 and 11).

use crate::bookmark_import::{
    text::{collapse_whitespace, decode_entities},
    tokenizer::{StartTag, Token, Tokenizer},
};

use super::TEXT_LIMIT_BYTES;

/// The largest page read; a larger one is skipped unread, as an oversized snapshot is.
pub const SENT_HTML_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// `autocomplete` tokens that mark a payment-card field (step 10).
const CARD_TOKENS: &[&str] = &[
    "cc-number",
    "cc-csc",
    "cc-exp",
    "cc-exp-month",
    "cc-exp-year",
];

/// Elements whose contents are raw text that never shows: scripts, styles, form fields' text.
const RAW_TEXT: &[&str] = &[
    "script",
    "style",
    "noscript",
    "title",
    "textarea",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "plaintext",
];

/// Elements whose whole subtree is never kept: form choices, templates and the head.
const DROPPED: &[&str] = &["select", "template", "head"];

/// Elements that never have contents, so they open no subtree.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements that break a line, so the words either side of them never run together.
const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "br",
    "caption",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "legend",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "td",
    "th",
    "tr",
    "ul",
];

/// The words kept from one page.
#[derive(Debug, Eq, PartialEq)]
pub struct SentHtmlText {
    pub text: String,
    pub truncated: bool,
}

/// Why a page's HTML was thrown away instead of read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SentHtmlRefusal {
    /// Larger than `SENT_HTML_LIMIT_BYTES`.
    TooLarge,
    /// Not UTF-8; other encodings are not guessed at.
    NotUtf8,
    /// Step 10: the page has a password or payment-card field.
    SignInOrPayment,
}

/// Steps 10 and 11 on the page as sent: any sign-in or card field discards it; otherwise the
/// text outside scripts, styles, form fields, the head and hidden elements is kept, bounded.
/// Text a page builds with script, or hides with a style sheet, is not seen this way.
pub fn sent_html_text(html: &[u8]) -> Result<SentHtmlText, SentHtmlRefusal> {
    if html.len() > SENT_HTML_LIMIT_BYTES {
        return Err(SentHtmlRefusal::TooLarge);
    }
    let html = std::str::from_utf8(html).map_err(|_| SentHtmlRefusal::NotUtf8)?;
    let html = html.strip_prefix('\u{FEFF}').unwrap_or(html);
    let words = Reader::default().read(html)?;
    let (text, truncated) = bounded_page_text(collapse_whitespace(&words));
    Ok(SentHtmlText { text, truncated })
}

/// Whether an element is a password field or a payment-card field (step 10). `kind` and
/// `autocomplete` are its `type` and `autocomplete` attributes, entities decoded.
pub fn is_sign_in_or_card_field(
    element: &str,
    kind: Option<&str>,
    autocomplete: Option<&str>,
) -> bool {
    let password = element.eq_ignore_ascii_case("input")
        && kind.is_some_and(|kind| kind.trim().eq_ignore_ascii_case("password"));
    let card = autocomplete.is_some_and(|tokens| {
        tokens.split_ascii_whitespace().any(|token| {
            CARD_TOKENS
                .iter()
                .any(|card| token.eq_ignore_ascii_case(card))
        })
    });
    password || card
}

/// Caps page text at `TEXT_LIMIT_BYTES`, on a character boundary; reports whether it cut.
pub fn bounded_page_text(mut text: String) -> (String, bool) {
    if text.len() <= TEXT_LIMIT_BYTES {
        return (text, false);
    }
    let mut end = TEXT_LIMIT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

/// The words read so far, and the outermost element being skipped, with how deeply its own
/// name is nested inside itself.
#[derive(Default)]
struct Reader<'a> {
    words: String,
    skipping: Option<(&'a str, usize)>,
}

impl<'a> Reader<'a> {
    fn read(mut self, html: &'a str) -> Result<String, SentHtmlRefusal> {
        let mut tokens = Tokenizer::quoted(html);
        while let Some((_, token)) = tokens.next() {
            match token {
                Token::Start(tag) => self.start(tag, &mut tokens)?,
                Token::End(name) => self.end(name),
                Token::Text(raw) if self.skipping.is_none() => {
                    self.words.push_str(&decode_entities(raw));
                }
                Token::Text(_) | Token::Doctype(_) => {}
            }
        }
        Ok(self.words)
    }

    fn start(
        &mut self,
        tag: StartTag<'a>,
        tokens: &mut Tokenizer<'a>,
    ) -> Result<(), SentHtmlRefusal> {
        let decoded = |name: &str| tag.attribute(name).map(decode_entities);
        let kind = decoded("type");
        let autocomplete = decoded("autocomplete");
        if is_sign_in_or_card_field(tag.name, kind.as_deref(), autocomplete.as_deref()) {
            return Err(SentHtmlRefusal::SignInOrPayment);
        }
        self.separate(tag.name);
        if named(RAW_TEXT, tag.name) {
            tokens.skip_raw_text(tag.name);
            return Ok(());
        }
        if named(VOID, tag.name) {
            return Ok(());
        }
        match &mut self.skipping {
            Some((skipped, depth)) if skipped.eq_ignore_ascii_case(tag.name) => *depth += 1,
            Some(_) => {}
            None if hides_subtree(&tag) => self.skipping = Some((tag.name, 1)),
            None => {}
        }
        Ok(())
    }

    fn end(&mut self, name: &str) {
        self.separate(name);
        let Some((skipped, depth)) = &mut self.skipping else {
            return;
        };
        if !skipped.eq_ignore_ascii_case(name) {
            return;
        }
        *depth -= 1;
        if *depth == 0 {
            self.skipping = None;
        }
    }

    /// A block element's edge is a word break.
    fn separate(&mut self, name: &str) {
        if named(BLOCKS, name) {
            self.words.push(' ');
        }
    }
}

/// Whether an element and everything inside it is never shown or never kept: a dropped
/// element, an element marked `hidden`, an inline style that hides it, or an editable region.
fn hides_subtree(tag: &StartTag<'_>) -> bool {
    if named(DROPPED, tag.name) || tag.attribute("hidden").is_some() {
        return true;
    }
    let editable = tag
        .attribute("contenteditable")
        .is_some_and(|value| !value.trim().eq_ignore_ascii_case("false"));
    editable || tag.attribute("style").is_some_and(style_hides)
}

/// An inline style of `display: none` or `visibility: hidden` (or `collapse`).
fn style_hides(style: &str) -> bool {
    let compact: String = decode_entities(style)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    compact.split(';').any(|declaration| {
        let value = declaration.trim_end_matches("!important");
        matches!(
            value,
            "display:none" | "visibility:hidden" | "visibility:collapse"
        )
    })
}

fn named(list: &[&str], name: &str) -> bool {
    list.iter().any(|listed| listed.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(html: &str) -> String {
        sent_html_text(html.as_bytes()).expect("readable").text
    }

    #[test]
    fn keeps_body_words_and_drops_head_scripts_and_styles() {
        let html = "<!DOCTYPE html><html><head><title>Synthetic title</title>\
            <style>p { color: red }</style></head><body><h1>Heading</h1>\
            <p>First <b>bold</b>, then<br>next.</p>\
            <script>if (a < b && c > d) { document.write('<p>never</p>') }</script>\
            <p>Last &amp; done</p></body></html>";
        assert_eq!(text(html), "Heading First bold, then next. Last & done");
    }

    #[test]
    fn form_fields_and_their_values_are_never_kept() {
        let html = "<p>Before</p><input type=hidden value=\"token>synthetic-secret\">\
            <textarea>typed <b>draft</b></textarea>\
            <select><option>Choice one</option><option>Choice two</option></select><p>After</p>";
        assert_eq!(text(html), "Before After");
    }

    #[test]
    fn a_quoted_greater_than_sign_stays_inside_its_attribute() {
        let html = "<a title=\"a > b\" href='x>y'>Link</a> text";
        assert_eq!(text(html), "Link text");
    }

    #[test]
    fn hidden_and_editable_subtrees_are_skipped_with_their_nested_children() {
        let html = "<div>Shown<div hidden>Hidden <div>nested</div> still hidden</div>\
            <span style=\"color: red; DISPLAY : none !important\">styled away</span>\
            <p style=\"visibility:hidden\">invisible</p>\
            <div contenteditable=\"\">editor</div><div contenteditable=false>plain</div>\
            <template><p>template</p></template>after</div>";
        assert_eq!(text(html), "Shown plain after");
    }

    #[test]
    fn a_password_or_card_field_discards_the_page() {
        for field in [
            "<input type=\"password\" name=pw>",
            "<INPUT TYPE=PASSWORD>",
            "<input type=\"pass&#119;ord\">",
            "<input autocomplete=\"billing cc-number\">",
            "<div autocomplete=cc-exp></div>",
        ] {
            let html = format!("<p>Sign in</p><form>{field}</form>");
            assert_eq!(
                sent_html_text(html.as_bytes()),
                Err(SentHtmlRefusal::SignInOrPayment),
                "{field}"
            );
        }
        assert!(sent_html_text(b"<input type=text autocomplete=email>").is_ok());
    }

    #[test]
    fn a_field_inside_a_script_is_not_a_field_but_one_in_a_hidden_region_is() {
        assert!(sent_html_text(b"<script>'<input type=password>'</script><p>x</p>").is_ok());
        assert_eq!(
            sent_html_text(b"<div hidden><input type=password></div>"),
            Err(SentHtmlRefusal::SignInOrPayment)
        );
    }

    #[test]
    fn oversized_or_non_utf8_pages_are_refused_unread() {
        let large = vec![b'a'; SENT_HTML_LIMIT_BYTES + 1];
        assert_eq!(sent_html_text(&large), Err(SentHtmlRefusal::TooLarge));
        assert_eq!(
            sent_html_text(b"<p>caf\xE9</p>"),
            Err(SentHtmlRefusal::NotUtf8)
        );
        assert_eq!(text("\u{FEFF}<p>bom</p>"), "bom");
    }

    #[test]
    fn long_text_is_bounded_on_a_character_boundary() {
        let html = format!("<p>{}</p>", "é".repeat(TEXT_LIMIT_BYTES));
        let read = sent_html_text(html.as_bytes()).expect("readable");
        assert!(read.truncated);
        assert!(read.text.len() <= TEXT_LIMIT_BYTES);
        assert!(read.text.chars().all(|character| character == 'é'));
    }

    #[test]
    fn unterminated_markup_never_panics_and_fails_toward_dropping() {
        assert_eq!(text("<p>open <script>never closed"), "open");
        assert_eq!(text("<div hidden>never closed <p>more</p>"), "");
        assert_eq!(text("a < b <"), "a < b <");
        assert_eq!(text("<a title=\"unterminated>text"), "");
    }
}
