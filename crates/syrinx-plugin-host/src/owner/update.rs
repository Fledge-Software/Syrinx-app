//! Applying a reviewed plugin update as one step (#140): the new version replaces the installed
//! one with its record, grants and registration, or, if any step fails, the installed version
//! is put back in full.

use std::collections::BTreeMap;
use std::sync::Arc;

use syrinx_core::CommandRegistry;
use syrinx_store::{
    FolderNarrowing, PluginFolder, PluginGrant, PluginInstall, PluginState, StoreError, StoredGrant,
};

use super::install::{
    Base, InstallRefusal, Offer, Plan, Reviewed, check_required, install_record, is_newer,
    same_install,
};
use super::{OwnerError, OwnerRuntime, requested_scope, scope_json};
use crate::manifest::{CapabilityKind, FolderCount, PluginManifest, Scope};
use crate::registry::{PluginRegistry, RegisteredPlugin, RegistrationError};

/// The installed version as an update found it: everything a failure must put back.
struct Previous {
    record: PluginInstall,
    grants: Vec<StoredGrant>,
    folders: Vec<PluginFolder>,
    component: Arc<[u8]>,
    /// Its registration, withdrawn so the new version can take the id; `None` if it was not
    /// registered, as after a refused compile.
    registered: Option<RegisteredPlugin>,
    /// Whether it was loaded, so a failure loads it again; one that was not stays unloaded.
    was_loaded: bool,
}

impl OwnerRuntime {
    /// Replace the installed version with a reviewed update, as one step: a held update only
    /// once accepted, and only while the install record is still the one it was reviewed
    /// against. The installed version need not be running: one withdrawn after a refused
    /// compile can be updated too. The new version must compile against its manifest before
    /// anything is written. Its record replaces the old one, its grants are rescoped to what it
    /// asks for (folders granted under them narrowed to match), grants of dropped requests are
    /// deleted, and new capabilities are granted only as the owner chose. Every kind the new
    /// version requires must be granted already or granted now, or the update is refused with
    /// `RequiredDeclined` before anything is written. The record keeps its state, so a disabled
    /// plugin is updated but not enabled, registered or started. If any step fails,
    /// the old version's registration, record, grants and folders are put back, and it is
    /// loaded again if it was loaded.
    ///
    /// The old version's compiled code is deleted before the new one loads, so the new version
    /// never starts from the old code. The cache already keeps one entry per plugin and checks
    /// it against the record, so this is defence in depth.
    pub fn update_reviewed(
        &mut self,
        reviewed: Reviewed,
        granted: &[CapabilityKind],
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
        now: i64,
    ) -> Result<(), InstallRefusal> {
        match &reviewed.plan {
            Plan::Update(_) => {}
            Plan::Held(_) => return Err(InstallRefusal::NeedsConsent),
            Plan::Install => return Err(InstallRefusal::WrongPlan),
        }
        let id = reviewed.manifest().id().to_owned();
        let grants = self.stored_grants(&id)?;
        let stored: Vec<CapabilityKind> = grants
            .iter()
            .filter_map(|stored| CapabilityKind::from_name(&stored.grant.capability))
            .collect();
        // Every required kind must end up granted: already held, or granted now.
        let available: Vec<CapabilityKind> = stored.iter().chain(granted).copied().collect();
        check_required(reviewed.manifest(), &available, |_| true)?;
        // The owner's new choices: kinds the new version asks for that are not held yet.
        let requested = requested_kinds(reviewed.manifest());
        let new_grants: Vec<CapabilityKind> = granted
            .iter()
            .copied()
            .filter(|kind| requested.contains(kind) && !stored.contains(kind))
            .collect();
        let Reviewed {
            signed,
            offer,
            base,
            ..
        } = reviewed;
        let base = base.ok_or(InstallRefusal::WrongPlan)?;
        self.check_base(&base, signed.manifest().version())?;
        let previous = self.previous(&id, base, grants, registry, commands)?;
        let enabled = previous.record.state == PluginState::Enabled;

        if let Err(error) = registry.register(signed, commands) {
            self.restore(&id, previous, registry, commands);
            return Err(error.into());
        }
        if let Err(error) = self.swap(&id, &offer, &previous, &new_grants, registry, now) {
            self.restore(&id, previous, registry, commands);
            return Err(error);
        }
        self.commit_grants(&id, &requested, &new_grants);
        // As at start-up, a plugin that is not enabled offers no commands, tabs or feeds.
        if !enabled {
            registry.withdraw(&id, commands);
        }
        Ok(())
    }

