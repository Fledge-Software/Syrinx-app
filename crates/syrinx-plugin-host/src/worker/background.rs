//! Compiles a plugin on its own thread before its first activation, so the compiled-code
//! cache (#137) is warm when the plugin is first used. Built only with the `store` feature.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::manifest::PluginManifest;
use crate::session::CallResult;
use crate::{HostError, PluginHost, Runtime};

use super::thread::{self, Prepared};
use super::{PluginEvent, Plugins, join_compile, send_event};

impl<S: PluginHost> Plugins<S> {
    /// Compile `plugin` on a background thread when the runtime's cache has no current entry
    /// for it, so its first activation loads it instead of compiling. An activation that
    /// starts meanwhile waits for this compile rather than compiling twice. Returns at once;
    /// without a cache nothing is compiled. A thread that cannot start is reported as
    /// `PluginEvent::PrepareFailed`, and the first activation compiles instead.
    pub(crate) fn prepare_in_background(&mut self, plugin: &str) -> CallResult<()> {
        let slot = self
            .slots
            .get(plugin)
            .ok_or_else(|| HostError::Refused(format!("{plugin} is not installed")))?;
        if !self.runtime.has_cache() {
            return Ok(());
        }
        let background = Background {
            runtime: self.runtime.clone(),
            manifest: slot.manifest.clone(),
            component: Arc::clone(&slot.component),
            prepared: Arc::clone(&slot.prepared),
            events: self.events.clone(),
        };
        self.join_finished_compiles();
        let spawned = std::thread::Builder::new()
            .name(format!("syrinx-prepare-{plugin}"))
            .spawn(move || background.run());
        match spawned {
            Ok(thread) => self.preparing.push((plugin.to_owned(), thread)),
            Err(error) => {
                let error = HostError::Unavailable(format!("no compile thread: {error}"));
                let plugin = plugin.to_owned();
                send_event(&self.events, PluginEvent::PrepareFailed { plugin, error });
            }
        }
        Ok(())
    }

    /// Join each background compile that has finished, reporting one that panicked.
    fn join_finished_compiles(&mut self) {
        let (finished, running) = std::mem::take(&mut self.preparing)
            .into_iter()
            .partition(|(_, thread)| thread.is_finished());
        self.preparing = running;
        for (plugin, thread) in finished {
            join_compile(&self.events, plugin, thread);
        }
    }
}

/// One plugin's compile ahead of its first activation, run on its own thread.
struct Background<S: 'static> {
    runtime: Runtime,
    manifest: PluginManifest,
    component: Arc<[u8]>,
    prepared: Prepared<S>,
    events: Sender<PluginEvent>,
}

impl<S: PluginHost> Background<S> {
    /// Compile the plugin unless the cache is current for it; the cache reports its own
    /// problems, and a failed compile is reported here.
    fn run(self) {
        if !self
            .runtime
            .needs_compiling(&self.manifest, &self.component)
        {
            return;
        }
        let compiled = thread::prepare(
            &self.runtime,
            &self.manifest,
            &self.component,
            &self.prepared,
        );
        if let Err(error) = compiled {
            let plugin = self.manifest.id().to_owned();
            send_event(&self.events, PluginEvent::PrepareFailed { plugin, error });
        }
    }
}
