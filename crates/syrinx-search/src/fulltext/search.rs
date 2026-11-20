//! Turns the owner's words into a ranked query and a highlighted, bounded snippet.

use std::collections::BTreeMap;

use tantivy::{
    Index, Score, Searcher, TantivyDocument, Term,
    collector::TopDocs,
    query::{BooleanQuery, ConstScoreQuery, Occur, PhrasePrefixQuery, Query, TermQuery},
    schema::{Field, IndexRecordOption},
    snippet::SnippetGenerator,
};

use super::schema::{Fields, RowKind};
use super::types::{FullTextError, FullTextHit, Namespace, NamespaceFilter, SearchRequest};

/// Snippets are bounded in bytes, as Tantivy measures them; about two lines of preview.
pub(super) const SNIPPET_BYTES: usize = 160;
/// The most results one query returns; Tantivy reserves room for twice the limit.
pub(super) const MAX_RESULTS: usize = 1_000;
/// How many indexed words one typed prefix may stand for.
const MAX_PREFIX_EXPANSIONS: u32 = 1_000;

/// The query's words after the index's own folding, split into whole words and a prefix.
struct Words {
    whole: Vec<String>,
    prefix: Option<String>,
}

pub(super) fn run(
    index: &Index,
    searcher: &Searcher,
    fields: &Fields,
    request: &SearchRequest,
) -> Result<Vec<FullTextHit>, FullTextError> {
    let words = fold(index, fields, request)?;
    if request.limit == 0 || (words.whole.is_empty() && words.prefix.is_none()) {
        return Ok(Vec::new());
    }
    let query = build_query(fields, &words, &request.namespaces);
    let snippets = snippet_generator(index, searcher, fields, &words)?;
    let ranked = searcher.search(
        &query,
        &TopDocs::with_limit(request.limit.min(MAX_RESULTS)).order_by_score(),
    )?;
    ranked
        .into_iter()
        .map(|(score, address)| {
            let row: TantivyDocument = searcher.doc(address)?;
            hit(fields, &snippets, &row, score)
        })
        .collect()
}

fn fold(index: &Index, fields: &Fields, request: &SearchRequest) -> Result<Words, FullTextError> {
    let mut analyzer = index.tokenizer_for_field(fields.body)?;
    let mut whole = Vec::new();
    let mut stream = analyzer.token_stream(&request.text);
    while stream.advance() {
        whole.push(stream.token().text.clone());
    }
    let prefix = if request.prefix_last {
        whole.pop()
    } else {
        None
    };
    Ok(Words { whole, prefix })
}

/// Every word must match the title or the body; only live documents in the chosen namespaces.
fn build_query(fields: &Fields, words: &Words, namespaces: &NamespaceFilter) -> BooleanQuery {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    for word in &words.whole {
        clauses.push((
            Occur::Must,
            either_field(fields, |field| term_query(field, word)),
        ));
    }
    if let Some(prefix) = &words.prefix {
        clauses.push((
            Occur::Must,
            either_field(fields, |field| prefix_query(field, prefix)),
        ));
    }
    clauses.push((
        Occur::Must,
        filter(TermQuery::new(
            fields.kind_term(RowKind::Document),
            IndexRecordOption::Basic,
        )),
    ));
    if let NamespaceFilter::Only(chosen) = namespaces {
        clauses.push((Occur::Must, namespace_filter(fields, chosen)));
    }
    BooleanQuery::new(clauses)
}

fn either_field(fields: &Fields, query: impl Fn(Field) -> Box<dyn Query>) -> Box<dyn Query> {
    Box::new(BooleanQuery::new(vec![
        (Occur::Should, query(fields.title)),
        (Occur::Should, query(fields.body)),
    ]))
}

fn term_query(field: Field, word: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(
        Term::from_field_text(field, word),
        IndexRecordOption::WithFreqsAndPositions,
    ))
}