    /// The grants the store holds for `plugin`.
    fn stored_grants(&self, plugin: &str) -> Result<Vec<StoredGrant>, InstallRefusal> {
        Ok(self
            .store
            .plugin_grants()?
            .into_iter()
            .filter(|stored| stored.plugin_id == plugin)
            .collect())
    }

    /// The install record must still be the one the update was reviewed against, and the
    /// update newer than it.
    fn check_base(&self, base: &Base, offered: &str) -> Result<(), InstallRefusal> {
        let id = &base.record.plugin_id;
        let current = self
            .record(id)?
            .ok_or_else(|| OwnerError::NotInstalled(id.clone()))?;
        if !same_install(&base.record, &current) {
            return Err(InstallRefusal::Stale);
        }
        // Defence in depth: the review already refused an update that is not newer.
        if !is_newer(offered, &current.version) {
            return Err(InstallRefusal::NotNewer);
        }
        Ok(())
    }

    /// Everything the installed version holds, its `grants` read already, with its registration
    /// withdrawn so the new version can take the id.
    fn previous(
        &self,
        plugin: &str,
        base: Base,
        grants: Vec<StoredGrant>,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) -> Result<Previous, InstallRefusal> {
        let folders = self.store.plugin_folders(plugin)?;
        Ok(Previous {
            record: base.record,
            grants,
            folders,
            component: base.component,
            registered: registry.withdraw(plugin, commands),
            was_loaded: self.added.contains_key(plugin),
        })
    }

    /// Check the new component, write the new record and grants, then, if the plugin is
    /// enabled, load it in place of the installed version. The caller puts everything back if
    /// this fails.
    fn swap(
        &mut self,
        plugin: &str,
        offer: &Offer,
        previous: &Previous,
        new_grants: &[CapabilityKind],
        registry: &PluginRegistry,
        now: i64,
    ) -> Result<(), InstallRefusal> {
        let registered = registry
            .get(plugin)
            .ok_or_else(|| RegistrationError::UnknownPlugin(plugin.to_owned()))?;
        self.check_component(registered, &offer.component)?;
        let record = PluginInstall {
            // The owner's choice stands: an update never enables a disabled plugin.
            state: previous.record.state,
            ..install_record(
                plugin,
                offer,
                registered.manifest(),
                previous.record.installed_at,
                now.max(previous.record.installed_at),
            )
        };
        self.store.save_plugin_install(record)?;
        self.rescope_grants(
            plugin,
            registered.manifest(),
            offer.manifest_sha256,
            previous,
        )?;
        for kind in new_grants {
            let scope = requested_scope(registered.manifest(), *kind)?;
            let grant = PluginGrant {
                capability: kind.name().to_owned(),
                scope_json: scope_json(scope),
                granted_at: now,
                granted_for_manifest: offer.manifest_sha256,
            };
            self.store.grant_plugin(plugin.to_owned(), grant)?;
        }
        self.stop(plugin)?;
        if let Some(cache) = self.cache.as_ref() {
            cache.delete_entry_or_report(plugin);
        }
        // A plugin that is not enabled is updated in place, but not started.
        if previous.record.state != PluginState::Enabled {
            return Ok(());
        }
        Ok(self.load(registered, Arc::clone(&offer.component))?)
    }

    /// Compile a component against its registered manifest on the workers' runtime.
    fn check_component(
        &mut self,
        registered: &RegisteredPlugin,
        component: &[u8],
    ) -> Result<(), InstallRefusal> {
        let plugins = &self.running()?.plugins;
        Ok(plugins.check_component(registered, component)?)
    }

    /// Keep each stored grant the new manifest still asks for, rescoped to what it asks for
    /// now, its folders narrowed to match; delete the rest. Widening happens only here and
    /// only after the owner accepted it.
    fn rescope_grants(
        &self,
        plugin: &str,
        manifest: &PluginManifest,
        manifest_sha256: [u8; 32],
        previous: &Previous,
    ) -> Result<(), InstallRefusal> {
        for stored in &previous.grants {
            let Some(kind) = CapabilityKind::from_name(&stored.grant.capability) else {
                continue;
            };
            let Ok(scope) = requested_scope(manifest, kind) else {
                self.store
                    .revoke_plugin_grant(plugin.to_owned(), stored.grant.capability.clone())?;
                continue;
            };
            let rescoped = PluginGrant {
                scope_json: scope_json(scope),
                granted_for_manifest: manifest_sha256,
                ..stored.grant.clone()
            };
            self.store.grant_plugin(plugin.to_owned(), rescoped)?;
            if let Some(narrowing) = folder_narrowing(stored.grant_id, scope) {
                self.store
                    .narrow_plugin_folders(plugin.to_owned(), narrowing)?;
            }
        }
        Ok(())
    }

