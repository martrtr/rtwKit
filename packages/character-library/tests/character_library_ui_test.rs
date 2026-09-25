//! Integration tests for Character Library catalog state, actions, and Portable UI rendering.

use std::collections::BTreeMap;

use rintawa_artifacts::ArtifactDigest;
use rintawa_character_library::{
    CHARACTER_LIBRARY_ACTION_IMPORT, CHARACTER_LIBRARY_ACTION_INSTANTIATE,
    CHARACTER_LIBRARY_ACTION_REFRESH, CHARACTER_LIBRARY_ACTION_SELECT,
    CHARACTER_LIBRARY_SURFACE_ID, CHARACTER_TEMPLATE_CONTENT_V1, CharacterContentDocument,
    CharacterContentRecord, CharacterLibraryController, CharacterLibraryError,
    CharacterLibraryGateway, CharacterLibraryGatewayError, CharacterLibraryIntent,
    CharacterTemplate, MAX_CHARACTER_IMPORT_TEXT_BYTES, MAX_CHARACTER_LIBRARY_ENTRIES,
    build_character_library_snapshot, character_library_surface_contribution, entry_select_node_id,
};
use rintawa_sdk::{
    contracts::ComponentRef,
    types::{ExtensionInstanceId, RuntimeScopeId},
    ui::{UiActionEvent, UiActionId, UiActionPayload, UiNodeId, UiNodeKind, UiSurfaceId},
};
use rintawa_ui_runtime::{OwnedUiSurfaceContribution, UiRuntime};

#[derive(Default)]
struct RecordingGateway {
    records: Vec<CharacterContentRecord>,
    next_records: Option<Vec<CharacterContentRecord>>,
    documents: BTreeMap<String, CharacterContentDocument>,
    next_documents: Option<BTreeMap<String, CharacterContentDocument>>,
    list_calls: usize,
}

impl CharacterLibraryGateway for RecordingGateway {
    fn list_templates(
        &mut self,
    ) -> Result<Vec<CharacterContentRecord>, CharacterLibraryGatewayError> {
        self.list_calls = self.list_calls.saturating_add(1);
        if self.list_calls > 1
            && let Some(records) = &self.next_records
        {
            return Ok(records.clone());
        }
        Ok(self.records.clone())
    }

    fn read_template(
        &mut self,
        id: &str,
    ) -> Result<CharacterContentDocument, CharacterLibraryGatewayError> {
        if self.list_calls > 1
            && let Some(documents) = &self.next_documents
            && let Some(document) = documents.get(id)
        {
            return Ok(document.clone());
        }
        self.documents
            .get(id)
            .cloned()
            .ok_or(CharacterLibraryGatewayError::NotFound)
    }
}

fn template(name: &str, creator: &str) -> CharacterTemplate {
    let mut template = serde_json::from_value::<CharacterTemplate>(serde_json::json!({
        "name": name,
        "description": format!("Description for {name}")
    }))
    .expect("test CharacterTemplate must decode");
    template.metadata.creator = creator.to_string();
    template
}

fn record(
    id: &str,
    name: &str,
    creator: &str,
) -> (CharacterContentRecord, CharacterContentDocument) {
    let template = template(name, creator);
    let revision = ArtifactDigest::sha256(name.as_bytes()).to_string();
    let metadata = CharacterContentRecord {
        id: id.to_string(),
        content: CHARACTER_TEMPLATE_CONTENT_V1.to_string(),
        revision,
    };
    let document = CharacterContentDocument {
        metadata: metadata.clone(),
        descriptor: serde_json::to_vec(&template).expect("test template must serialize"),
    };
    (metadata, document)
}

fn gateway(items: &[(&str, &str, &str)]) -> RecordingGateway {
    let mut gateway = RecordingGateway::default();
    for (id, name, creator) in items {
        let (metadata, document) = record(id, name, creator);
        gateway.documents.insert((*id).to_string(), document);
        gateway.records.push(metadata);
    }
    gateway
}