fn prefix_query(field: Field, prefix: &str) -> Box<dyn Query> {
    let mut query = PhrasePrefixQuery::new(vec![Term::from_field_text(field, prefix)]);
    query.set_max_expansions(MAX_PREFIX_EXPANSIONS);
    Box::new(query)
}

/// Filters narrow the results without changing their order.
fn filter(query: impl Query) -> Box<dyn Query> {
    Box::new(ConstScoreQuery::new(Box::new(query), 0.0))
}

fn namespace_filter(fields: &Fields, chosen: &[Namespace]) -> Box<dyn Query> {
    let any = chosen
        .iter()
        .map(|namespace| {
            let query = TermQuery::new(fields.namespace_term(namespace), IndexRecordOption::Basic);
            (Occur::Should, Box::new(query) as Box<dyn Query>)
        })
        .collect();
    filter(BooleanQuery::new(any))
}

/// Weights each matched body word; a prefix stands for the words it expands to.
///
/// Built by hand because Tantivy's prefix query omits its expansions from snippet
/// extraction, which left prefix matches with empty snippets (`docs/dev/trapstoknow.md`).
fn snippet_generator(
    index: &Index,
    searcher: &Searcher,
    fields: &Fields,
    words: &Words,
) -> Result<SnippetGenerator, FullTextError> {
    let mut frequencies: BTreeMap<String, u64> = BTreeMap::new();
    for word in &words.whole {
        let frequency = searcher.doc_freq(&Term::from_field_text(fields.body, word))?;
        frequencies.insert(word.clone(), frequency);
    }
    if let Some(prefix) = &words.prefix {
        expand_prefix(searcher, fields.body, prefix, &mut frequencies)?;
    }
    let weights = frequencies
        .into_iter()
        .filter(|(_, frequency)| *frequency > 0)
        .map(|(word, frequency)| (word, 1.0 / (1.0 + frequency as Score)))
        .collect();
    Ok(SnippetGenerator::new(
        weights,
        index.tokenizer_for_field(fields.body)?,
        fields.body,
        SNIPPET_BYTES,
    ))
}

fn expand_prefix(
    searcher: &Searcher,
    body: Field,
    prefix: &str,
    frequencies: &mut BTreeMap<String, u64>,
) -> Result<(), FullTextError> {
    for segment in searcher.segment_readers() {
        let inverted = segment.inverted_index(body)?;
        let mut stream = inverted
            .terms()
            .range()
            .ge(prefix.as_bytes())
            .into_stream()?;
        let mut expanded = 0;
        while expanded < MAX_PREFIX_EXPANSIONS
            && stream.advance()
            && stream.key().starts_with(prefix.as_bytes())
        {
            let word = std::str::from_utf8(stream.key()).map_err(|_| FullTextError::Damaged)?;
            *frequencies.entry(word.to_owned()).or_insert(0) += u64::from(stream.value().doc_freq);
            expanded += 1;
        }
    }
    Ok(())
}

fn hit(
    fields: &Fields,
    snippets: &SnippetGenerator,
    row: &TantivyDocument,
    score: Score,
) -> Result<FullTextHit, FullTextError> {
    let snippet = snippets.snippet_from_doc(row);
    let (text, highlights) = if snippet.fragment().is_empty() {
        // A title-only match still shows the start of its body, without highlights.
        (opening(fields.text(row, fields.body)?), Vec::new())
    } else {
        (
            snippet.fragment().to_owned(),
            snippet.highlighted().to_vec(),
        )
    };
    Ok(FullTextHit {
        namespace: Namespace::from_stored(fields.text(row, fields.namespace)?)?,
        id: fields.text(row, fields.id)?.to_owned(),
        revision: fields.number(row, fields.revision)?,
        score,
        title: fields.text(row, fields.title)?.to_owned(),
        snippet: text,
        highlights,
    })
}

/// The body's first bytes, cut back to a character boundary.
fn opening(body: &str) -> String {
    let mut end = body.len().min(SNIPPET_BYTES);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    body[..end].to_owned()
}
