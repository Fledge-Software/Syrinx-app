//! A report read back from its exact text, so the preview's table shows what would leave (AN-02).

/// Which period a report covers: the one being counted, or the last completed one.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReportPeriod {
    Current,
    Previous,
}

/// Every field of one report, in the order its text lists them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportTable {
    /// The envelope's fields and their values as written, text unquoted.
    pub envelope: Vec<(String, String)>,
    /// `counts`, `commands`, `timings` and `errors`, in that order.
    pub groups: Vec<ReportGroup>,
}

/// One top-level group of a report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportGroup {
    pub key: String,
    pub rows: Vec<ReportRow>,
}

/// One field of a group: a single count, or a timing's buckets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRow {
    pub key: String,
    pub value: ReportValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReportValue {
    Count(u64),
    /// Each bucket's name and count, in the schema's order.
    Buckets(Vec<(String, u64)>),
}

impl ReportTable {
    /// The envelope value written under `key`, if the report has it.
    pub fn envelope_value(&self, key: &str) -> Option<&str> {
        self.envelope
            .iter()
            .find(|(field, _)| field == key)
            .map(|(_, value)| value.as_str())
    }
}
