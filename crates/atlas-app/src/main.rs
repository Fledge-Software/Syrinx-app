//! Proves the Windows application executable can run before the browser shell is built.

#[cfg(not(target_os = "windows"))]
compile_error!("Atlas v1 supports Windows 11 only.");

fn main() {
    // The first scaffold has no window or owner data to initialize.
    println!("Atlas {}", env!("CARGO_PKG_VERSION"));
}