fn action(
    revision: u64,
    node_id: UiNodeId,
    action_id: &str,
    payload: UiActionPayload,
) -> UiActionEvent {
    UiActionEvent {
        owner_instance_id: ExtensionInstanceId::new("character-library"),
        surface_id: UiSurfaceId::new(CHARACTER_LIBRARY_SURFACE_ID),
        node_id,
        action_id: UiActionId::new(action_id),
        surface_revision: revision,
        payload,
    }
}

#[test]
fn test_should_sort_select_and_mount_character_library_snapshot() -> anyhow::Result<()> {
    let mut controller = CharacterLibraryController::new(gateway(&[
        ("id-b", "Zeta", "Bob"),
        ("id-a", "Alice", "Ada"),
    ]));
    controller.refresh()?;
    assert_eq!(controller.state().entries()[0].template.name, "Alice");
    assert_eq!(controller.state().selected_id(), Some("id-a"));

    let snapshot = build_character_library_snapshot(controller.state());
    let ui = UiRuntime::new();
    let instance_id = ExtensionInstanceId::new("character-library");
    let owner = ComponentRef::new(instance_id.clone(), "runtime");
    ui.register_instance(
        instance_id.clone(),
        RuntimeScopeId::new("host"),
        vec![OwnedUiSurfaceContribution {
            owner: owner.clone(),
            contribution: character_library_surface_contribution(),
        }],
        Vec::new(),
    )?;
    ui.set_instance_active(&instance_id, true)?;
    ui.mount_surface(&owner, snapshot)?;
    assert_eq!(ui.presentation_surfaces().len(), 1);

    let select = action(
        controller.state().revision(),
        entry_select_node_id(1),
        CHARACTER_LIBRARY_ACTION_SELECT,
        UiActionPayload::None,
    );
    controller.handle_action(&select)?;
    assert_eq!(controller.state().selected_id(), Some("id-b"));
    Ok(())
}

#[test]
fn test_should_preserve_selection_across_refresh_when_entry_survives() -> anyhow::Result<()> {
    let mut gateway = gateway(&[("id-a", "Alice", "Ada"), ("id-b", "Zeta", "Bob")]);
    let (next, document) = record("id-b", "Zeta Revised", "Bob");
    gateway.next_records = Some(vec![next]);
    gateway.next_documents = Some(BTreeMap::from([(String::from("id-b"), document)]));
    let mut controller = CharacterLibraryController::new(gateway);
    controller.refresh()?;
    controller.select_index(1)?;
    controller.refresh()?;
    assert_eq!(controller.state().entries().len(), 1);
    assert_eq!(controller.state().selected_id(), Some("id-b"));
    assert_eq!(
        controller.state().entries()[0].template.name,
        "Zeta Revised"
    );
    Ok(())
}

#[test]
fn test_should_fail_closed_for_stale_spoofed_or_invalid_payload_actions() -> anyhow::Result<()> {
    let mut controller = CharacterLibraryController::new(gateway(&[("id-a", "Alice", "Ada")]));
    controller.refresh()?;

    let stale = action(
        controller.state().revision().saturating_sub(1),
        entry_select_node_id(0),
        CHARACTER_LIBRARY_ACTION_SELECT,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&stale),
        Err(CharacterLibraryError::StaleAction)
    );

    let spoofed = action(
        controller.state().revision(),
        UiNodeId::new("catalog.empty"),
        CHARACTER_LIBRARY_ACTION_REFRESH,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&spoofed),
        Err(CharacterLibraryError::WrongActionNode)
    );

    let spoofed_instantiate = action(
        controller.state().revision(),
        entry_select_node_id(0),
        CHARACTER_LIBRARY_ACTION_INSTANTIATE,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&spoofed_instantiate),
        Err(CharacterLibraryError::WrongActionNode)
    );

    let payload = action(
        controller.state().revision(),
        entry_select_node_id(0),
        CHARACTER_LIBRARY_ACTION_SELECT,
        UiActionPayload::Text(String::from("id-a")),
    );
    assert_eq!(
        controller.handle_action(&payload),
        Err(CharacterLibraryError::InvalidActionPayload)
    );
    Ok(())
}

