//! The owner's grants under the workers (#136): checked on every gated call, required ones
//! before activation, and a revocation taking effect before the plugin runs again.

use std::time::Instant;

use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::manifest::CapabilityKind;
use syrinx_plugin_host::session::CallResult;
use syrinx_plugin_host::test_host::{SyntheticGrants, SyntheticHost};
use syrinx_plugin_host::worker::{PluginStatus, Plugins, StopReason};
use syrinx_plugin_host::{HostError, PluginHost};

use super::services::{Journal, TestServices};
use super::{WAIT, add, answer, granted_plugins, instance_state, run, submit_pending};

const OPTIONAL_STORAGE: &str = "[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = false\n";
const REQUIRED_STORAGE: &str = "[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = true\n";

/// Run `plugin`'s `echo` command, which writes and reads one setting.
fn echo<S: PluginHost>(
    plugins: &mut Plugins<S>,
    plugin: &str,
) -> CallResult<Result<(), PluginError>> {
    let command = format!("{plugin}.echo");
    answer(plugins.call(plugin, move |session| session.run_command(&command)))
}

#[test]
fn a_plugin_missing_a_required_capability_is_not_activated_until_it_is_granted() {
    let grants = SyntheticGrants::new();
    let (mut plugins, _events) = granted_plugins(&grants, |_manifest| SyntheticHost::new());
    add(&mut plugins, "test", REQUIRED_STORAGE);
    let refused = plugins.call("test", |session| session.run_command("test.echo"));
    assert!(
        matches!(refused, Err(HostError::NotGranted(CapabilityKind::Storage))),
        "refused before any code ran"
    );
    assert_eq!(plugins.status("test"), Some(PluginStatus::Inactive));
    grants.grant("test", CapabilityKind::Storage);
    assert_eq!(run(&mut plugins, "test.echo"), Ok(Ok(())));
}

#[test]
fn a_revoked_required_capability_keeps_the_plugin_inactive_until_granted_again() {
    let grants = SyntheticGrants::new();
    grants.grant("test", CapabilityKind::Storage);
    let (mut plugins, _events) = granted_plugins(&grants, |_manifest| SyntheticHost::new());
    add(&mut plugins, "test", REQUIRED_STORAGE);
    assert_eq!(run(&mut plugins, "test.echo"), Ok(Ok(())));
    grants.revoke("test", CapabilityKind::Storage);
    plugins
        .revoke("test", CapabilityKind::Storage)
        .expect("revoked");
    assert!(matches!(
        plugins.call("test", |session| session.run_command("test.echo")),
        Err(HostError::NotGranted(CapabilityKind::Storage))
    ));
    grants.grant("test", CapabilityKind::Storage);
    assert_eq!(run(&mut plugins, "test.echo"), Ok(Ok(())));
}

#[test]
fn every_settings_call_checks_the_grant_of_the_calling_plugin_only() {
    let grants = SyntheticGrants::new();
    grants.grant("test", CapabilityKind::Storage);
    let (mut plugins, _events) = granted_plugins(&grants, |_manifest| SyntheticHost::new());
    add(&mut plugins, "test", OPTIONAL_STORAGE);
    add(&mut plugins, "other", OPTIONAL_STORAGE);
    assert_eq!(echo(&mut plugins, "test"), Ok(Ok(())));
    assert!(
        matches!(echo(&mut plugins, "other"), Ok(Err(PluginError::Denied(_)))),
        "another plugin's grant is not this one's"
    );

    // The grant store alone stops the next call, before the supervisor hears of it.
    grants.revoke("test", CapabilityKind::Storage);
    assert!(matches!(
        echo(&mut plugins, "test"),
        Ok(Err(PluginError::Revoked(_)))
    ));

    // A fresh instance never held it, so it is denied, not revoked.
    plugins
        .deactivate("test", StopReason::Shutdown)
        .expect("stopped");
    assert!(matches!(
        echo(&mut plugins, "test"),
        Ok(Err(PluginError::Denied(_)))
    ));
}

#[test]
fn revoking_an_optional_capability_abandons_its_work_once_and_tells_the_plugin() {
    let journal = Journal::default();
    let grants = SyntheticGrants::new();
    grants.grant("test", CapabilityKind::Storage);
    let (mut plugins, events) = granted_plugins(&grants, TestServices::recording(&journal));
    add(&mut plugins, "test", OPTIONAL_STORAGE);
    assert_eq!(echo(&mut plugins, "test"), Ok(Ok(())));
    let abandoned = submit_pending(&mut plugins);
    let kept = submit_pending(&mut plugins);
    journal.work_on(abandoned);

    grants.revoke("test", CapabilityKind::Storage);
    plugins
        .revoke("test", CapabilityKind::Storage)
        .expect("revoked");

    // The idle worker is woken: the services hear of it with no further call.
    let started = Instant::now();
    while journal.count("revoked storage") == 0 {
        assert!(
            started.elapsed() < WAIT,
            "the revocation never reached the services"
        );
        std::thread::yield_now();
    }
    assert!(matches!(
        echo(&mut plugins, "test"),
        Ok(Err(PluginError::Revoked(_)))
    ));
    let delivered = format!("deliver {abandoned} uncertain the capability was revoked");
    assert_eq!(journal.count(&delivered), 1, "told once, never resubmitted");
    assert_eq!(journal.count("revoked storage"), 1, "abandoned once");
    let (_, in_flight) = instance_state(&mut plugins);
    assert_eq!(in_flight, [kept], "work the services kept is untouched");
    assert_eq!(plugins.status("test"), Some(PluginStatus::Active));
    assert!(
        events.try_recv().is_err(),
        "the plugin was told; the host settled nothing itself"
    );
}
