//! Lays out the index's fields once, so writing, reading and checking agree on them.

use tantivy::{
    Index, TantivyDocument, Term,
    schema::{
        FAST, Field, INDEXED, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing,
        TextOptions, Value,
    },
    tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer},
};

use super::types::{FullTextError, Namespace};

/// Folds case and accents, so "Cafe" finds "café".
const TOKENIZER: &str = "syrinx-folded";
/// Ends the namespace inside a key; namespaces never contain it, so keys never collide.
const KEY_SEPARATOR: char = '\u{1f}';

/// What a stored row is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RowKind {
    Document = 0,
    /// Marks a deleted id and revision; it holds no title or body.
    Tombstone = 1,
}

/// Every field the adapter writes; one row per key at most.
#[derive(Clone, Copy)]
pub(super) struct Fields {
    pub key: Field,
    pub namespace: Field,
    pub id: Field,
    pub kind: Field,
    pub revision: Field,
    pub captured: Field,
    pub title: Field,
    pub body: Field,
}

/// A row's identity and revision, read back from storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Held {
    pub kind: RowKind,
    pub revision: u64,
}

pub(super) fn build() -> (Schema, Fields) {
    let mut schema = Schema::builder();
    let text = TextOptions::default().set_stored().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let fields = Fields {
        key: schema.add_text_field("key", STRING | STORED),
        namespace: schema.add_text_field("namespace", STRING | STORED),
        id: schema.add_text_field("id", STORED),
        kind: schema.add_u64_field("kind", INDEXED | STORED),
        revision: schema.add_u64_field("revision", STORED),
        // Indexed and fast, so expiry and clears select by date without loading rows.
        captured: schema.add_i64_field("captured", INDEXED | STORED | FAST),
        title: schema.add_text_field("title", text.clone()),
        body: schema.add_text_field("body", text),
    };
    (schema.build(), fields)
}

pub(super) fn register_tokenizer(index: &Index) {
    index.tokenizers().register(
        TOKENIZER,
        TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(LowerCaser)
            .filter(AsciiFoldingFilter)
            .build(),
    );
}

pub(super) fn key(namespace: &Namespace, id: &str) -> String {
    format!("{}{KEY_SEPARATOR}{id}", namespace.stored())
}

impl Fields {
    pub fn key_term(&self, namespace: &Namespace, id: &str) -> Term {
        Term::from_field_text(self.key, &key(namespace, id))
    }

    pub fn namespace_term(&self, namespace: &Namespace) -> Term {
        Term::from_field_text(self.namespace, &namespace.stored())
    }

    pub fn kind_term(&self, kind: RowKind) -> Term {
        Term::from_field_u64(self.kind, kind as u64)
    }

    pub fn held(&self, row: &TantivyDocument) -> Result<Held, FullTextError> {
        let kind = match self.number(row, self.kind)? {
            0 => RowKind::Document,
            1 => RowKind::Tombstone,
            _ => return Err(FullTextError::Damaged),
        };
        let revision = self.number(row, self.revision)?;
        Ok(Held { kind, revision })
    }

    pub fn number(&self, row: &TantivyDocument, field: Field) -> Result<u64, FullTextError> {
        row.get_first(field)
            .and_then(|value| value.as_u64())
            .ok_or(FullTextError::Damaged)
    }

    pub fn text<'a>(
        &self,
        row: &'a TantivyDocument,
        field: Field,
    ) -> Result<&'a str, FullTextError> {
        row.get_first(field)
            .and_then(|value| value.as_str())
            .ok_or(FullTextError::Damaged)
    }

    /// A tombstone row: the key and revision only, dated for expiry.
    pub fn tombstone(
        &self,
        namespace: &Namespace,
        id: &str,
        revision: u64,
        dated: i64,
    ) -> TantivyDocument {
        let mut row = self.identity(namespace, id, RowKind::Tombstone, revision);
        row.add_i64(self.captured, dated);
        row
    }

    pub fn document(&self, document: &super::types::FullTextDocument) -> TantivyDocument {
        let mut row = self.identity(
            &document.namespace,
            &document.id,
            RowKind::Document,
            document.revision,
        );
        row.add_i64(self.captured, document.captured_at);
        row.add_text(self.title, &document.title);
        row.add_text(self.body, &document.body);
        row
    }

    fn identity(
        &self,
        namespace: &Namespace,
        id: &str,
        kind: RowKind,
        revision: u64,
    ) -> TantivyDocument {
        let mut row = TantivyDocument::default();
        row.add_text(self.key, key(namespace, id));
        row.add_text(self.namespace, namespace.stored());
        row.add_text(self.id, id);
        row.add_u64(self.kind, kind as u64);
        row.add_u64(self.revision, revision);
        row
    }
}
