//! Installed-contract-style integration tests for AI Provider's nonsecret settings.
//!
//! These tests use the real renderer-neutral Core UI Runtime and its action
//! validation, not a mock JSON snapshot validator or a browser-only DOM path.

use std::collections::BTreeSet;

use rintawa_ai_provider::{
    OpenAiConnection,
    model_catalog::{ModelProbe, ModelProbeEvent, ModelProbeStep},
    settings::{
        SETTINGS_SURFACE, SettingsController, SettingsIntent, settings_surface_contribution,
    },
};
use rintawa_sdk::{
    contracts::ComponentRef,
    types::{ExtensionInstanceId, RuntimeScopeId},
    ui::{
        UiActionEvent, UiActionId, UiActionPayload, UiNodeId, UiPatch, UiPatchBatch, UiSurfaceId,
        UiSurfaceSnapshot,
    },
};
use rintawa_ui_runtime::{OwnedUiSurfaceContribution, UiRuntime};

fn owner() -> (ExtensionInstanceId, ComponentRef) {
    let instance = ExtensionInstanceId::new("provider-settings-integration");
    let component = ComponentRef::new(instance.clone(), "runtime");
    (instance, component)
}

fn runtime() -> anyhow::Result<(UiRuntime, ComponentRef)> {
    let ui = UiRuntime::new();
    let (instance, component) = owner();
    ui.register_instance(
        instance.clone(),
        RuntimeScopeId::new("host"),
        vec![OwnedUiSurfaceContribution {
            owner: component.clone(),
            contribution: settings_surface_contribution(),
        }],
        Vec::new(),
    )?;
    ui.set_instance_active(&instance, true)?;
    Ok((ui, component))
}

fn event(revision: u64, node: &str, action: &str, payload: UiActionPayload) -> UiActionEvent {
    let (instance, _) = owner();
    UiActionEvent {
        owner_instance_id: instance,
        surface_id: UiSurfaceId::new(SETTINGS_SURFACE),
        node_id: UiNodeId::new(node),
        action_id: UiActionId::new(action),
        surface_revision: revision,
        payload,
    }
}

fn replace_snapshot_batch(previous: &UiSurfaceSnapshot, next: &UiSurfaceSnapshot) -> UiPatchBatch {
    let next_nodes = next
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut patches = next
        .nodes
        .iter()
        .cloned()
        .map(|node| UiPatch::UpsertNode { node })
        .collect::<Vec<_>>();
    patches.extend(
        previous
            .nodes
            .iter()
            .filter(|node| !next_nodes.contains(node.id.as_str()))
            .map(|node| UiPatch::RemoveNode {
                node_id: node.id.clone(),
            }),
    );
    UiPatchBatch {
        surface_id: previous.surface_id.clone(),
        base_revision: previous.revision,
        next_revision: next.revision,
        patches,
    }
}

#[test]
fn test_should_mount_settings_and_patch_explicit_saved_connection_in_real_ui_runtime()
-> anyhow::Result<()> {
    let (ui, component) = runtime()?;
    let mut settings = SettingsController::new(OpenAiConnection::default())?;
    let initial = settings.snapshot();
    ui.mount_surface(&component, initial.clone())?;
    assert_eq!(ui.presentation_surfaces().len(), 1);

    settings.handle_action(&event(
        initial.revision,
        "connection.base-url",
        "rintawa.ai-provider.settings.set-base-url",
        UiActionPayload::Text("http://127.0.0.1:1234/v1".to_string()),
    ))?;
    let edited = settings.snapshot();
    ui.apply_patches(&component, replace_snapshot_batch(&initial, &edited))?;

    settings.handle_action(&event(
        edited.revision,
        "connection.model",
        "rintawa.ai-provider.settings.set-model",
        UiActionPayload::Text("local-model".to_string()),
    ))?;
    let modeled = settings.snapshot();
    ui.apply_patches(&component, replace_snapshot_batch(&edited, &modeled))?;

    settings.handle_action(&event(
        modeled.revision,
        "connection.requires-key",
        "rintawa.ai-provider.settings.set-requires-key",
        UiActionPayload::Boolean(false),
    ))?;
    let no_key = settings.snapshot();
    ui.apply_patches(&component, replace_snapshot_batch(&modeled, &no_key))?;

    let to_save = settings.handle_action(&event(
        no_key.revision,
        "connection.save",
        "rintawa.ai-provider.settings.save",
        UiActionPayload::None,
    ))?;
    let SettingsIntent::Save(valid) = to_save else {
        anyhow::bail!("settings did not request an explicit validated save");
    };
    assert_eq!(valid.base_url, "http://127.0.0.1:1234/v1");
    assert_eq!(valid.model, "local-model");
    assert!(!valid.requires_api_key);
    // A real runtime adapter saves this bounded descriptor with preferences.set.
    // Simulate success only after that Host-side write returns.
    settings.confirm_saved(&valid)?;
    let saved = settings.snapshot();
    ui.apply_patches(&component, replace_snapshot_batch(&no_key, &saved))?;

    let surfaces = ui.presentation_surfaces();
    assert_eq!(surfaces.len(), 1);
    let presentation_json = serde_json::to_string(&surfaces[0])?;
    assert!(presentation_json.contains("local-model"));
    assert!(presentation_json.contains("127.0.0.1:1234"));
    assert!(!presentation_json.contains("Bearer "));
    assert!(!presentation_json.contains("Authorization"));
    assert!(!settings.is_dirty());
    Ok(())
}

