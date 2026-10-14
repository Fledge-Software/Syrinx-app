//! WinHTTP refuses an untrusted certificate. The server is P1-14's standard-library Python
//! fixture on 127.0.0.1, with a fresh self-signed certificate that is never installed.

use std::fs;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use syrinx_update::install::{InstallError, PackageSource, SignedFile, TransferProblem};

use crate::endpoint::{Endpoint, Timeouts};
use crate::remote::RemotePackage;
use crate::winhttp::WinHttp;

/// Windows' `CREATE_NO_WINDOW`, so the fixture never shows a console.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The fixture's Python server and certificate generator, shared with P1-14's native fixture.
const SERVER: &str =
    include_str!("../../syrinx-platform/examples/page_close/certificate_server.py");
const GENERATOR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../syrinx-platform/examples/page_close/certificate_server.ps1"
);

/// The fixture server, stopped and its fresh folder removed when dropped.
struct CertificateServer {
    child: Child,
    directory: PathBuf,
}

impl CertificateServer {
    fn start() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(r"..\..\target\syrinx-update-net-tests")
            .join(format!("tls-{}-{nanos}", std::process::id()));
        // A fresh name only: the test owns this folder and nothing outside it.
        fs::create_dir_all(directory.parent().expect("parent")).expect("test folder parent");
        fs::create_dir(&directory).expect("fresh test folder");
        let log = fs::File::create_new(directory.join("server.log")).expect("server log");
        let errors = log.try_clone().expect("server log");
        let child = Command::new("python.exe")
            .args(["-c", SERVER])
            .env("SYRINX_SYNTHETIC_CERT_DIRECTORY", &directory)
            .env("SYRINX_SYNTHETIC_CERT_GENERATOR", GENERATOR)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(errors)
            .spawn()
            .expect("start the synthetic TLS server (python.exe on PATH)");
        Self { child, directory }
    }

    /// The port the server published, once it is listening.
    fn port(&mut self) -> u16 {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let exited = self.child.try_wait().expect("poll the TLS server");
            assert!(
                exited.is_none(),
                "the TLS server stopped; see its server.log"
            );
            match fs::read_to_string(self.directory.join("port")) {
                Ok(port) if !port.is_empty() => return port.parse().expect("a port number"),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => panic!("read the TLS server's port: {error}"),
            }
            assert!(Instant::now() < deadline, "the TLS server did not start");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn delivered(&self) -> bool {
        self.directory.join("delivered").exists()
    }
}

impl Drop for CertificateServer {
    fn drop(&mut self) {
        let stopped = match self.child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => self.child.kill().and_then(|()| self.child.wait().map(drop)),
            Err(error) => Err(error),
        };
        let removed = fs::remove_dir_all(&self.directory);
        if thread::panicking() {
            return;
        }
        stopped.expect("stop the TLS server");
        removed.expect("remove the test's own folder");
    }
}

#[test]
fn an_untrusted_certificate_is_refused_before_any_request() {
    let mut server = CertificateServer::start();
    let timeouts = Timeouts {
        connect: Duration::from_secs(5),
        receive: Duration::from_secs(5),
    };
    let endpoint = Endpoint::loopback(server.port(), true, timeouts);
    let transport = WinHttp::open(endpoint).expect("WinHTTP session");
    let package = RemotePackage::with_transport(Box::new(transport), endpoint.host());
    let refused = package.read_signed(SignedFile::Statement, 4096);
    assert_eq!(
        refused,
        Err(InstallError::Transfer(TransferProblem::Certificate))
    );
    assert!(
        !server.delivered(),
        "no request crossed the refused handshake"
    );
}
