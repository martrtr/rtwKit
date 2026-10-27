//! Renderer-neutral connection settings for the standard text inference provider.
//!
//! This is presentation and nonsecret profile data, not inference authority.
//! All network requests still pass Host HTTP policy and every key remains in
//! the Core secret capability. A settings action cannot grant either permission.

use crate::{
    OpenAiConnection,
    model_catalog::{ModelCatalog, ModelProbeError},
};
use rintawa_sdk::ui::{
    UI_CAPABILITY_BUTTON, UI_CAPABILITY_CHECKBOX, UI_CAPABILITY_COLUMN, UI_CAPABILITY_ROW,
    UI_CAPABILITY_SELECT, UI_CAPABILITY_TEXT, UI_CAPABILITY_TEXT_INPUT, UiActionEvent, UiActionId,
    UiActionPayload, UiActivityContribution, UiButtonAppearance, UiButtonNode, UiCheckboxNode,
    UiContainerNode, UiNode, UiNodeId, UiNodeKind, UiPlacementHint, UiSelectNode, UiSelectOption,
    UiSurfaceContribution, UiSurfaceId, UiSurfaceSnapshot, UiTextInputNode, UiTextNode,
};
use thiserror::Error;

/// Stable surface identity, usable in baseline or a scoped World composition.
pub const SETTINGS_SURFACE: &str = "rintawa.ai-provider.settings";
/// This section participates in the normal shared management workspace.
pub const MANAGE_ACTIVITY: &str = "rintawa.management";
/// The sole nonsecret preference containing the connection descriptor.
pub const CONNECTION_PREFERENCE: &str = "connection";

const BASE_URL_NODE: &str = "connection.base-url";
const MODEL_NODE: &str = "connection.model";
const REQUIRES_KEY_NODE: &str = "connection.requires-key";
const SAVE_NODE: &str = "connection.save";
const RESET_NODE: &str = "connection.reset";
const DEFAULTS_NODE: &str = "connection.defaults";
const CHECK_NODE: &str = "connection.check";
const MODELS_SELECT_NODE: &str = "connection.select-discovered-model";

const SET_BASE_URL: &str = "rintawa.ai-provider.settings.set-base-url";
const SET_MODEL: &str = "rintawa.ai-provider.settings.set-model";
const SET_REQUIRES_KEY: &str = "rintawa.ai-provider.settings.set-requires-key";
const SAVE: &str = "rintawa.ai-provider.settings.save";
const RESET: &str = "rintawa.ai-provider.settings.reset";
const DEFAULTS: &str = "rintawa.ai-provider.settings.defaults";
const CHECK: &str = "rintawa.ai-provider.settings.check";
const SELECT_MODEL: &str = "rintawa.ai-provider.settings.select-model";

/// Bounded state-machine error for an untrusted semantic input.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettingsError {
    /// The renderer referred to an older or future surface revision.
    #[error("stale provider-settings action")]
    StaleRevision,
    /// The action is not owned by this surface or matching control.
    #[error("unknown provider-settings action")]
    UnknownAction,
    /// The action carried an incompatible payload type.
    #[error("invalid provider-settings payload")]
    InvalidPayload,
    /// Unbounded input was refused before entering persistent preferences.
    #[error("provider-settings input exceeded its limit")]
    InputTooLarge,
    /// A requested connection descriptor failed validation.
    #[error("invalid provider connection settings")]
    InvalidConnection,
    /// Revision exhaustion must not silently restart at an old value.
    #[error("provider-settings revision exhausted")]
    RevisionExhausted,
    /// No completed catalog is available or a caller selected a phantom model.
    #[error("model was not present in the discovered provider inventory")]
    UnknownModel,
    /// Cannot test a draft or start a second concurrent request.
    #[error("save the connection before checking or finish the active check")]
    CheckUnavailable,
}

