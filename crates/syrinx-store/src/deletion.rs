//! Defines the content-free deletion log that an older backup must honor (D47).

use std::{error::Error, fmt};

const MAGIC: &[u8; 8] = b"SYRXDJ01";
const RECORD_SIZE: usize = 48;
const CHECKSUM_OFFSET: usize = RECORD_SIZE - 4;
const MAX_LOG_BYTES: usize = 16 * 1024 * 1024;

/// Initial bytes for an exclusively created journal; a missing or empty file is not a journal.
pub const EMPTY_DELETION_LOG: &[u8] = b"SYRINX-DELETE-01";

/// The length of one encoded record, so a writer can recognise a torn final append.
pub const DELETION_RECORD_BYTES: usize = RECORD_SIZE;

/// The only durable browsing category introduced by this contract; unknown kinds fail closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletionCategory {
    History,
}

/// A half-open interval of Unix seconds. Values contain no URLs or page contents.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DeletionRange {
    start: i64,
    end: i64,
}

impl fmt::Debug for DeletionRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeletionRange([redacted])")
    }
}

impl DeletionRange {
    pub fn new(start: i64, end: i64) -> Result<Self, DeletionError> {
        if start >= end {
            return Err(DeletionError::InvalidRange);
        }
        Ok(Self { start, end })
    }

    /// Every visit Syrinx can store (`visited_at >= 0`), for clearing all history.
    pub fn all_time() -> Self {
        Self {
            start: 0,
            end: i64::MAX,
        }
    }

    pub fn start(self) -> i64 {
        self.start
    }

    pub fn end(self) -> i64 {
        self.end
    }
}

/// Identifies a requested clear independently of the database being restored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeletionRequest {
    generation: u64,
    category: DeletionCategory,
    range: DeletionRange,
}

impl DeletionRequest {
    pub fn generation(self) -> u64 {
        self.generation
    }

    pub fn category(self) -> DeletionCategory {
        self.category
    }

    pub fn range(self) -> DeletionRange {
        self.range
    }
}

/// A request must be durable before deletion; completion follows all required participants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletionRecord {
    Requested(DeletionRequest),
    Completed(DeletionRequest),
}

impl DeletionRecord {
    /// Produces a fixed-size record for a guarded, durably flushed append.
    pub fn encode(self) -> [u8; RECORD_SIZE] {
        let (kind, request) = match self {
            Self::Requested(request) => (1, request),
            Self::Completed(request) => (2, request),
        };
        // Fixed fields and zero reserved bytes make incomplete or future formats detectable.
        let mut bytes = [0; RECORD_SIZE];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8] = kind;
        bytes[9] = match request.category {
            DeletionCategory::History => 1,
        };
        bytes[16..24].copy_from_slice(&request.generation.to_be_bytes());
        bytes[24..32].copy_from_slice(&request.range.start.to_be_bytes());
        bytes[32..40].copy_from_slice(&request.range.end.to_be_bytes());
        let checksum = checksum(&bytes[..CHECKSUM_OFFSET]);
        bytes[CHECKSUM_OFFSET..].copy_from_slice(&checksum.to_be_bytes());
        bytes
    }
}

/// Validated replay state. This type performs no filesystem or history collection operation.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct DeletionLog {
    requests: Vec<DeletionRequest>,
    completed_generation: u64,
}

impl fmt::Debug for DeletionLog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeletionLog")
            .field("requests", &self.requests.len())
            .field("completed_generation", &self.completed_generation)
            .finish_non_exhaustive()
    }
}

impl DeletionLog {
    /// Requires the external generation barrier, never a generation taken from an old backup.
    pub fn decode(bytes: &[u8], required_generation: u64) -> Result<Self, DeletionError> {
        if bytes.len() > MAX_LOG_BYTES {
            return Err(DeletionError::TooLarge);
        }
        let bytes = bytes
            .strip_prefix(EMPTY_DELETION_LOG)
            .ok_or(DeletionError::UnsupportedFormat)?;
        if !bytes.len().is_multiple_of(RECORD_SIZE) {
            return Err(DeletionError::DamagedRecord);
        }
        let mut log = Self::default();
        for record in bytes.chunks_exact(RECORD_SIZE) {
            log.accept(decode_record(record)?)?;
        }
        let last_generation = log.requests.last().map_or(0, |request| request.generation);
        if last_generation < required_generation {
            return Err(DeletionError::MissingGeneration);
        }
        Ok(log)
    }

    /// Prepares the next entry without acknowledging or applying an undurable request.
    pub fn request(
        &self,
        category: DeletionCategory,
        range: DeletionRange,
    ) -> Result<DeletionRecord, DeletionError> {
        if self.pending().is_some() {
            return Err(DeletionError::PendingClear);
        }
        // Reserve room for both records before permitting the request's durable append.
        let maximum_requests = (MAX_LOG_BYTES - EMPTY_DELETION_LOG.len()) / (2 * RECORD_SIZE);
        if self.requests.len() >= maximum_requests {
            return Err(DeletionError::TooLarge);
        }
        let generation = self
            .completed_generation
            .checked_add(1)
            .ok_or(DeletionError::GenerationExhausted)?;
        Ok(DeletionRecord::Requested(DeletionRequest {
            generation,
            category,
            range,
        }))
    }

