//! Sets the engine's process-wide options while Syrinx still has a single thread.

use std::path::Path;

/// WebKit's renderer switch; its DMA-BUF renderer draws nothing on NVIDIA's own driver.
const DMABUF_SWITCH: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// Present only while NVIDIA's proprietary kernel driver is loaded.
const NVIDIA_DRIVER: &str = "/proc/driver/nvidia/version";

/// Turns WebKit's DMA-BUF renderer off on NVIDIA's driver, where pages otherwise stay black
/// ("Failed to create GBM buffer"); an owner's own setting of the switch is left alone.
///
/// # Safety
/// Changes the environment, so it must run before any other thread exists.
pub(crate) unsafe fn prepare() {
    if !Path::new(NVIDIA_DRIVER).exists() || std::env::var_os(DMABUF_SWITCH).is_some() {
        return;
    }
    // SAFETY: the caller guarantees no other thread can read the environment concurrently.
    unsafe { std::env::set_var(DMABUF_SWITCH, "1") };
}