#[test]
fn test_should_validate_and_reconcile_tavern_json_import_actions() -> anyhow::Result<()> {
    let mut controller = CharacterLibraryController::new(gateway(&[]));
    controller.refresh()?;
    let snapshot = build_character_library_snapshot(controller.state());
    let import_node = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "import.source")
        .ok_or_else(|| anyhow::anyhow!("import text area must be rendered"))?;
    assert!(matches!(
        &import_node.kind,
        UiNodeKind::TextArea(area)
            if area.is_enabled
                && area.change_action.is_none()
                && area.submit_action.as_ref().is_some_and(
                    |action| action.as_str() == CHARACTER_LIBRARY_ACTION_IMPORT
                )
    ));

    let source =
        String::from(r#"{"spec":"chara_card_v2","spec_version":"2.0","data":{"name":"Alice"}}"#);
    let intent = controller.handle_action(&action(
        controller.state().revision(),
        UiNodeId::new("import.source"),
        CHARACTER_LIBRARY_ACTION_IMPORT,
        UiActionPayload::Text(source.clone()),
    ))?;
    assert_eq!(
        intent,
        CharacterLibraryIntent::ImportTavernJson {
            source: source.clone()
        }
    );
    assert!(controller.state().import_pending());
    assert_eq!(controller.state().import_source(), source);

    assert_eq!(
        controller.handle_action(&action(
            controller.state().revision(),
            UiNodeId::new("import.source"),
            CHARACTER_LIBRARY_ACTION_IMPORT,
            UiActionPayload::Text(String::from("{}")),
        )),
        Err(CharacterLibraryError::ImportAlreadyPending)
    );

    controller.fail_import("invalid card")?;
    assert!(!controller.state().import_pending());
    assert_eq!(controller.state().import_source(), source);
    assert_eq!(controller.state().import_status(), Some("invalid card"));
    let failed_snapshot = build_character_library_snapshot(controller.state());
    assert!(matches!(
        failed_snapshot
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "import.source")
            .map(|node| &node.kind),
        Some(UiNodeKind::TextArea(area)) if area.is_enabled && area.value == source
    ));

    assert_eq!(
        controller.handle_action(&action(
            controller.state().revision(),
            UiNodeId::new("import.source"),
            CHARACTER_LIBRARY_ACTION_IMPORT,
            UiActionPayload::Text(String::from("   ")),
        )),
        Err(CharacterLibraryError::EmptyImportSource)
    );
    assert_eq!(
        controller.handle_action(&action(
            controller.state().revision(),
            UiNodeId::new("import.source"),
            CHARACTER_LIBRARY_ACTION_IMPORT,
            UiActionPayload::Text("x".repeat(MAX_CHARACTER_IMPORT_TEXT_BYTES + 1)),
        )),
        Err(CharacterLibraryError::ImportSourceTooLarge)
    );
    Ok(())
}

#[test]
fn test_should_select_imported_item_only_after_validated_refresh() -> anyhow::Result<()> {
    let mut gateway = gateway(&[]);
    let (imported, document) = record("id-imported", "Imported Alice", "Ada");
    gateway.next_records = Some(vec![imported]);
    gateway.next_documents = Some(BTreeMap::from([(String::from("id-imported"), document)]));
    let mut controller = CharacterLibraryController::new(gateway);
    controller.refresh()?;
    controller.handle_action(&action(
        controller.state().revision(),
        UiNodeId::new("import.source"),
        CHARACTER_LIBRARY_ACTION_IMPORT,
        UiActionPayload::Text(String::from(
            r#"{"spec":"chara_card_v2","spec_version":"2.0","data":{"name":"Alice"}}"#,
        )),
    ))?;

    controller.complete_import("id-imported")?;
    assert!(!controller.state().import_pending());
    assert!(controller.state().import_source().is_empty());
    assert_eq!(controller.state().selected_id(), Some("id-imported"));
    assert_eq!(
        controller.state().import_status(),
        Some("Imported Imported Alice.")
    );
    Ok(())
}

