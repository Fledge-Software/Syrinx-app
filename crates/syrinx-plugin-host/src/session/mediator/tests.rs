//! The mediator's settings rules, called as a plugin's import would call them.

use std::sync::Arc;

use super::Mediator;
use crate::bindings::syrinx::plugin::host::Host;
use crate::bindings::syrinx::plugin::types::PluginError;
use crate::limits;
use crate::manifest::CapabilityKind;
use crate::session::Declarations;
use crate::test_host::{SyntheticGrants, SyntheticHost};

fn mediator(plugin: &str, grants: &SyntheticGrants) -> Mediator<SyntheticHost> {
    let declarations = Declarations {
        plugin_id: plugin.to_owned(),
        ..Declarations::default()
    };
    Mediator::new(SyntheticHost::new(), declarations, Arc::new(grants.clone()))
}

fn invalid<T: std::fmt::Debug>(result: Result<T, PluginError>) -> bool {
    matches!(result, Err(PluginError::InvalidInput(_)))
}

#[test]
fn settings_keys_and_values_are_bounded_before_any_service_sees_them() {
    let grants = SyntheticGrants::new();
    grants.grant("first", CapabilityKind::Storage);
    let mut first = mediator("first", &grants);
    let long_key = "k".repeat(limits::MAX_SETTINGS_KEY_BYTES + 1);
    assert!(invalid(first.settings_set(long_key.clone(), Vec::new())));
    assert!(invalid(first.settings_get(long_key)));
    assert!(invalid(first.settings_set("Not An Id".into(), Vec::new())));
    let big = vec![0; limits::MAX_SETTINGS_VALUE_BYTES + 1];
    assert!(invalid(first.settings_set("first.big".into(), big)));
    let largest = vec![0; limits::MAX_SETTINGS_VALUE_BYTES];
    assert_eq!(first.settings_set("first.big".into(), largest), Ok(()));
}

#[test]
fn a_grant_to_one_plugin_is_not_a_grant_to_another() {
    let grants = SyntheticGrants::new();
    grants.grant("first", CapabilityKind::Storage);
    let mut first = mediator("first", &grants);
    let mut second = mediator("second", &grants);
    assert_eq!(
        first.settings_set("shared.key".into(), b"one".to_vec()),
        Ok(())
    );
    assert!(matches!(
        second.settings_get("shared.key".into()),
        Err(PluginError::Denied(_))
    ));
}

#[test]
fn a_kind_held_at_activation_answers_revoked_once_withdrawn() {
    let grants = SyntheticGrants::new();
    grants.grant("first", CapabilityKind::Storage);
    let mut first = mediator("first", &grants);
    assert_eq!(first.take_granted(), [CapabilityKind::Storage]);
    grants.revoke("first", CapabilityKind::Storage);
    assert!(matches!(
        first.settings_get("first.key".into()),
        Err(PluginError::Revoked(_))
    ));
}