/// Explicit host-persistence intent produced by a validated UI action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsIntent {
    /// Only ephemeral draft state or status changed.
    None,
    /// Persist a nonsecret, validated connection descriptor.
    Save(OpenAiConnection),
    /// Begin one asynchronous read-only check using the exact saved descriptor.
    Check(OpenAiConnection),
}

/// One component-local draft: Host preferences hold only the saved connection.
#[derive(Debug, Clone)]
pub struct SettingsController {
    saved: OpenAiConnection,
    draft: OpenAiConnection,
    revision: u64,
    notice: String,
    has_unsaved_changes: bool,
    needs_repair: bool,
    catalog: Option<ModelCatalog>,
    is_checking: bool,
}

impl SettingsController {
    /// Starts the surface from exactly one previously validated persisted connection.
    ///
    /// The caller must parse the preference as an OpenAiConnection and reject
    /// invalid or oversized values before constructing this controller.
    pub fn new(saved: OpenAiConnection) -> Result<Self, SettingsError> {
        saved
            .validate()
            .map_err(|_| SettingsError::InvalidConnection)?;
        Ok(Self {
            draft: saved.clone(),
            saved,
            revision: 1,
            notice: String::from("Saved connection loaded. Host permissions are separate."),
            has_unsaved_changes: false,
            needs_repair: false,
            catalog: None,
            is_checking: false,
        })
    }

    /// Starts from safe defaults when an existing nonsecret stored preference
    /// cannot be decoded or does not satisfy current validation.
    ///
    /// This never rewrites corrupt data silently: an explicit Save action must
    /// successfully replace the invalid stored value before the warning clears.
    /// The active inference runtime continues to reject the invalid preference.
    pub fn from_invalid_preference() -> Self {
        let fallback = OpenAiConnection::default();
        Self {
            saved: fallback.clone(),
            draft: fallback,
            revision: 1,
            notice: String::from(
                "Stored connection is invalid. Review these defaults and save to repair it.",
            ),
            has_unsaved_changes: true,
            needs_repair: true,
            catalog: None,
            is_checking: false,
        }
    }

    /// Current revision of the semantic surface.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the draft; never includes an API key.
    pub const fn draft(&self) -> &OpenAiConnection {
        &self.draft
    }

    /// Returns the last confirmed, nonsecret persisted descriptor.
    pub const fn saved(&self) -> &OpenAiConnection {
        &self.saved
    }

    /// Whether an edit is waiting for explicit persistence.
    pub const fn is_dirty(&self) -> bool {
        self.has_unsaved_changes
    }

    /// The last validated diagnostic inventory, when it still matches the draft.
    pub const fn catalog(&self) -> Option<&ModelCatalog> {
        self.catalog.as_ref()
    }

    /// Whether an explicit diagnostic HTTP request is currently outstanding.
    pub const fn is_checking(&self) -> bool {
        self.is_checking
    }

    /// Finishes an explicit diagnostic request without importing untrusted raw
    /// response headers, bodies or credentials into the surface.
    pub fn finish_check(
        &mut self,
        result: Result<ModelCatalog, ModelProbeError>,
    ) -> Result<(), SettingsError> {
        if !self.is_checking {
            return Err(SettingsError::CheckUnavailable);
        }
        self.is_checking = false;
        match result {
            Ok(catalog) => {
                self.notice = catalog.summary();
                self.catalog = Some(catalog);
            }
            Err(error) => {
                self.catalog = None;
                self.notice = error.to_string();
            }
        }
        self.advance()
    }

    /// Short user-facing error or status, excluding raw credentials or prompt text.
    pub fn notice(&self) -> &str {
        &self.notice
    }

