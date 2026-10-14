//! The update host as a `PackageSource`: fixed paths under the compiled-in base, size bounds on
//! every body, and cancellation between reads.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use syrinx_update::install::{InstallError, PackageSource, SignedFile, TransferProblem};
use syrinx_update::{FileEntry, Version};

use crate::endpoint::Endpoint;
use crate::winhttp::{Body, Transport, WinHttp};

/// Bytes asked of WinHTTP per read.
const READ_CHUNK: usize = 64 * 1024;

/// Stops a check or download at its next read; shared with the owner's Cancel button.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn check(&self) -> Result<(), TransferProblem> {
        if self.0.load(Ordering::SeqCst) {
            return Err(TransferProblem::Cancelled);
        }
        Ok(())
    }
}

/// One owner-pressed check and its download, from the update host.
///
/// The signed files are fetched once and kept, so the download stages exactly the statement
/// and manifest the owner was shown.
pub struct RemotePackage {
    transport: Box<dyn Transport>,
    host: &'static str,
    cancel: Cancel,
    fetched: RefCell<BTreeMap<SignedFile, Vec<u8>>>,
}

impl RemotePackage {
    /// The compiled-in host. Opening the session makes no network call.
    pub fn production() -> Result<Self, InstallError> {
        let endpoint = Endpoint::production();
        let transport = WinHttp::open(endpoint).map_err(InstallError::Transfer)?;
        Ok(Self::with_transport(Box::new(transport), endpoint.host()))
    }

    pub(crate) fn with_transport(transport: Box<dyn Transport>, host: &'static str) -> Self {
        Self {
            transport,
            host,
            cancel: Cancel::default(),
            fetched: RefCell::new(BTreeMap::new()),
        }
    }

    /// The host's name, which Settings shows next to the button.
    pub fn host(&self) -> &'static str {
        self.host
    }

    /// A handle that cancels this check or download from another thread.
    pub fn canceller(&self) -> Cancel {
        self.cancel.clone()
    }

    /// Fetch a small file whole, refusing a body longer than `limit`.
    fn fetch(&self, path: &str, limit: usize) -> Result<Vec<u8>, TransferProblem> {
        self.cancel.check()?;
        let mut body = self.transport.get(path)?;
        if body.length().is_some_and(|length| length > limit as u64) {
            return Err(TransferProblem::TooLong);
        }
        let mut bytes = Vec::new();
        let mut chunk = vec![0; READ_CHUNK];
        loop {
            self.cancel.check()?;
            let read = body.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            if bytes.len() + read > limit {
                return Err(TransferProblem::TooLong);
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        if body
            .length()
            .is_some_and(|length| length != bytes.len() as u64)
        {
            return Err(TransferProblem::Truncated);
        }
        Ok(bytes)
    }
}

/// The path of a signed file under the base; built only from fixed names and a checked version.
fn signed_path(file: SignedFile) -> String {
    match file {
        SignedFile::Statement | SignedFile::StatementSignature => file.name().to_owned(),
        SignedFile::Manifest(version) | SignedFile::ManifestSignature(version) => {
            format!("{version}/{}", file.name())
        }
    }
}

/// A payload file's path: the version and a manifest path that passed P1-38's path rules.
fn payload_path(version: Version, entry: &FileEntry) -> String {
    format!("{version}/files/{}", entry.path().as_str())
}

impl PackageSource for RemotePackage {
    fn read_signed(&self, file: SignedFile, limit: usize) -> Result<Vec<u8>, InstallError> {
        if let Some(bytes) = self.fetched.borrow().get(&file) {
            return Ok(bytes.clone());
        }
        let bytes = self
            .fetch(&signed_path(file), limit)
            .map_err(InstallError::Transfer)?;
        self.fetched.borrow_mut().insert(file, bytes.clone());
        Ok(bytes)
    }

    fn open_file(
        &self,
        version: Version,
        entry: &FileEntry,
    ) -> Result<Box<dyn Read + '_>, InstallError> {
        let refused = InstallError::Transfer;
        self.cancel.check().map_err(refused)?;
        let body = self
            .transport
            .get(&payload_path(version, entry))
            .map_err(refused)?;
        let expected = entry.size();
        match body.length() {
            Some(length) if length > expected => return Err(refused(TransferProblem::TooLong)),
            Some(length) if length < expected => {
                return Err(refused(TransferProblem::Truncated));
            }
            _ => {}
        }
        Ok(Box::new(Download {
            body,
            remaining: expected,
            cancel: self.cancel.clone(),
        }))
    }
}

/// A payload body that must be exactly its listed size; it never yields a byte past it.
struct Download<'a> {
    body: Box<dyn Body + 'a>,
    remaining: u64,
    cancel: Cancel,
}

impl Download<'_> {
    fn read_body(&mut self, buffer: &mut [u8]) -> Result<usize, TransferProblem> {
        self.cancel.check()?;
        if self.remaining == 0 {
            // One more read tells a finished body from a longer one.
            let mut probe = [0u8; 1];
            return match self.body.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(TransferProblem::TooLong),
            };
        }
        let wanted = buffer
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let read = self.body.read(&mut buffer[..wanted])?;
        if read == 0 {
            return Err(TransferProblem::Truncated);
        }
        self.remaining -= read as u64;
        Ok(read)
    }
}

impl Read for Download<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.read_body(buffer).map_err(TransferProblem::into_io)
    }
}
