//! Owns the certificate generator and TLS server, with bounded startup and reaping.

use super::{Duration, no_delivery};
use crate::linux::child_process::OwnedChild;
use crate::linux::fixture::LIMIT;
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::Command,
    thread,
    time::Instant,
};

pub(super) struct CertificateServer {
    child: OwnedChild,
    files: CertificateFiles,
    port: u16,
}

impl CertificateServer {
    pub(super) fn start(directory: PathBuf) -> Result<Self, String> {
        let files = CertificateFiles(directory);
        let directory = &files.0;
        let key = directory.join("synthetic-key.pem");
        // OpenSSL replaces the contents of this already-private inode, inside a mode-0700 root.
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&key)
            .map_err(|error| error.to_string())?;
        let mut generator = OwnedChild::spawn(
            Command::new("/usr/bin/openssl")
                .args([
                    "req",
                    "-x509",
                    "-newkey",
                    "rsa:2048",
                    "-noenc",
                    "-days",
                    "1",
                    "-subj",
                    "/CN=synthetic.invalid",
                    "-addext",
                    "subjectAltName=IP:127.0.0.1",
                    "-keyout",
                ])
                .arg(&key)
                .arg("-out")
                .arg(directory.join("synthetic-certificate.pem")),
        )?;
        let status = generator.wait(LIMIT)?;
        if !status.success() {
            return Err(
                "Synthetic certificate generation failed; key material is never logged".into(),
            );
        }
        if fs::metadata(&key)
            .map_err(|error| error.to_string())?
            .permissions()
            .mode()
            & 0o777
            != 0o600
        {
            return Err("Synthetic TLS private key permissions changed".into());
        }
        let child = OwnedChild::spawn(
            Command::new("python3")
                .args(["-c", include_str!("../../page_close/certificate_server.py")])
                .env("SYRINX_SYNTHETIC_CERT_DIRECTORY", directory),
        )?;
        let mut server = Self {
            child,
            files,
            port: 0,
        };
        let deadline = Instant::now() + LIMIT;
        while Instant::now() < deadline {
            if server.child.poll()?.is_some() {
                return Err("Synthetic TLS server exited before publishing its port".into());
            }
            match fs::read_to_string(server.files.0.join("port")) {
                Ok(port) if !port.is_empty() => {
                    server.port = port.parse().map_err(|_| "Invalid synthetic TLS port")?;
                    if server.port == 0 {
                        return Err("TLS server published port zero".into());
                    }
                    return Ok(server);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err("Synthetic TLS server startup timed out".into())
    }

    pub(super) fn url(&self) -> String {
        format!("https://127.0.0.1:{}/invalid", self.port)
    }

    pub(super) fn require_no_delivery(&mut self) -> Result<(), String> {
        if self.child.child.is_some() && self.child.poll()?.is_some() {
            return Err("TLS server exited before final live delivery inspection".into());
        }
        no_delivery(&self.files.0)
    }

    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.child.stop()?;
        self.files.remove()
    }
}

impl Drop for CertificateServer {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("TLS process cleanup: {error}");
        }
    }
}

struct CertificateFiles(PathBuf);

impl CertificateFiles {
    fn remove(&self) -> Result<(), String> {
        for name in ["synthetic-key.pem", "synthetic-certificate.pem"] {
            match fs::remove_file(self.0.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("Synthetic TLS artifact cleanup: {error}")),
            }
        }
        Ok(())
    }
}

impl Drop for CertificateFiles {
    fn drop(&mut self) {
        if let Err(error) = self.remove() {
            eprintln!("{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::fixture::reserve_profile;

    #[test]
    fn actual_tls_helper_rejects_default_client_and_reaps_owned_process() {
        let directory = reserve_profile().unwrap();
        let mut server = CertificateServer::start(directory.clone()).unwrap();
        assert_eq!(
            fs::metadata(directory.join("synthetic-key.pem"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let script = "import socket,ssl,sys\ntry:\n with socket.create_connection(('127.0.0.1',int(sys.argv[1])),timeout=3) as sock:\n  ssl.create_default_context().wrap_socket(sock,server_hostname='127.0.0.1')\nexcept ssl.SSLCertVerificationError:\n sys.exit(0)\nsys.exit(1)";
        let mut client = OwnedChild::spawn(Command::new("python3").args([
            "-c",
            script,
            &server.port.to_string(),
        ]))
        .unwrap();
        assert!(client.wait(Duration::from_secs(5)).unwrap().success());
        server.require_no_delivery().unwrap();
        server.stop().unwrap();
        server.require_no_delivery().unwrap();
        assert!(server.child.child.is_none());
        assert!(!directory.join("synthetic-key.pem").exists());
        assert!(!directory.join("synthetic-certificate.pem").exists());
        drop(server);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn delivery_marker_is_a_failure_even_without_native_page_events() {
        let directory = reserve_profile().unwrap();
        no_delivery(&directory).unwrap();
        fs::write(directory.join("delivered"), b"request\n").unwrap();
        assert!(no_delivery(&directory).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