    fn advance(&mut self) -> Result<(), SettingsError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(SettingsError::RevisionExhausted)?;
        self.has_unsaved_changes = self.draft != self.saved || self.needs_repair;
        if self.has_unsaved_changes {
            self.is_checking = false;
            self.catalog = None;
        }
        Ok(())
    }

    fn edit_text(&mut self, value: &str, is_base: bool) -> Result<(), SettingsError> {
        let limit = if is_base { 2048 } else { 160 };
        if value.len() > limit {
            return Err(SettingsError::InputTooLarge);
        }
        if is_base {
            self.draft.base_url = value.to_owned();
        } else {
            self.draft.model = value.to_owned();
        }
        self.notice = String::from("Review and save to use these settings for new requests.");
        self.advance()
    }

    /// Validates the exact UI control/action/payload combination and applies an edit.
    ///
    /// Host surface ownership and instance checks are enforced before dispatch;
    /// this controller independently rejects stale/mismatched semantic actions.
    pub fn handle_action(
        &mut self,
        event: &UiActionEvent,
    ) -> Result<SettingsIntent, SettingsError> {
        if event.surface_id.as_str() != SETTINGS_SURFACE {
            return Err(SettingsError::UnknownAction);
        }
        if event.surface_revision != self.revision {
            return Err(SettingsError::StaleRevision);
        }

        let node = event.node_id.as_str();
        let action = event.action_id.as_str();
        match (node, action, &event.payload) {
            (BASE_URL_NODE, SET_BASE_URL, UiActionPayload::Text(value)) => {
                self.edit_text(value, true)?;
            }
            (MODEL_NODE, SET_MODEL, UiActionPayload::Text(value)) => {
                self.edit_text(value, false)?;
            }
            (REQUIRES_KEY_NODE, SET_REQUIRES_KEY, UiActionPayload::Boolean(value)) => {
                self.draft.requires_api_key = *value;
                self.notice = String::from("Review and save to update the connection.");
                self.advance()?;
            }
            (RESET_NODE, RESET, UiActionPayload::None) => {
                self.draft = self.saved.clone();
                self.notice = String::from("Unsaved edits discarded.");
                self.advance()?;
            }
            (DEFAULTS_NODE, DEFAULTS, UiActionPayload::None) => {
                self.draft = OpenAiConnection::default();
                self.notice = String::from("Defaults loaded into draft; save to persist.");
                self.advance()?;
            }
            (CHECK_NODE, CHECK, UiActionPayload::None) => {
                if self.is_dirty() || self.is_checking {
                    return Err(SettingsError::CheckUnavailable);
                }
                self.saved
                    .validate()
                    .map_err(|_| SettingsError::InvalidConnection)?;
                self.catalog = None;
                self.is_checking = true;
                self.notice = String::from("Checking saved provider endpoint and model catalog…");
                self.advance()?;
                return Ok(SettingsIntent::Check(self.saved.clone()));
            }
            (MODELS_SELECT_NODE, SELECT_MODEL, UiActionPayload::Text(model)) => {
                if self.is_checking
                    || !self
                        .catalog
                        .as_ref()
                        .is_some_and(|catalog| catalog.includes(model))
                {
                    return Err(SettingsError::UnknownModel);
                }
                self.edit_text(model, false)?;
            }
            (SAVE_NODE, SAVE, UiActionPayload::None) => {
                self.draft
                    .validate()
                    .map_err(|_| SettingsError::InvalidConnection)?;
                if !self.is_dirty() {
                    self.notice = String::from("Connection already saved.");
                    self.advance()?;
                    return Ok(SettingsIntent::None);
                }
                return Ok(SettingsIntent::Save(self.draft.clone()));
            }
            (BASE_URL_NODE | MODEL_NODE | REQUIRES_KEY_NODE, _, _)
            | (SAVE_NODE | RESET_NODE | DEFAULTS_NODE | CHECK_NODE | MODELS_SELECT_NODE, _, _) => {
                return Err(SettingsError::InvalidPayload);
            }
            _ => return Err(SettingsError::UnknownAction),
        }
        Ok(SettingsIntent::None)
    }

    /// Records successful host persistence; do not call before a successful set.
    pub fn confirm_saved(&mut self, saved: &OpenAiConnection) -> Result<(), SettingsError> {
        if &self.draft != saved {
            return Err(SettingsError::InvalidConnection);
        }
        self.saved = saved.clone();
        self.needs_repair = false;
        self.catalog = None;
        self.is_checking = false;
        self.notice = String::from("Saved. New requests use this connection.");
        self.advance()
    }

    /// Makes a storage rejection visible without claiming anything was persisted.
    pub fn reject_save(&mut self) -> Result<(), SettingsError> {
        self.notice =
            String::from("Could not save preference. Host policy or storage unavailable.");
        self.advance()
    }

    /// Makes malformed actions visible without echoing arbitrary user bytes.
    pub fn report_invalid(&mut self, error: SettingsError) -> Result<(), SettingsError> {
        self.notice = match error {
            SettingsError::InvalidConnection => {
                String::from("Invalid endpoint or model. Check the fields before saving.")
            }
            SettingsError::InputTooLarge => {
                String::from("Endpoint or model exceeds the allowed input size.")
            }
            _ => String::from("Action rejected. Refresh the settings surface and retry."),
        };
        self.advance()
    }

    /// Builds the complete portable surface from current nonsecret state.
    pub fn snapshot(&self) -> UiSurfaceSnapshot {
        build_settings_snapshot(self)
    }
}