    /// Prepares completion after the caller has successfully cleared every participant.
    pub fn completion(&self) -> Result<DeletionRecord, DeletionError> {
        self.pending()
            .map(DeletionRecord::Completed)
            .ok_or(DeletionError::NoPendingClear)
    }

    /// Applies a decoded or durably appended record; a failed append must not call this.
    pub fn accept(&mut self, record: DeletionRecord) -> Result<(), DeletionError> {
        match record {
            DeletionRecord::Requested(request) => self.accept_request(request),
            DeletionRecord::Completed(request) => self.accept_completion(request),
        }
    }

    pub fn completed_generation(&self) -> u64 {
        self.completed_generation
    }

    /// Returns an interrupted clear that must finish before new clears or restore exposure.
    pub fn pending(&self) -> Option<DeletionRequest> {
        self.requests
            .last()
            .copied()
            .filter(|request| request.generation > self.completed_generation)
    }

    /// The newest durable request's generation, whether completed or still pending.
    pub fn last_generation(&self) -> u64 {
        self.requests.last().map_or(0, |request| request.generation)
    }

    /// Lists every durable request a copy at `applied_generation` lacks, including a pending
    /// one: a requested clear is the owner's intent, so applying it early never over-deletes.
    pub fn unapplied(&self, applied_generation: u64) -> Result<&[DeletionRequest], DeletionError> {
        if applied_generation > self.last_generation() {
            return Err(DeletionError::MissingGeneration);
        }
        let applied =
            usize::try_from(applied_generation).map_err(|_| DeletionError::MissingGeneration)?;
        self.requests
            .get(applied..)
            .ok_or(DeletionError::MissingGeneration)
    }

    /// Lists completed clears that a staged candidate has not yet applied.
    pub fn replay_after(
        &self,
        applied_generation: u64,
    ) -> Result<&[DeletionRequest], DeletionError> {
        if self.pending().is_some() {
            return Err(DeletionError::PendingClear);
        }
        if applied_generation > self.completed_generation {
            return Err(DeletionError::MissingGeneration);
        }
        let applied =
            usize::try_from(applied_generation).map_err(|_| DeletionError::MissingGeneration)?;
        self.requests
            .get(applied..)
            .ok_or(DeletionError::MissingGeneration)
    }

    fn accept_request(&mut self, request: DeletionRequest) -> Result<(), DeletionError> {
        let expected = self.request(request.category, request.range)?;
        if expected != DeletionRecord::Requested(request) {
            return Err(DeletionError::MissingGeneration);
        }
        self.requests.push(request);
        Ok(())
    }

    fn accept_completion(&mut self, request: DeletionRequest) -> Result<(), DeletionError> {
        if self.pending() != Some(request) {
            return Err(DeletionError::CompletionMismatch);
        }
        self.completed_generation = request.generation;
        Ok(())
    }
}

/// Structural parsing and sequencing failures, without record contents or paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletionError {
    InvalidRange,
    DamagedRecord,
    UnsupportedFormat,
    UnsupportedCategory,
    TooLarge,
    MissingGeneration,
    GenerationExhausted,
    PendingClear,
    NoPendingClear,
    CompletionMismatch,
}

impl fmt::Display for DeletionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "deletion log validation failed: {self:?}")
    }
}

impl Error for DeletionError {}

fn decode_record(bytes: &[u8]) -> Result<DeletionRecord, DeletionError> {
    // Parse only a complete frame whose checksum and reserved fields match the known format.
    if bytes.len() != RECORD_SIZE || &bytes[..8] != MAGIC {
        return Err(DeletionError::UnsupportedFormat);
    }
    let expected = checksum(&bytes[..CHECKSUM_OFFSET]).to_be_bytes();
    if bytes[CHECKSUM_OFFSET..] != expected {
        return Err(DeletionError::DamagedRecord);
    }
    if bytes[10..16] != [0; 6] || bytes[40..44] != [0; 4] {
        return Err(DeletionError::UnsupportedFormat);
    }
    if bytes[9] != 1 {
        return Err(DeletionError::UnsupportedCategory);
    }
    let generation = u64::from_be_bytes(read_number(bytes, 16)?);
    let start = i64::from_be_bytes(read_number(bytes, 24)?);
    let end = i64::from_be_bytes(read_number(bytes, 32)?);
    let request = DeletionRequest {
        generation,
        category: DeletionCategory::History,
        range: DeletionRange::new(start, end)?,
    };
    match bytes[8] {
        1 => Ok(DeletionRecord::Requested(request)),
        2 => Ok(DeletionRecord::Completed(request)),
        _ => Err(DeletionError::UnsupportedFormat),
    }
}

fn read_number(bytes: &[u8], offset: usize) -> Result<[u8; 8], DeletionError> {
    bytes
        .get(offset..offset + 8)
        .ok_or(DeletionError::DamagedRecord)?
        .try_into()
        .map_err(|_| DeletionError::DamagedRecord)
}

fn checksum(bytes: &[u8]) -> u32 {
    // CRC-32 detects accidental damage; it is not an authentication or tamper-proofing mechanism.
    let mut value = u32::MAX;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _bit in 0..8 {
            let low_bit = value & 1;
            value = (value >> 1) ^ (0xedb8_8320 * low_bit);
        }
    }
    !value
}

#[cfg(test)]
mod tests;