    /// The new version is in place: the grants the workers check follow the store, whether or
    /// not it was started, keeping only the kinds it `requested`. No worker needs telling:
    /// `load` added the new version fresh, so no instance holds an old grant.
    fn commit_grants(
        &mut self,
        plugin: &str,
        requested: &[CapabilityKind],
        new_grants: &[CapabilityKind],
    ) {
        for kind in self.grants.kinds(plugin) {
            if !requested.contains(&kind) {
                self.grants.revoke(plugin, kind);
            }
        }
        for kind in new_grants {
            self.grants.grant(plugin, *kind);
        }
    }

    /// Put the installed version back after a failed update: its registration, record, grants
    /// and folders, and, if it was loaded, its component. Each failure to do so is reported.
    fn restore(
        &mut self,
        plugin: &str,
        previous: Previous,
        registry: &mut PluginRegistry,
        commands: &mut CommandRegistry,
    ) {
        // `stop` forgets a plugin only once its worker is gone, and a failed load adds nothing,
        // so a plugin still added here is the old version, still loaded.
        let still_loaded = self.added.contains_key(plugin);
        if !still_loaded {
            self.drop_stray_slot(plugin);
        }
        registry.withdraw(plugin, commands);
        if let Err(error) = self.store.save_plugin_install(previous.record) {
            eprintln!("a failed plugin update could not restore its record: {error}");
        }
        if let Err(error) = self.restore_grants(plugin, &previous.grants, &previous.folders) {
            eprintln!("a failed plugin update could not restore its grants: {error}");
        }
        let Some(registered) = previous.registered else {
            return;
        };
        let registered = match registry.reinstate(registered, commands) {
            Ok(registered) => registered,
            Err(error) => {
                eprintln!("a failed plugin update could not register the old version: {error}");
                return;
            }
        };
        if still_loaded || !previous.was_loaded {
            return;
        }
        if let Err(error) = self.load(registered, previous.component) {
            eprintln!("a failed plugin update could not load the old version: {error}");
        }
    }

    /// Forget a slot a failed load left behind, so the old version can be added again.
    fn drop_stray_slot(&mut self, plugin: &str) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        if running.plugins.status(plugin).is_none() {
            return;
        }
        if let Err(error) = running.plugins.uninstall(plugin) {
            eprintln!("a failed plugin update could not stop the new version: {error}");
        }
    }

    /// Make the stored grants and folders exactly `grants` and `folders` again.
    fn restore_grants(
        &self,
        plugin: &str,
        grants: &[StoredGrant],
        folders: &[PluginFolder],
    ) -> Result<(), StoreError> {
        let stored = self.store.plugin_grants()?;
        let extra = stored.iter().filter(|stored| {
            stored.plugin_id == plugin
                && !grants
                    .iter()
                    .any(|kept| kept.grant.capability == stored.grant.capability)
        });
        for stored in extra {
            self.store
                .revoke_plugin_grant(plugin.to_owned(), stored.grant.capability.clone())?;
        }
        let mut grant_ids = BTreeMap::new();
        for kept in grants {
            let grant_id = self
                .store
                .grant_plugin(plugin.to_owned(), kept.grant.clone())?;
            grant_ids.insert(kept.grant_id, grant_id);
        }
        for folder in folders {
            let Some(grant_id) = grant_ids.get(&folder.grant_id) else {
                continue;
            };
            let restored = PluginFolder {
                grant_id: *grant_id,
                ..folder.clone()
            };
            self.store.add_plugin_folder(plugin.to_owned(), restored)?;
        }
        Ok(())
    }
}

/// Every kind `manifest` asks for.
fn requested_kinds(manifest: &PluginManifest) -> Vec<CapabilityKind> {
    manifest
        .capabilities()
        .iter()
        .map(|request| request.kind)
        .collect()
}

/// How the folders granted under `grant_id` narrow to a folder scope; `None` for any other
/// scope.
fn folder_narrowing(grant_id: i64, scope: &Scope) -> Option<FolderNarrowing> {
    let Scope::Folder {
        writable, count, ..
    } = scope
    else {
        return None;
    };
    let keep = match count {
        FolderCount::Limited(limit) => Some(*limit),
        FolderCount::Many => None,
    };
    Some(FolderNarrowing {
        grant_id,
        read_only: !writable,
        keep,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_scope_narrows_its_folders_and_nothing_else_does() {
        let read_two = Scope::Folder {
            writable: false,
            count: FolderCount::Limited(2),
            watch: false,
        };
        assert_eq!(
            folder_narrowing(7, &read_two),
            Some(FolderNarrowing {
                grant_id: 7,
                read_only: true,
                keep: Some(2)
            })
        );
        assert_eq!(folder_narrowing(7, &Scope::None), None);
    }
}