/// Declares one standard settings section through the renderer-neutral shell.
pub fn settings_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(SETTINGS_SURFACE, UiPlacementHint::Settings)
        .with_semantic(semantic("management.ai-providers"))
        .with_trait("workspace-tool")
        .with_trait("activity-section")
        .with_trait("ai-provider-settings")
        .with_trait("navigable")
        .with_activity(
            UiActivityContribution::new(MANAGE_ACTIVITY, "Manage")
                .with_icon_slot("activity.management"),
        )
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_TEXT_INPUT)
        .requiring_capability(UI_CAPABILITY_CHECKBOX)
        .requiring_capability(UI_CAPABILITY_SELECT)
        .requiring_capability(UI_CAPABILITY_BUTTON)
}

fn semantic(name: &str) -> rintawa_sdk::contracts::ContractKey {
    rintawa_sdk::contracts::ContractKey::new(name, rintawa_sdk::contracts::ContractVersion::new(1))
}

fn text_node(id: &str, text: impl Into<String>, trait_name: &str) -> UiNode {
    UiNode::new(id, UiNodeKind::Text(UiTextNode { text: text.into() })).with_trait(trait_name)
}

fn column(id: &str, children: &[&str], trait_name: &str) -> UiNode {
    UiNode::new(
        id,
        UiNodeKind::Column(UiContainerNode {
            children: children.iter().map(|id| UiNodeId::new(*id)).collect(),
        }),
    )
    .with_trait(trait_name)
}

fn row(id: &str, children: &[&str], trait_name: &str) -> UiNode {
    UiNode::new(
        id,
        UiNodeKind::Row(UiContainerNode {
            children: children.iter().map(|id| UiNodeId::new(*id)).collect(),
        }),
    )
    .with_trait(trait_name)
}

fn input(id: &str, value: String, placeholder: &str, action: &str) -> UiNode {
    UiNode::new(
        id,
        UiNodeKind::TextInput(UiTextInputNode {
            value,
            placeholder: Some(placeholder.into()),
            change_action: Some(UiActionId::new(action)),
            submit_action: None,
            submit_label: None,
            is_enabled: true,
        }),
    )
    .with_trait("settings-field")
}

fn button(
    id: &str,
    label: &str,
    action: &str,
    enabled: bool,
    appearance: UiButtonAppearance,
) -> UiNode {
    UiNode::new(
        id,
        UiNodeKind::Button(UiButtonNode {
            label: label.into(),
            action: UiActionId::new(action),
            is_enabled: enabled,
            appearance,
        }),
    )
    .with_trait("settings-action")
}