#[test]
fn test_should_keep_real_surface_mounted_after_rejected_connection_and_recovery()
-> anyhow::Result<()> {
    let (ui, component) = runtime()?;
    let mut settings = SettingsController::new(OpenAiConnection::default())?;
    let first = settings.snapshot();
    ui.mount_surface(&component, first.clone())?;

    settings.handle_action(&event(
        1,
        "connection.base-url",
        "rintawa.ai-provider.settings.set-base-url",
        UiActionPayload::Text("http://169.254.169.254/latest".to_string()),
    ))?;
    let invalid = settings.snapshot();
    ui.apply_patches(&component, replace_snapshot_batch(&first, &invalid))?;

    let rejected = settings.handle_action(&event(
        invalid.revision,
        "connection.save",
        "rintawa.ai-provider.settings.save",
        UiActionPayload::None,
    ));
    assert!(rejected.is_err());
    assert_eq!(settings.saved(), &OpenAiConnection::default());
    settings.report_invalid(rintawa_ai_provider::settings::SettingsError::InvalidConnection)?;
    let warning = settings.snapshot();
    ui.apply_patches(&component, replace_snapshot_batch(&invalid, &warning))?;

    settings.handle_action(&event(
        warning.revision,
        "connection.reset",
        "rintawa.ai-provider.settings.reset",
        UiActionPayload::None,
    ))?;
    let recovered = settings.snapshot();
    ui.apply_patches(&component, replace_snapshot_batch(&warning, &recovered))?;

    assert_eq!(ui.presentation_surfaces().len(), 1);
    assert_eq!(settings.draft(), &OpenAiConnection::default());
    assert!(!settings.is_dirty());
    assert_eq!(
        recovered.revision,
        warning.revision + 1,
        "recovery must advance the mounted revision"
    );
    Ok(())
}

#[test]
fn test_should_patch_actual_ui_runtime_after_model_probe_and_selection() -> anyhow::Result<()> {
    let (ui, owner) = runtime()?;
    let mut controller = SettingsController::new(OpenAiConnection::default())?;
    let initial = controller.snapshot();
    ui.mount_surface(&owner, initial.clone())?;
    let SettingsIntent::Check(_) = controller.handle_action(&event(
        initial.revision,
        "connection.check",
        "rintawa.ai-provider.settings.check",
        UiActionPayload::None,
    ))?
    else {
        anyhow::bail!("model discovery should start from an explicit action");
    };
    let pending = controller.snapshot();
    ui.apply_patches(&owner, replace_snapshot_batch(&initial, &pending))?;

    let mut probe = ModelProbe::new();
    assert_eq!(
        probe.advance(ModelProbeEvent::Response(200)),
        ModelProbeStep::Pending
    );
    for chunk in [
        br#"{"data":[{"id":"local-a"},"#.as_slice(),
        br#"{"id":"local-b"}]}"#.as_slice(),
    ] {
        assert_eq!(
            probe.advance(ModelProbeEvent::Data(chunk)),
            ModelProbeStep::Pending
        );
    }
    let ModelProbeStep::Finished(Ok(catalog)) = probe.advance(ModelProbeEvent::Completed) else {
        anyhow::bail!("provider HTTP fixture must complete with a bounded inventory");
    };
    controller.finish_check(Ok(catalog))?;
    let offered = controller.snapshot();
    ui.apply_patches(&owner, replace_snapshot_batch(&pending, &offered))?;

    controller.handle_action(&event(
        offered.revision,
        "connection.select-discovered-model",
        "rintawa.ai-provider.settings.select-model",
        UiActionPayload::Text("local-b".into()),
    ))?;
    let selected = controller.snapshot();
    ui.apply_patches(&owner, replace_snapshot_batch(&offered, &selected))?;
    assert!(controller.is_dirty());
    assert_eq!(controller.draft().model, "local-b");
    assert_eq!(ui.presentation_surfaces().len(), 1);

    let SettingsIntent::Save(chosen) = controller.handle_action(&event(
        selected.revision,
        "connection.save",
        "rintawa.ai-provider.settings.save",
        UiActionPayload::None,
    ))?
    else {
        anyhow::bail!("chosen model must require explicit confirmation");
    };
    controller.confirm_saved(&chosen)?;
    let saved = controller.snapshot();
    ui.apply_patches(&owner, replace_snapshot_batch(&selected, &saved))?;
    assert_eq!(chosen.model, "local-b");
    assert!(!controller.is_dirty());
    Ok(())
}