#[test]
fn test_should_preserve_previous_state_when_refresh_document_metadata_is_spoofed()
-> anyhow::Result<()> {
    let mut gateway = gateway(&[("id-a", "Alice", "Ada")]);
    let (next, mut document) = record("id-a", "Alice Revised", "Ada");
    document.metadata.revision = ArtifactDigest::sha256(b"wrong revision").to_string();
    gateway.next_records = Some(vec![next]);
    gateway.next_documents = Some(BTreeMap::from([(String::from("id-a"), document)]));
    let mut controller = CharacterLibraryController::new(gateway);
    controller.refresh()?;
    let original = controller.state().clone();

    assert_eq!(
        controller.refresh(),
        Err(CharacterLibraryError::MetadataMismatch)
    );
    assert_eq!(controller.state(), &original);
    Ok(())
}

#[test]
fn test_should_bound_every_rendered_text_node_with_long_utf8_content() -> anyhow::Result<()> {
    let long_text = "é".repeat(4_000);
    let (metadata, mut document) = record("id-long", "Long", "Tester");
    let mut template = serde_json::from_slice::<CharacterTemplate>(&document.descriptor)?;
    template.description = long_text.clone();
    template.personality = long_text.clone();
    template.metadata.creator = long_text.clone();
    template.metadata.tags = vec![long_text];
    document.descriptor = serde_json::to_vec(&template)?;
    let gateway = RecordingGateway {
        records: vec![metadata],
        documents: BTreeMap::from([(String::from("id-long"), document)]),
        ..RecordingGateway::default()
    };
    let mut controller = CharacterLibraryController::new(gateway);
    controller.refresh()?;
    let snapshot = build_character_library_snapshot(controller.state());
    for node in snapshot.nodes {
        if let UiNodeKind::Text(text) = node.kind {
            assert!(text.text.len() <= 4 * 1024);
        }
    }
    Ok(())
}

#[test]
fn test_should_emit_exact_revision_instantiation_intent() -> anyhow::Result<()> {
    let mut controller = CharacterLibraryController::new(gateway(&[("id-a", "Alice", "Ada")]));
    controller.refresh()?;
    let snapshot = build_character_library_snapshot(controller.state());
    let button = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "details.instantiate")
        .ok_or_else(|| anyhow::anyhow!("instantiate button must be rendered"))?;
    assert!(matches!(button.kind, UiNodeKind::Button(_)));

    let expected_revision = controller
        .state()
        .selected_entry()
        .ok_or_else(|| anyhow::anyhow!("selected fixture entry must exist"))?
        .revision
        .to_string();
    let intent = controller.handle_action(&action(
        controller.state().revision(),
        UiNodeId::new("details.instantiate"),
        CHARACTER_LIBRARY_ACTION_INSTANTIATE,
        UiActionPayload::None,
    ))?;
    assert_eq!(
        intent,
        CharacterLibraryIntent::InstantiateSelected {
            template_id: String::from("id-a"),
            template_revision: expected_revision,
        }
    );
    Ok(())
}

#[test]
fn test_should_bound_catalog_and_render_maximum_snapshot() -> anyhow::Result<()> {
    let mut catalog_gateway = RecordingGateway::default();
    for index in 0..MAX_CHARACTER_LIBRARY_ENTRIES {
        let id = format!("id-{index:03}");
        let name = format!("Character {index:03}");
        let (metadata, document) = record(&id, &name, "Tester");
        catalog_gateway.documents.insert(id, document);
        catalog_gateway.records.push(metadata);
    }
    let mut controller = CharacterLibraryController::new(catalog_gateway);
    controller.refresh()?;
    let snapshot = build_character_library_snapshot(controller.state());
    assert!(snapshot.nodes.len() < 1024);

    let mut oversized = gateway(&[]);
    for index in 0..=MAX_CHARACTER_LIBRARY_ENTRIES {
        let id = format!("too-many-{index:03}");
        let (metadata, document) = record(&id, "Bounded", "Tester");
        oversized.documents.insert(id, document);
        oversized.records.push(metadata);
    }
    let mut oversized_controller = CharacterLibraryController::new(oversized);
    assert_eq!(
        oversized_controller.refresh(),
        Err(CharacterLibraryError::CatalogTooLarge)
    );
    Ok(())
}