fn build_settings_snapshot(state: &SettingsController) -> UiSurfaceSnapshot {
    let summary = if state.draft.requires_api_key {
        "The Host must grant secret-read for ai.api_keys.openai and approve the HTTP origin."
    } else {
        "No key requested. HTTP origin permission is still mandatory, including loopback."
    };
    let save_label = if state.is_dirty() {
        "Save changes"
    } else {
        "Saved"
    };
    let nodes = vec![
        column(
            "root",
            &[
                "title-group",
                "status",
                "form",
                "actions",
                "model-discovery",
                "policy",
            ],
            "ai-provider-settings-root",
        ),
        column("title-group", &["title", "subtitle"], "settings-heading"),
        text_node("title", "AI Providers", "page-title"),
        text_node(
            "subtitle",
            "OpenAI-compatible text generation connection for this exact component scope.",
            "muted",
        ),
        text_node("status", state.notice().to_owned(), "settings-status"),
        column(
            "form",
            &[
                "endpoint-label",
                BASE_URL_NODE,
                "model-label",
                MODEL_NODE,
                "auth-label",
                REQUIRES_KEY_NODE,
            ],
            "settings-form",
        ),
        text_node("endpoint-label", "API base URL", "field-label"),
        input(
            BASE_URL_NODE,
            state.draft.base_url.clone(),
            "https://api.openai.com/v1",
            SET_BASE_URL,
        ),
        text_node("model-label", "Model ID", "field-label"),
        input(MODEL_NODE, state.draft.model.clone(), "model", SET_MODEL),
        text_node("auth-label", "Requires Host API key", "field-label"),
        UiNode::new(
            REQUIRES_KEY_NODE,
            UiNodeKind::Checkbox(UiCheckboxNode {
                label: String::from("Use Host-managed API key"),
                checked: state.draft.requires_api_key,
                change_action: UiActionId::new(SET_REQUIRES_KEY),
                is_enabled: true,
            }),
        )
        .with_trait("settings-field"),
        row(
            "actions",
            &[SAVE_NODE, RESET_NODE, DEFAULTS_NODE],
            "command-set",
        ),
        button(
            SAVE_NODE,
            save_label,
            SAVE,
            state.is_dirty(),
            UiButtonAppearance::Primary,
        ),
        button(
            RESET_NODE,
            "Discard edits",
            RESET,
            state.is_dirty(),
            UiButtonAppearance::Subtle,
        ),
        button(
            DEFAULTS_NODE,
            "Use defaults",
            DEFAULTS,
            true,
            UiButtonAppearance::Subtle,
        ),
        column(
            "model-discovery",
            &[
                "check-title",
                "check-help",
                CHECK_NODE,
                "model-choice-label",
                MODELS_SELECT_NODE,
            ],
            "model-discovery",
        ),
        text_node("check-title", "Connection and models", "section-heading"),
        text_node(
            "check-help",
            "Check the saved endpoint explicitly. The Host must separately approve its origin and any required secret.",
            "muted",
        ),
        button(
            CHECK_NODE,
            if state.is_checking {
                "Checking…"
            } else {
                "Check connection / list models"
            },
            CHECK,
            !state.is_dirty() && !state.is_checking,
            UiButtonAppearance::Subtle,
        ),
        text_node("model-choice-label", "Discovered model", "field-label"),
        UiNode::new(
            MODELS_SELECT_NODE,
            UiNodeKind::Select(UiSelectNode {
                value: if state
                    .catalog
                    .as_ref()
                    .is_some_and(|catalog| catalog.includes(&state.draft.model))
                {
                    state.draft.model.clone()
                } else {
                    String::new()
                },
                options: std::iter::once(UiSelectOption {
                    value: String::new(),
                    label: String::from("Choose a discovered model…"),
                })
                .chain(
                    state
                        .catalog
                        .iter()
                        .flat_map(|catalog| catalog.models.iter())
                        .map(|id| UiSelectOption {
                            value: id.clone(),
                            label: id.clone(),
                        }),
                )
                .collect(),
                change_action: UiActionId::new(SELECT_MODEL),
                is_enabled: !state.is_checking && !state.is_dirty() && state.catalog.is_some(),
            }),
        )
        .with_trait("settings-model-choice"),
        column(
            "policy",
            &["policy-title", "policy-explainer", "policy-scope"],
            "settings-policy",
        ),
        text_node("policy-title", "Host security", "section-heading"),
        text_node("policy-explainer", summary, "muted"),
        text_node(
            "policy-scope",
            "Saving a connection does not grant network or secret access. Existing in-flight generations keep their original endpoint.",
            "muted",
        ),
    ];
    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(SETTINGS_SURFACE),
        revision: state.revision(),
        root: UiNodeId::new("root"),
        nodes,
    }
}

#[cfg(test)]
mod tests {
    use rintawa_sdk::types::ExtensionInstanceId;

    use super::*;

    fn controller() -> SettingsController {
        SettingsController::new(OpenAiConnection::default()).unwrap()
    }

    fn action(revision: u64, node: &str, action: &str, payload: UiActionPayload) -> UiActionEvent {
        UiActionEvent {
            owner_instance_id: ExtensionInstanceId::new("provider-settings-test"),
            surface_id: UiSurfaceId::new(SETTINGS_SURFACE),
            node_id: UiNodeId::new(node),
            action_id: UiActionId::new(action),
            surface_revision: revision,
            payload,
        }
    }

    #[test]
    fn should_reject_stale_spoofed_or_wrongly_typed_actions() {
        let mut state = controller();
        assert_eq!(
            state.handle_action(&action(
                0,
                BASE_URL_NODE,
                SET_BASE_URL,
                UiActionPayload::Text(String::from("https://example.com/v1"))
            )),
            Err(SettingsError::StaleRevision)
        );
        assert_eq!(
            state.handle_action(&action(1, MODEL_NODE, SAVE, UiActionPayload::None)),
            Err(SettingsError::InvalidPayload)
        );
        assert_eq!(
            state.handle_action(&action(1, "unknown-control", SAVE, UiActionPayload::None)),
            Err(SettingsError::UnknownAction)
        );
        assert_eq!(state.revision(), 1);
    }

    #[test]
    fn should_require_explicit_save_and_preserve_saved_connection_on_failed_write() {
        let mut state = controller();
        let old = state.saved().clone();
        state
            .handle_action(&action(
                1,
                MODEL_NODE,
                SET_MODEL,
                UiActionPayload::Text(String::from("custom-model")),
            ))
            .unwrap();
        assert!(state.is_dirty());
        let result = state
            .handle_action(&action(2, SAVE_NODE, SAVE, UiActionPayload::None))
            .unwrap();
        assert!(matches!(result, SettingsIntent::Save(_)));
        assert_eq!(state.saved(), &old);
        state.reject_save().unwrap();
        assert!(state.is_dirty());
        let result = state
            .handle_action(&action(3, SAVE_NODE, SAVE, UiActionPayload::None))
            .unwrap();
        let SettingsIntent::Save(config) = result else {
            panic!("save must return a validated descriptor");
        };
        state.confirm_saved(&config).unwrap();
        assert_eq!(state.saved().model, "custom-model");
        assert!(!state.is_dirty());
    }

    #[test]
    fn should_not_persist_invalid_model_or_endpoint() {
        let mut state = controller();
        state
            .handle_action(&action(
                1,
                BASE_URL_NODE,
                SET_BASE_URL,
                UiActionPayload::Text(String::from("http://192.168.1.2:8080/v1")),
            ))
            .unwrap();
        assert_eq!(
            state.handle_action(&action(2, SAVE_NODE, SAVE, UiActionPayload::None)),
            Err(SettingsError::InvalidConnection)
        );
        assert_eq!(state.saved(), &OpenAiConnection::default());
    }

    #[test]
    fn should_discard_draft_and_restore_defaults_without_implicit_persistence() {
        let mut state = controller();
        state
            .handle_action(&action(
                1,
                MODEL_NODE,
                SET_MODEL,
                UiActionPayload::Text(String::from("edited")),
            ))
            .unwrap();
        state
            .handle_action(&action(2, RESET_NODE, RESET, UiActionPayload::None))
            .unwrap();
        assert_eq!(state.draft(), state.saved());
        assert!(!state.is_dirty());
        state
            .handle_action(&action(
                3,
                REQUIRES_KEY_NODE,
                SET_REQUIRES_KEY,
                UiActionPayload::Boolean(false),
            ))
            .unwrap();
        state
            .handle_action(&action(4, DEFAULTS_NODE, DEFAULTS, UiActionPayload::None))
            .unwrap();
        assert_eq!(state.draft(), &OpenAiConnection::default());
    }

    #[test]
    fn should_keep_provider_secret_out_of_all_portable_snapshots() {
        let state = controller();
        let snapshot = state.snapshot();
        assert_eq!(snapshot.surface_id.as_str(), SETTINGS_SURFACE);
        assert_eq!(snapshot.revision, 1);
        assert!(snapshot.nodes.len() >= 15);
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("management") || json.contains("ai-provider"));
        assert!(!json.contains("Authorization"));
        assert!(!json.contains("Bearer "));
        assert!(!json.contains("api-key-value"));
        assert!(json.len() < 16 * 1024);
    }

    #[test]
    fn should_reject_unbounded_unicode_input_before_save() {
        let mut state = controller();
        assert_eq!(
            state.handle_action(&action(
                1,
                MODEL_NODE,
                SET_MODEL,
                UiActionPayload::Text("λ".repeat(161))
            )),
            Err(SettingsError::InputTooLarge)
        );
        assert_eq!(state.revision(), 1);
        assert!(!state.is_dirty());
    }

    #[test]
    fn should_repair_corrupt_stored_settings_only_after_host_confirms_write() {
        let mut state = SettingsController::from_invalid_preference();
        assert!(state.is_dirty());
        assert_eq!(state.revision(), 1);
        assert!(state.notice().contains("Stored connection is invalid"));
        let intent = state
            .handle_action(&action(1, SAVE_NODE, SAVE, UiActionPayload::None))
            .unwrap();
        let SettingsIntent::Save(defaults) = intent else {
            panic!("repair needs an explicit write");
        };
        assert_eq!(defaults, OpenAiConnection::default());
        state.reject_save().unwrap();
        assert!(
            state.is_dirty(),
            "failed persistence must retain repair intent"
        );
        let retry = state
            .handle_action(&action(2, SAVE_NODE, SAVE, UiActionPayload::None))
            .unwrap();
        let SettingsIntent::Save(defaults) = retry else {
            panic!("repair should be retryable");
        };
        state.confirm_saved(&defaults).unwrap();
        assert!(!state.is_dirty());
        assert_eq!(state.saved(), &OpenAiConnection::default());
    }

    #[test]
    fn test_should_start_explicit_probe_and_choose_a_real_discovered_model() {
        let mut state = controller();
        assert!(!state.is_checking());
        let SettingsIntent::Check(connection) = state
            .handle_action(&action(1, CHECK_NODE, CHECK, UiActionPayload::None))
            .unwrap()
        else {
            panic!("a saved connection must be probed explicitly");
        };
        assert_eq!(connection, OpenAiConnection::default());
        assert!(state.is_checking());
        assert_eq!(
            state.handle_action(&action(2, CHECK_NODE, CHECK, UiActionPayload::None)),
            Err(SettingsError::CheckUnavailable)
        );
        assert!(state.snapshot().nodes.iter().any(|node| {
            node.id.as_str() == CHECK_NODE
                && matches!(&node.kind, UiNodeKind::Button(button) if !button.is_enabled)
        }));
        let models = crate::model_catalog::decode_catalog(
            br#"{"data":[{"id":"model-a"},{"id":"model-b"}]}"#,
        )
        .unwrap();
        state.finish_check(Ok(models)).unwrap();
        assert_eq!(state.revision(), 3);
        assert!(!state.is_checking());
        assert!(state.catalog().unwrap().includes("model-b"));
        assert_eq!(
            state.handle_action(&action(
                3,
                MODELS_SELECT_NODE,
                SELECT_MODEL,
                UiActionPayload::Text("unknown".to_owned())
            )),
            Err(SettingsError::UnknownModel)
        );
        let snapshot = state.snapshot();
        let list = snapshot
            .nodes
            .iter()
            .find(|node| node.id.as_str() == MODELS_SELECT_NODE)
            .unwrap();
        assert!(matches!(&list.kind, UiNodeKind::Select(select) if
            select.is_enabled && select.options.len() == 3));
        state
            .handle_action(&action(
                3,
                MODELS_SELECT_NODE,
                SELECT_MODEL,
                UiActionPayload::Text("model-b".to_owned()),
            ))
            .unwrap();
        assert_eq!(state.draft().model, "model-b");
        assert_eq!(state.saved().model, OpenAiConnection::default().model);
        assert!(state.is_dirty());
        assert!(state.catalog().is_none());
        let SettingsIntent::Save(descriptor) = state
            .handle_action(&action(4, SAVE_NODE, SAVE, UiActionPayload::None))
            .unwrap()
        else {
            panic!("selected model must require explicit save");
        };
        assert_eq!(descriptor.model, "model-b");
        state.confirm_saved(&descriptor).unwrap();
        assert!(!state.is_dirty());
    }

    #[test]
    fn test_should_keep_bad_status_and_timeout_out_of_connection_credentials() {
        let mut state = controller();
        state
            .handle_action(&action(1, CHECK_NODE, CHECK, UiActionPayload::None))
            .unwrap();
        state
            .finish_check(Err(ModelProbeError::Unauthorized))
            .unwrap();
        assert!(state.notice().contains("401"));
        assert!(!state.notice().contains("Bearer"));
        assert!(state.catalog().is_none());
        assert!(!state.is_checking());

        let mut dirty = controller();
        dirty
            .handle_action(&action(
                1,
                MODEL_NODE,
                SET_MODEL,
                UiActionPayload::Text("new".into()),
            ))
            .unwrap();
        assert_eq!(
            dirty.handle_action(&action(2, CHECK_NODE, CHECK, UiActionPayload::None)),
            Err(SettingsError::CheckUnavailable)
        );

        let mut repair = SettingsController::from_invalid_preference();
        assert_eq!(
            repair.handle_action(&action(1, CHECK_NODE, CHECK, UiActionPayload::None)),
            Err(SettingsError::CheckUnavailable)
        );
        state
            .handle_action(&action(3, CHECK_NODE, CHECK, UiActionPayload::None))
            .unwrap();
        state.finish_check(Err(ModelProbeError::Timeout)).unwrap();
        assert!(state.notice().contains("timed out"));
    }

    #[test]
    fn test_should_invalidate_outstanding_check_after_editing_draft() {
        let mut state = controller();
        state
            .handle_action(&action(1, CHECK_NODE, CHECK, UiActionPayload::None))
            .unwrap();
        state
            .handle_action(&action(
                2,
                MODEL_NODE,
                SET_MODEL,
                UiActionPayload::Text("different-model".into()),
            ))
            .unwrap();
        assert!(!state.is_checking());
        assert!(state.is_dirty());
        assert_eq!(
            state.finish_check(Ok(ModelCatalog {
                models: vec!["stale".into()],
                omitted: 0
            })),
            Err(SettingsError::CheckUnavailable)
        );
        assert!(state.catalog().is_none());
    }

    #[test]
    fn should_reject_wrong_surface_even_with_correct_action_identity() {
        let mut state = controller();
        let mut event = action(1, SAVE_NODE, SAVE, UiActionPayload::None);
        event.surface_id = UiSurfaceId::new("rintawa.another-provider.settings");
        assert_eq!(
            state.handle_action(&event),
            Err(SettingsError::UnknownAction)
        );
    }
}
