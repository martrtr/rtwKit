//! Package-manager runtime state, host actions, and portable UI composition.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    i18n::{Message, message},
    model::{
        BUILTIN_REPOSITORY, Catalog, CatalogPackage, PublishedAsset, RegistryIndex,
        RepositoryPreference, merge_catalog, normalize_repositories, parse_registry,
        solve_install_plan, validate_repository_url, validate_rtw_url,
    },
};

wit_bindgen::generate!({
    path: "../../wit",
    world: "task-runtime-plugin",
});

const SURFACE_ID: &str = "package-manager.main";
const REPOSITORIES_PREFERENCE: &str = "repositories.v1";
const MANAGED_INSTALLS_PREFERENCE: &str = "managed-installs.v1";
const MAX_REPOSITORIES: usize = 16;
const MAX_REGISTRY_BYTES: u32 = 4 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 40;
const MAX_INSTALLED_RESULTS: usize = 200;
const MAX_EMBEDDED_IMAGE_BASE64_BYTES: usize = 192 * 1024;
const MAX_EMBEDDED_README_BYTES: usize = 192 * 1024;
const MAX_CARD_DESCRIPTION_BYTES: usize = 512;
const MAX_CARD_AUTHORS_BYTES: usize = 256;
const MAX_SEARCH_QUERY_BYTES: usize = 512;
const MAX_REPOSITORY_INPUT_BYTES: usize = 2048;
const MAX_DIRECT_URL_INPUT_BYTES: usize = 4096;
const INITIAL_REFRESH_DELAY_MS: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Installed,
    Browse,
    Repositories,
    DirectUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstalledSortColumn {
    Enabled,
    Name,
    Version,
    Provider,
    Status,
}

impl InstalledSortColumn {
    fn key(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Name => "name",
            Self::Version => "version",
            Self::Provider => "provider",
            Self::Status => "status",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "enabled" => Ok(Self::Enabled),
            "name" => Ok(Self::Name),
            "version" => Ok(Self::Version),
            "provider" => Ok(Self::Provider),
            "status" => Ok(Self::Status),
            _ => Err(format!("unknown installed sort column '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InstalledStatus {
    Enabled,
    Disabled,
    UpdateAvailable,
    RequiredDependency,
    DependencyMetadataUnknown,
}

impl InstalledStatus {
    fn message(self) -> Message {
        match self {
            Self::Enabled => Message::StatusEnabled,
            Self::Disabled => Message::StatusDisabled,
            Self::UpdateAvailable => Message::StatusUpdateAvailable,
            Self::RequiredDependency => Message::StatusRequiredDependency,
            Self::DependencyMetadataUnknown => Message::StatusDependencyMetadataUnknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    fn value(self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
        }
    }

    fn reversed(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogFilter {
    All,
    Available,
    Installed,
    Updates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogSort {
    Name,
    Version,
    Repository,
}

impl CatalogFilter {
    fn value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Available => "available",
            Self::Installed => "installed",
            Self::Updates => "updates",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "all" => Ok(Self::All),
            "available" => Ok(Self::Available),
            "installed" => Ok(Self::Installed),
            "updates" => Ok(Self::Updates),
            _ => Err(format!("unknown catalog filter '{value}'")),
        }
    }
}

impl CatalogSort {
    fn value(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Version => "version",
            Self::Repository => "repository",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "name" => Ok(Self::Name),
            "version" => Ok(Self::Version),
            "repository" => Ok(Self::Repository),
            _ => Err(format!("unknown catalog sort '{value}'")),
        }
    }
}

#[derive(Debug, Clone)]
struct RepositoryStatus {
    loaded: bool,
    package_count: usize,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct InstalledPackage {
    subject: String,
    name: String,
    version_text: Option<String>,
    version: Option<Version>,
    digest: String,
    instance_id: String,
    scope_id: String,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct InstalledComponentPolicy {
    component_id: String,
    requested: Vec<String>,
    granted: Vec<String>,
}

#[derive(Debug, Clone)]
struct CachedImage {
    media_type: String,
    data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ManagedInstallRecord {
    version: String,
    dependencies: Vec<String>,
    #[serde(default)]
    dependencies_known: bool,
}

#[derive(Debug, Clone)]
struct PreparedPermission {
    component_id: String,
    permission: String,
    selected: bool,
}

#[derive(Debug, Clone)]
struct PreparedInstallItem {
    digest: String,
    subject: String,
    name: String,
    version: String,
    origin: String,
    dependencies: Option<Vec<String>>,
    permissions: Vec<PreparedPermission>,
}

#[derive(Debug, Clone)]
struct PendingInstall {
    origin: String,
    items: Vec<PreparedInstallItem>,
}

#[derive(Debug, Clone)]
enum ActionTarget {
    Package(String),
    Activation(String),
    Repository(usize),
    Permission { item: usize, permission: usize },
}

struct ManagerState {
    repositories: Vec<RepositoryPreference>,
    loaded_repositories: BTreeMap<String, RegistryIndex>,
    repository_status: BTreeMap<String, RepositoryStatus>,
    catalog: Catalog,
    installed: BTreeMap<String, InstalledPackage>,
    installed_policy: BTreeMap<(String, String), Vec<InstalledComponentPolicy>>,
    installed_policy_error: Option<String>,
    managed_installs: BTreeMap<String, ManagedInstallRecord>,
    view: View,
    view_history: Vec<View>,
    search: String,
    installed_search: String,
    installed_sort: InstalledSortColumn,
    installed_sort_direction: SortDirection,
    repository_filter: Option<String>,
    catalog_filter: CatalogFilter,
    catalog_sort: CatalogSort,
    selected_package: Option<String>,
    selected_version: Option<Version>,
    selected_packages: BTreeSet<String>,
    logo_cache: BTreeMap<String, Result<CachedImage, String>>,
    readme_cache: BTreeMap<String, Result<String, String>>,
    repository_input: String,
    direct_url: String,
    pending_install: Option<PendingInstall>,
    refresh_task: Option<u64>,
    status: String,
    revision: u64,
    mounted: bool,
    previous_node_ids: BTreeSet<String>,
    action_targets: BTreeMap<String, ActionTarget>,
}

impl Default for ManagerState {
    fn default() -> Self {
        Self {
            repositories: vec![RepositoryPreference::builtin()],
            loaded_repositories: BTreeMap::new(),
            repository_status: BTreeMap::new(),
            catalog: Catalog {
                packages: BTreeMap::new(),
                warnings: Vec::new(),
            },
            installed: BTreeMap::new(),
            installed_policy: BTreeMap::new(),
            installed_policy_error: None,
            managed_installs: BTreeMap::new(),
            view: View::Installed,
            view_history: Vec::new(),
            search: String::new(),
            installed_search: String::new(),
            installed_sort: InstalledSortColumn::Name,
            installed_sort_direction: SortDirection::Ascending,
            repository_filter: None,
            catalog_filter: CatalogFilter::All,
            catalog_sort: CatalogSort::Name,
            selected_package: None,
            selected_version: None,
            selected_packages: BTreeSet::new(),
            logo_cache: BTreeMap::new(),
            readme_cache: BTreeMap::new(),
            repository_input: String::new(),
            direct_url: String::new(),
            pending_install: None,
            refresh_task: None,
            status: "Preparing repository refresh…".to_string(),
            revision: 0,
            mounted: false,
            previous_node_ids: BTreeSet::new(),
            action_targets: BTreeMap::new(),
        }
    }
}

thread_local! {
    static STATE: RefCell<ManagerState> = RefCell::new(ManagerState::default());
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[derive(Debug, Deserialize)]
struct UiActionEvent {
    node_id: String,
    action_id: String,
    payload: UiActionPayload,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
enum UiActionPayload {
    None,
    Text(String),
    Boolean(bool),
}

struct PackageManager;

impl exports::rintawa::engine::guest::Guest for PackageManager {
    fn register() {
        let capabilities = vec![
            "rintawa.ui.text@1".to_string(),
            "rintawa.ui.markdown@1".to_string(),
            "rintawa.ui.button@1".to_string(),
            "rintawa.ui.icon@1".to_string(),
            "rintawa.ui.image@1".to_string(),
            "rintawa.ui.input.checkbox@1".to_string(),
            "rintawa.ui.input.select@1".to_string(),
            "rintawa.ui.input.text@1".to_string(),
            "rintawa.ui.layout.split@1".to_string(),
            "rintawa.ui.layout.row@1".to_string(),
            "rintawa.ui.layout.column@1".to_string(),
            "rintawa.ui.list@1".to_string(),
            "rintawa.ui.data-grid@1".to_string(),
        ];
        let activity = rintawa::engine::portable_ui::Activity {
            id: "extensions".to_string(),
            label: "Extensions".to_string(),
            icon_slot: Some("activity.extensions".to_string()),
        };
        let traits = vec![
            "workspace-tool".to_string(),
            "settings".to_string(),
            "navigable".to_string(),
            "inspectable".to_string(),
        ];
        let registration = rintawa::engine::portable_ui::register_surface(
            SURFACE_ID,
            rintawa::engine::portable_ui::PlacementHint::Settings,
            Some("management.extensions"),
            Some(1),
            Some(&activity),
            &traits,
            &capabilities,
        );
        REGISTRATION_ERROR.with(|slot| {
            *slot.borrow_mut() = registration
                .err()
                .map(|error| format!("Package Manager UI registration failed: {error:?}"));
        });
    }

    fn start() {
        STATE.with(|state| {
            *state.borrow_mut() = ManagerState::default();
        });
        let registration_error = REGISTRATION_ERROR.with(|slot| slot.borrow().clone());
        if let Some(error) = registration_error {
            set_status(error.clone());
            rintawa::engine::host::log(rintawa::engine::host::LogLevel::Error, &error);
            return;
        }
        load_repository_preferences();
        load_managed_installs();
        refresh_installed();
        schedule_initial_refresh();
        render_surface();
    }

    fn stop() {
        let task = STATE.with(|state| state.borrow_mut().refresh_task.take());
        if let Some(task) = task {
            let _ = rintawa::engine::runtime_tasks::cancel(task);
        }
        let _ = rintawa::engine::portable_ui::unmount_surface(SURFACE_ID);
        STATE.with(|state| {
            state.borrow_mut().mounted = false;
        });
    }

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_ui_action(action_json: Vec<u8>) {
        let result = serde_json::from_slice::<UiActionEvent>(&action_json)
            .map_err(|error| format!("Invalid UI action: {error}"))
            .and_then(dispatch_action);

        if let Err(error) = result {
            set_status(format!("Error: {error}"));
        }
        render_surface();
    }

    fn handle_service(_contract: String, _version: u32, _payload: Vec<u8>) -> Vec<u8> {
        Vec::new()
    }
}

impl exports::rintawa::engine::task_handler::Guest for PackageManager {
    fn on_task(handle: u64) {
        let is_refresh = STATE.with(|state| state.borrow().refresh_task == Some(handle));
        if !is_refresh {
            return;
        }

        let _ = rintawa::engine::runtime_tasks::cancel(handle);
        STATE.with(|state| state.borrow_mut().refresh_task = None);
        if let Err(error) = refresh_repositories() {
            set_status(format!("Repository refresh failed: {error}"));
        }
        render_surface();
    }
}

fn schedule_initial_refresh() {
    match rintawa::engine::runtime_tasks::spawn_periodic(INITIAL_REFRESH_DELAY_MS) {
        Ok(handle) => {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.refresh_task = Some(handle);
                state.status = "Refreshing repositories…".to_string();
            });
        }
        Err(error) => set_status(format!(
            "Automatic repository refresh unavailable ({error:?}). Use Refresh repositories."
        )),
    }
}

fn load_repository_preferences() {
    let stored = match rintawa::engine::preferences::get(REPOSITORIES_PREFERENCE) {
        Ok(Some(source)) => match serde_json::from_str::<Vec<RepositoryPreference>>(&source) {
            Ok(repositories) => Some(repositories),
            Err(error) => {
                set_status(format!(
                    "Stored repository settings were invalid and were reset: {error}"
                ));
                None
            }
        },
        Ok(None) => None,
        Err(error) => {
            set_status(format!(
                "Could not read repository settings ({error:?}); using defaults"
            ));
            None
        }
    };

    STATE.with(|state| {
        state.borrow_mut().repositories = normalize_repositories(stored);
    });
}

fn persist_repositories() -> Result<(), String> {
    let repositories = STATE.with(|state| state.borrow().repositories.clone());
    let source = serde_json::to_string(&repositories)
        .map_err(|error| format!("could not encode repository settings: {error}"))?;
    rintawa::engine::preferences::set(REPOSITORIES_PREFERENCE, &source)
        .map_err(|error| format!("could not persist repository settings: {error:?}"))
}

fn load_managed_installs() {
    let managed = match rintawa::engine::preferences::get(MANAGED_INSTALLS_PREFERENCE) {
        Ok(Some(source)) => serde_json::from_str::<BTreeMap<String, ManagedInstallRecord>>(&source)
            .unwrap_or_default(),
        Ok(None) => BTreeMap::new(),
        Err(_) => BTreeMap::new(),
    };
    STATE.with(|state| state.borrow_mut().managed_installs = managed);
}

fn persist_managed_installs() -> Result<(), String> {
    let managed = STATE.with(|state| state.borrow().managed_installs.clone());
    let source = serde_json::to_string(&managed)
        .map_err(|error| format!("could not encode managed install state: {error}"))?;
    rintawa::engine::preferences::set(MANAGED_INSTALLS_PREFERENCE, &source)
        .map_err(|error| format!("could not persist managed install state: {error:?}"))
}

fn refresh_installed() {
    match rintawa::engine::composition::list_activations() {
        Ok(activations) => {
            let installed = activations
                .into_iter()
                .map(|activation| {
                    let version_text = activation.version.clone();
                    let version = version_text
                        .as_deref()
                        .and_then(|value| Version::parse(value).ok());
                    (
                        activation.subject.clone(),
                        InstalledPackage {
                            subject: activation.subject,
                            name: activation.name,
                            version_text,
                            version,
                            digest: activation.digest,
                            instance_id: activation.instance_id,
                            scope_id: activation.scope_id,
                            enabled: activation.enabled,
                        },
                    )
                })
                .collect();
            STATE.with(|state| state.borrow_mut().installed = installed);
        }
        Err(error) => set_status(format!(
            "Composition is unavailable ({error:?}). Grant composition-read to Package Manager."
        )),
    }
    refresh_installed_policy();
}

fn refresh_installed_policy() {
    match rintawa::engine::runtime_policy::list_components() {
        Ok(entries) => {
            let mut policy = BTreeMap::<(String, String), Vec<InstalledComponentPolicy>>::new();
            for entry in entries {
                policy
                    .entry((entry.scope_id, entry.instance_id))
                    .or_default()
                    .push(InstalledComponentPolicy {
                        component_id: entry.component_id,
                        requested: entry.requested,
                        granted: entry.granted,
                    });
            }
            for components in policy.values_mut() {
                components.sort_by(|left, right| left.component_id.cmp(&right.component_id));
                for component in components {
                    component.requested.sort();
                    component.granted.sort();
                }
            }
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.installed_policy = policy;
                state.installed_policy_error = None;
            });
        }
        Err(error) => {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.installed_policy.clear();
                state.installed_policy_error =
                    Some(format!("Runtime policy is unavailable: {error:?}"));
            });
        }
    }
}

fn refresh_repositories() -> Result<(), String> {
    let repositories = STATE.with(|state| state.borrow().repositories.clone());
    let enabled: Vec<_> = repositories
        .iter()
        .filter(|repository| repository.enabled)
        .cloned()
        .collect();

    let mut loaded = BTreeMap::new();
    let mut statuses = BTreeMap::new();
    let mut loaded_count = 0usize;
    for repository in enabled {
        match fetch_registry(&repository.url) {
            Ok(index) => {
                loaded_count += 1;
                statuses.insert(
                    repository.url.clone(),
                    RepositoryStatus {
                        loaded: true,
                        package_count: index.packages.len(),
                        error: None,
                    },
                );
                loaded.insert(repository.url, index);
            }
            Err(error) => {
                statuses.insert(
                    repository.url,
                    RepositoryStatus {
                        loaded: false,
                        package_count: 0,
                        error: Some(error),
                    },
                );
            }
        }
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.loaded_repositories = loaded;
        state.repository_status = statuses;
        rebuild_catalog_locked(&mut state);
        let package_count = state.catalog.packages.len();
        let warning_count = state.catalog.warnings.len();
        state.status = if warning_count == 0 {
            format!("Loaded {package_count} packages from {loaded_count} repositories.")
        } else {
            format!(
                "Loaded {package_count} packages from {loaded_count} repositories ({warning_count} priority warnings)."
            )
        };
    });
    Ok(())
}

fn fetch_registry(url: &str) -> Result<RegistryIndex, String> {
    validate_repository_url(url)?;
    let response = rintawa::engine::http_fetch::get(url, MAX_REGISTRY_BYTES)
        .map_err(|error| format!("fetch failed: {error:?}"))?;
    if response.status != 200 {
        return Err(format!("repository returned HTTP {}", response.status));
    }
    parse_registry(&response.body)
}

fn rebuild_catalog_locked(state: &mut ManagerState) {
    let ordered = state
        .repositories
        .iter()
        .filter(|repository| repository.enabled)
        .filter_map(|repository| {
            state
                .loaded_repositories
                .get(&repository.url)
                .cloned()
                .map(|index| (repository.url.clone(), index))
        })
        .collect::<Vec<_>>();
    state.catalog = merge_catalog(ordered);
}

fn dispatch_action(event: UiActionEvent) -> Result<(), String> {
    let has_pending = STATE.with(|state| state.borrow().pending_install.is_some());
    if has_pending
        && !matches!(
            event.action_id.as_str(),
            "install.confirm" | "install.cancel" | "install.permission-toggle"
        )
    {
        return Err("finish or cancel the pending installation first".to_string());
    }

    match event.action_id.as_str() {
        "install.confirm" => confirm_pending_install()?,
        "install.cancel" => cancel_pending_install(),
        "install.permission-toggle" => {
            let (item, permission) = target_permission(&event.node_id)?;
            let selected = boolean_payload(event.payload)?;
            set_pending_permission(item, permission, selected)?;
        }
        "view.browse" => open_view(View::Browse),
        "view.installed" => reset_view(View::Installed),
        "view.repositories" => open_view(View::Repositories),
        "view.direct-url" => open_view(View::DirectUrl),
        "view.close" => close_view(),
        "catalog.refresh" => refresh_repositories()?,
        "search.change" => {
            let value =
                bounded_text_payload(event.payload, MAX_SEARCH_QUERY_BYTES, "catalog search")?;
            STATE.with(|state| state.borrow_mut().search = value);
        }
        "installed.search" => {
            let value =
                bounded_text_payload(event.payload, MAX_SEARCH_QUERY_BYTES, "installed search")?;
            set_installed_search(value);
        }
        "installed.sort" => {
            let value = text_payload(event.payload)?;
            set_installed_sort(InstalledSortColumn::parse(&value)?);
        }
        "installed.row-select" => {
            let subject = text_payload(event.payload)?;
            select_installed_package(&subject)?;
        }
        "catalog.filter" => {
            let value = text_payload(event.payload)?;
            let filter = CatalogFilter::parse(&value)?;
            STATE.with(|state| state.borrow_mut().catalog_filter = filter);
        }
        "catalog.sort" => {
            let value = text_payload(event.payload)?;
            let sort = CatalogSort::parse(&value)?;
            STATE.with(|state| state.borrow_mut().catalog_sort = sort);
        }
        "catalog.repository" => {
            let value = text_payload(event.payload)?;
            STATE.with(|state| {
                state.borrow_mut().repository_filter =
                    if value == "all" { None } else { Some(value) };
            });
        }
        "catalog.repository-all" => {
            STATE.with(|state| state.borrow_mut().repository_filter = None);
        }
        "catalog.repository-target" => {
            let index = target_repository(&event.node_id)?;
            STATE.with(|state| -> Result<(), String> {
                let mut state = state.borrow_mut();
                let url = state
                    .repositories
                    .get(index)
                    .ok_or_else(|| "repository no longer exists".to_string())?
                    .url
                    .clone();
                state.repository_filter = Some(url);
                Ok(())
            })?;
        }
        "selection.clear" => {
            STATE.with(|state| state.borrow_mut().selected_packages.clear());
        }
        "selection.install" => install_selected_catalog_packages()?,
        "package.details" => {
            let package_id = target_package(&event.node_id)?;
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.selected_version = state
                    .catalog
                    .packages
                    .get(&package_id)
                    .map(|package| package.latest.clone());
                state.selected_package = Some(package_id);
                if state.view != View::Browse {
                    let current = state.view;
                    state.view_history.push(current);
                    state.view = View::Browse;
                }
            });
        }
        "package.selection" => {
            let package_id = target_package(&event.node_id)?;
            let selected = boolean_payload(event.payload)?;
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                if selected {
                    state.selected_packages.insert(package_id);
                } else {
                    state.selected_packages.remove(&package_id);
                }
            });
        }
        "package.version" => {
            let package_id = target_package(&event.node_id)?;
            let value = text_payload(event.payload)?;
            let version = Version::parse(&value)
                .map_err(|error| format!("invalid package version '{value}': {error}"))?;
            STATE.with(|state| -> Result<(), String> {
                let mut state = state.borrow_mut();
                let package = state
                    .catalog
                    .packages
                    .get(&package_id)
                    .ok_or_else(|| format!("package '{package_id}' disappeared"))?;
                if !package
                    .versions
                    .iter()
                    .any(|release| release.version == version)
                {
                    return Err(format!(
                        "package '{package_id}' does not contain version {version}"
                    ));
                }
                state.selected_package = Some(package_id);
                state.selected_version = Some(version);
                Ok(())
            })?;
        }
        "repository.input" => {
            let value =
                bounded_text_payload(event.payload, MAX_REPOSITORY_INPUT_BYTES, "repository URL")?;
            STATE.with(|state| state.borrow_mut().repository_input = value);
        }
        "repository.add" => add_repository()?,
        "repository.toggle" => {
            let index = target_repository(&event.node_id)?;
            toggle_repository(index)?;
        }
        "repository.remove" => {
            let index = target_repository(&event.node_id)?;
            remove_repository(index)?;
        }
        "direct.input" => {
            let value =
                bounded_text_payload(event.payload, MAX_DIRECT_URL_INPUT_BYTES, "direct RTW URL")?;
            STATE.with(|state| state.borrow_mut().direct_url = value);
        }
        "direct.install" => install_direct_url()?,
        "package.install" | "package.update" => {
            let package_id = target_package(&event.node_id)?;
            install_catalog_package(&package_id, None)?;
        }
        "package.install-version" => {
            let package_id = target_package(&event.node_id)?;
            let version = STATE.with(|state| state.borrow().selected_version.clone());
            install_catalog_package(&package_id, version.as_ref())?;
        }
        "installed.details" => {
            let subject = target_activation(&event.node_id)?;
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.selected_version = state
                    .installed
                    .get(&subject)
                    .and_then(|package| package.version.clone());
                state.selected_package = Some(subject);
                state.view = View::Installed;
            });
        }
        "package.uninstall" => {
            let subject = target_activation(&event.node_id)?;
            uninstall_package(&subject)?;
        }
        "package.enabled" => {
            let subject = target_activation(&event.node_id)?;
            let enabled = boolean_payload(event.payload)?;
            set_package_enabled(&subject, enabled)?;
        }
        "package.toggle" => {
            let subject = target_activation(&event.node_id)?;
            toggle_package_enabled(&subject)?;
        }
        other => return Err(format!("unknown UI action '{other}'")),
    }
    Ok(())
}

fn text_payload(payload: UiActionPayload) -> Result<String, String> {
    match payload {
        UiActionPayload::Text(value) => Ok(value),
        UiActionPayload::None | UiActionPayload::Boolean(_) => {
            Err("text action did not include text".to_string())
        }
    }
}

fn bounded_text_payload(
    payload: UiActionPayload,
    maximum: usize,
    label: &str,
) -> Result<String, String> {
    let value = text_payload(payload)?;
    if value.len() > maximum {
        return Err(format!("{label} exceeds {maximum} bytes"));
    }
    Ok(value)
}

fn boolean_payload(payload: UiActionPayload) -> Result<bool, String> {
    match payload {
        UiActionPayload::Boolean(value) => Ok(value),
        UiActionPayload::None | UiActionPayload::Text(_) => {
            Err("boolean action did not include a boolean".to_string())
        }
    }
}

fn open_view(view: View) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.view == view {
            return;
        }
        let current = state.view;
        state.view_history.push(current);
        state.view = view;
    });
}

fn reset_view(view: View) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.view = view;
        state.view_history.clear();
    });
}

fn close_view() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.view = state.view_history.pop().unwrap_or(View::Installed);
    });
}

fn set_installed_sort(column: InstalledSortColumn) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.installed_sort == column {
            state.installed_sort_direction = state.installed_sort_direction.reversed();
        } else {
            state.installed_sort = column;
            state.installed_sort_direction = SortDirection::Ascending;
        }
    });
}

fn select_installed_package(subject: &str) -> Result<(), String> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let version = state
            .installed
            .get(subject)
            .ok_or_else(|| format!("installed extension '{subject}' no longer exists"))?
            .version
            .clone();
        state.selected_package = Some(subject.to_string());
        state.selected_version = version;
        state.view = View::Installed;
        Ok(())
    })
}

fn set_installed_search(search: String) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.installed_search = search;
        let query = state.installed_search.trim().to_lowercase();
        let should_clear_selection = state.selected_package.as_deref().is_some_and(|subject| {
            state
                .installed
                .get(subject)
                .is_none_or(|package| !installed_package_matches_search(&state, package, &query))
        });
        if should_clear_selection {
            state.selected_package = None;
            state.selected_version = None;
        }
    });
}

fn set_status(status: String) {
    STATE.with(|state| state.borrow_mut().status = status);
}

fn target_package(node_id: &str) -> Result<String, String> {
    STATE.with(|state| match state.borrow().action_targets.get(node_id) {
        Some(ActionTarget::Package(id)) => Ok(id.clone()),
        _ => Err("package action target is stale".to_string()),
    })
}

fn target_activation(node_id: &str) -> Result<String, String> {
    STATE.with(|state| match state.borrow().action_targets.get(node_id) {
        Some(ActionTarget::Activation(id)) => Ok(id.clone()),
        _ => Err("activation action target is stale".to_string()),
    })
}

fn target_repository(node_id: &str) -> Result<usize, String> {
    STATE.with(|state| match state.borrow().action_targets.get(node_id) {
        Some(ActionTarget::Repository(index)) => Ok(*index),
        _ => Err("repository action target is stale".to_string()),
    })
}

fn target_permission(node_id: &str) -> Result<(usize, usize), String> {
    STATE.with(|state| match state.borrow().action_targets.get(node_id) {
        Some(ActionTarget::Permission { item, permission }) => Ok((*item, *permission)),
        _ => Err("permission action target is stale".to_string()),
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DependencyBlockers {
    required_by: Vec<String>,
    unknown: Vec<String>,
}

fn dependency_info_for_locked(state: &ManagerState, subject: &str) -> (BTreeSet<String>, bool) {
    let mut dependencies = BTreeSet::new();
    let mut known = false;
    let Some(installed) = state.installed.get(subject) else {
        return (dependencies, false);
    };

    if let Some(record) = state.managed_installs.get(subject)
        && installed.version_text.as_deref() == Some(record.version.as_str())
    {
        dependencies.extend(record.dependencies.iter().cloned());
        known |= record.dependencies_known;
    }

    if let Some(package) = state.catalog.packages.get(subject)
        && let Some(version) = installed.version.as_ref()
        && let Some(release) = package
            .versions
            .iter()
            .find(|release| &release.version == version)
    {
        dependencies.extend(
            release
                .dependencies
                .iter()
                .map(|dependency| dependency.id.clone()),
        );
        known = true;
    }

    (dependencies, known)
}

fn dependency_blockers_locked(state: &ManagerState, subject: &str) -> DependencyBlockers {
    let mut blockers = DependencyBlockers::default();
    for dependent in state.installed.keys() {
        if dependent == subject {
            continue;
        }
        let (dependencies, known) = dependency_info_for_locked(state, dependent);
        if dependencies.contains(subject) {
            blockers.required_by.push(dependent.clone());
        } else if !known {
            blockers.unknown.push(dependent.clone());
        }
    }
    blockers.required_by.sort();
    blockers.unknown.sort();
    blockers
}

fn dependency_blockers(subject: &str) -> DependencyBlockers {
    STATE.with(|state| dependency_blockers_locked(&state.borrow(), subject))
}

fn dependency_issues_for_enable_locked(
    state: &ManagerState,
    subject: &str,
) -> Result<Vec<String>, String> {
    let (dependencies, known) = dependency_info_for_locked(state, subject);
    if !known {
        return Err(format!(
            "dependency metadata for exact activation '{subject}' is unavailable"
        ));
    }
    Ok(dependencies
        .into_iter()
        .filter_map(|dependency| match state.installed.get(&dependency) {
            None => Some(format!("{dependency} (not installed)")),
            Some(package) if !package.enabled => Some(format!("{dependency} (disabled)")),
            Some(_) => None,
        })
        .collect())
}

fn dependency_issues_for_enable(subject: &str) -> Result<Vec<String>, String> {
    STATE.with(|state| dependency_issues_for_enable_locked(&state.borrow(), subject))
}

fn ensure_no_dependency_blockers(subject: &str, operation: &str) -> Result<(), String> {
    let blockers = dependency_blockers(subject);
    if !blockers.required_by.is_empty() {
        return Err(format!(
            "cannot {operation} '{subject}'; installed packages depend on it: {}",
            blockers.required_by.join(", ")
        ));
    }
    if !blockers.unknown.is_empty() {
        return Err(format!(
            "cannot safely {operation} '{subject}'; dependency metadata is unavailable for installed packages: {}",
            blockers.unknown.join(", ")
        ));
    }
    Ok(())
}

fn uninstall_package(subject: &str) -> Result<(), String> {
    ensure_no_dependency_blockers(subject, "uninstall")?;

    let before_activations = snapshot_activations()?;
    let before_policy = snapshot_runtime_policy()?;
    let before_managed = STATE.with(|state| state.borrow().managed_installs.clone());

    rintawa::engine::composition::remove_activation(subject)
        .map_err(|error| format!("uninstall failed: {error:?}"))?;
    STATE.with(|state| {
        state.borrow_mut().managed_installs.remove(subject);
    });

    if let Err(error) = persist_managed_installs() {
        STATE.with(|state| state.borrow_mut().managed_installs = before_managed);
        return install_failure_with_rollback(
            error,
            &before_activations,
            &before_policy,
            &[subject.to_string()],
        );
    }

    refresh_installed();
    set_status(format!(
        "Removed {subject} from the baseline composition. Running code stops on restart."
    ));
    Ok(())
}

fn set_package_enabled(subject: &str, enabled: bool) -> Result<(), String> {
    let current = STATE.with(|state| {
        state
            .borrow()
            .installed
            .get(subject)
            .map(|package| package.enabled)
    });
    let current = current.ok_or_else(|| format!("activation '{subject}' disappeared"))?;
    if current == enabled {
        return Ok(());
    }

    if enabled {
        let issues = dependency_issues_for_enable(subject)?;
        if !issues.is_empty() {
            return Err(format!(
                "cannot enable '{subject}'; dependencies are unavailable: {}",
                issues.join(", ")
            ));
        }
    } else {
        ensure_no_dependency_blockers(subject, "disable")?;
    }

    rintawa::engine::composition::set_enabled(subject, enabled)
        .map_err(|error| format!("could not change enabled state: {error:?}"))?;
    refresh_installed();
    set_status(format!(
        "{} {subject}. Change takes full effect on restart.",
        if enabled { "Enabled" } else { "Disabled" }
    ));
    Ok(())
}

fn toggle_package_enabled(subject: &str) -> Result<(), String> {
    let enabled = STATE.with(|state| {
        state
            .borrow()
            .installed
            .get(subject)
            .map(|package| !package.enabled)
    });
    let enabled = enabled.ok_or_else(|| format!("activation '{subject}' disappeared"))?;
    set_package_enabled(subject, enabled)
}

fn add_repository() -> Result<(), String> {
    let url = STATE.with(|state| state.borrow().repository_input.trim().to_string());
    validate_repository_url(&url)?;

    STATE.with(|state| -> Result<(), String> {
        let mut state = state.borrow_mut();
        if state.repositories.iter().any(|item| item.url == url) {
            return Err("repository is already registered".to_string());
        }
        if state.repositories.len() >= MAX_REPOSITORIES {
            return Err(format!(
                "repository limit of {MAX_REPOSITORIES} has been reached"
            ));
        }
        state.repositories.push(RepositoryPreference {
            url: url.clone(),
            enabled: true,
        });
        state.repository_input.clear();
        Ok(())
    })?;

    persist_repositories()?;
    set_status(format!("Added repository {url}. Use Refresh to load it."));
    Ok(())
}

fn toggle_repository(index: usize) -> Result<(), String> {
    let (url, enabled) = STATE.with(|state| -> Result<(String, bool), String> {
        let mut state = state.borrow_mut();
        let repository = state
            .repositories
            .get_mut(index)
            .ok_or_else(|| "repository no longer exists".to_string())?;
        repository.enabled = !repository.enabled;
        let result = (repository.url.clone(), repository.enabled);
        rebuild_catalog_locked(&mut state);
        Ok(result)
    })?;
    persist_repositories()?;
    set_status(format!(
        "{} repository {url}.",
        if enabled { "Enabled" } else { "Disabled" }
    ));
    Ok(())
}

fn remove_repository(index: usize) -> Result<(), String> {
    let url = STATE.with(|state| -> Result<String, String> {
        let mut state = state.borrow_mut();
        let repository = state
            .repositories
            .get(index)
            .ok_or_else(|| "repository no longer exists".to_string())?;
        if repository.url == BUILTIN_REPOSITORY {
            return Err(
                "the built-in rtwKit repository can be disabled but not removed".to_string(),
            );
        }
        let removed = state.repositories.remove(index);
        state.loaded_repositories.remove(&removed.url);
        state.repository_status.remove(&removed.url);
        rebuild_catalog_locked(&mut state);
        Ok(removed.url)
    })?;
    persist_repositories()?;
    set_status(format!("Removed repository {url}."));
    Ok(())
}

fn install_selected_catalog_packages() -> Result<(), String> {
    refresh_installed();
    let (selected, catalog, mut simulated_installed, disabled) = STATE.with(|state| {
        let state = state.borrow();
        let simulated = state
            .installed
            .iter()
            .filter_map(|(id, package)| {
                package.version.clone().map(|version| (id.clone(), version))
            })
            .collect::<BTreeMap<_, _>>();
        let disabled = state
            .installed
            .iter()
            .filter(|(_, package)| !package.enabled)
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        (
            state.selected_packages.iter().cloned().collect::<Vec<_>>(),
            state.catalog.clone(),
            simulated,
            disabled,
        )
    });

    if selected.is_empty() {
        return Err("no catalog packages are selected".to_string());
    }

    let mut planned_versions = BTreeMap::<String, Version>::new();
    let mut selections = Vec::new();
    for root in &selected {
        let plan = solve_install_plan(&catalog, &simulated_installed, root, None)?;
        let disabled_dependencies = plan
            .resolved
            .keys()
            .filter(|id| id.as_str() != root)
            .filter(|id| disabled.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        if !disabled_dependencies.is_empty() {
            return Err(format!(
                "cannot install '{root}'; required dependencies are disabled: {}",
                disabled_dependencies.join(", ")
            ));
        }

        for selection in plan.selections {
            let version = selection.version.version.clone();
            if let Some(existing) = planned_versions.get(&selection.package_id) {
                if existing != &version {
                    return Err(format!(
                        "selected packages require conflicting versions of '{}': {} and {}",
                        selection.package_id, existing, version
                    ));
                }
                continue;
            }
            simulated_installed.insert(selection.package_id.clone(), version.clone());
            planned_versions.insert(selection.package_id.clone(), version);
            selections.push(selection);
        }

        for (id, version) in plan.resolved {
            simulated_installed.insert(id, version);
        }
    }

    if selections.is_empty() {
        set_status("All selected extensions are already at compatible versions.".to_string());
        return Ok(());
    }

    set_status(format!(
        "Preparing {} package{} from {} selected extension{}…",
        selections.len(),
        if selections.len() == 1 { "" } else { "s" },
        selected.len(),
        if selected.len() == 1 { "" } else { "s" }
    ));

    let mut items = Vec::new();
    for selection in &selections {
        items.push(prepare_catalog_selection(selection)?);
    }
    STATE.with(|state| {
        state.borrow_mut().pending_install = Some(PendingInstall {
            origin: format!("{} selected repository extensions", selected.len()),
            items,
        });
    });
    set_status("Review requested permissions before installing selected extensions.".to_string());
    Ok(())
}

fn install_catalog_package(package_id: &str, target: Option<&Version>) -> Result<(), String> {
    refresh_installed();

    let (catalog, installed_versions) = STATE.with(|state| {
        let state = state.borrow();
        let versions = state
            .installed
            .iter()
            .filter_map(|(id, package)| {
                package.version.clone().map(|version| (id.clone(), version))
            })
            .collect();
        (state.catalog.clone(), versions)
    });
    let plan = solve_install_plan(&catalog, &installed_versions, package_id, target)?;
    let disabled_dependencies = STATE.with(|state| {
        let state = state.borrow();
        plan.resolved
            .keys()
            .filter(|id| id.as_str() != package_id)
            .filter(|id| {
                state
                    .installed
                    .get(id.as_str())
                    .is_some_and(|package| !package.enabled)
            })
            .cloned()
            .collect::<Vec<_>>()
    });
    if !disabled_dependencies.is_empty() {
        return Err(format!(
            "cannot install '{package_id}'; required dependencies are disabled: {}",
            disabled_dependencies.join(", ")
        ));
    }
    if plan.selections.is_empty() {
        set_status(format!("{package_id} is already at the selected version."));
        return Ok(());
    }

    set_status(format!(
        "Preparing {} package{} for installation…",
        plan.selections.len(),
        if plan.selections.len() == 1 { "" } else { "s" }
    ));

    let mut items = Vec::new();
    for selection in &plan.selections {
        items.push(prepare_catalog_selection(selection)?);
    }

    let origin = format!("repository package {package_id}");
    STATE.with(|state| {
        state.borrow_mut().pending_install = Some(PendingInstall { origin, items });
    });
    set_status("Review requested permissions before installing.".to_string());
    Ok(())
}

fn prepare_catalog_selection(
    selection: &crate::model::InstallSelection,
) -> Result<PreparedInstallItem, String> {
    let digest = download_and_import(selection)?;
    let policy = rintawa::engine::runtime_policy::inspect_artifact(&digest)
        .map_err(|error| format!("could not inspect imported RTW: {error:?}"))?;
    let expected_version = selection.version.version.to_string();
    if policy.subject != selection.package_id || policy.version != expected_version {
        return Err(format!(
            "artifact identity mismatch: repository advertised {} {}, RTW contains {} {}",
            selection.package_id, expected_version, policy.subject, policy.version
        ));
    }

    let source = &selection.version.source;
    let origin = format!(
        "repository {} · artifact {} · source {}@{} ({})",
        selection.repository_url,
        selection.version.artifact.url,
        source.repository,
        source.tag,
        source.commit
    );
    let dependencies = selection
        .version
        .dependencies
        .iter()
        .map(|dependency| dependency.id.clone())
        .collect();
    Ok(prepared_item_from_policy(
        digest,
        policy,
        origin,
        Some(dependencies),
    ))
}

fn prepared_item_from_policy(
    digest: String,
    policy: rintawa::engine::runtime_policy::ArtifactPolicy,
    origin: String,
    dependencies: Option<Vec<String>>,
) -> PreparedInstallItem {
    let mut seen = BTreeSet::new();
    let mut permissions = Vec::new();
    for component in policy.components {
        for permission in component.requested {
            if seen.insert((component.component_id.clone(), permission.clone())) {
                permissions.push(PreparedPermission {
                    component_id: component.component_id.clone(),
                    permission,
                    selected: true,
                });
            }
        }
    }

    PreparedInstallItem {
        digest,
        subject: policy.subject,
        name: policy.name,
        version: policy.version,
        origin,
        dependencies,
        permissions,
    }
}

fn download_and_import(selection: &crate::model::InstallSelection) -> Result<String, String> {
    let artifact = &selection.version.artifact;
    if artifact.size > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "{} {} is {} bytes; Package Manager limit is {} bytes",
            selection.package_id, selection.version.version, artifact.size, MAX_ARTIFACT_BYTES
        ));
    }
    let max_bytes = u32::try_from(artifact.size)
        .map_err(|_| "artifact size does not fit fetch limit".to_string())?;
    let response = rintawa::engine::http_fetch::get(&artifact.url, max_bytes)
        .map_err(|error| format!("download of {} failed: {error:?}", selection.package_id))?;
    if response.status != 200 {
        return Err(format!(
            "download of {} returned HTTP {}",
            selection.package_id, response.status
        ));
    }
    if response.body.len() as u64 != artifact.size {
        return Err(format!(
            "{} size mismatch: registry {}, downloaded {}",
            selection.package_id,
            artifact.size,
            response.body.len()
        ));
    }

    let actual = format!("sha256:{:x}", Sha256::digest(&response.body));
    if !actual.eq_ignore_ascii_case(&artifact.sha256) {
        return Err(format!(
            "{} SHA-256 mismatch; repository metadata or download is inconsistent",
            selection.package_id
        ));
    }

    let imported = rintawa::engine::artifact_store::import_rtw(&response.body)
        .map_err(|error| format!("RTW validation/import failed: {error:?}"))?;
    if imported.digest != actual {
        return Err(format!(
            "Core CAS digest mismatch for {}",
            selection.package_id
        ));
    }
    if imported.content != selection.version.content {
        return Err(format!(
            "{} content mismatch: registry '{}', artifact '{}'",
            selection.package_id, selection.version.content, imported.content
        ));
    }
    Ok(imported.digest)
}

fn snapshot_activations() -> Result<BTreeMap<String, InstalledPackage>, String> {
    let activations = rintawa::engine::composition::list_activations()
        .map_err(|error| format!("could not snapshot composition: {error:?}"))?;
    Ok(activations
        .into_iter()
        .map(|activation| {
            let version_text = activation.version.clone();
            let version = version_text
                .as_deref()
                .and_then(|value| Version::parse(value).ok());
            (
                activation.subject.clone(),
                InstalledPackage {
                    subject: activation.subject,
                    name: activation.name,
                    version_text,
                    version,
                    digest: activation.digest,
                    instance_id: activation.instance_id,
                    scope_id: activation.scope_id,
                    enabled: activation.enabled,
                },
            )
        })
        .collect())
}

type PolicyKey = (String, String, String);
type PolicySnapshot = BTreeMap<PolicyKey, BTreeSet<String>>;

fn snapshot_runtime_policy() -> Result<PolicySnapshot, String> {
    let entries = rintawa::engine::runtime_policy::list_components()
        .map_err(|error| format!("could not snapshot runtime policy: {error:?}"))?;
    Ok(entries
        .into_iter()
        .map(|entry| {
            (
                (entry.scope_id, entry.instance_id, entry.component_id),
                entry.granted.into_iter().collect(),
            )
        })
        .collect())
}

fn managed_install_record_for(
    item: &PreparedInstallItem,
    previous: Option<&ManagedInstallRecord>,
) -> ManagedInstallRecord {
    let (dependencies, dependencies_known) = match item.dependencies.as_ref() {
        Some(dependencies) => (dependencies.clone(), true),
        None => previous
            .map(|record| (record.dependencies.clone(), record.dependencies_known))
            .unwrap_or_default(),
    };
    ManagedInstallRecord {
        version: item.version.clone(),
        dependencies,
        dependencies_known,
    }
}

fn confirm_pending_install() -> Result<(), String> {
    let pending = STATE
        .with(|state| state.borrow().pending_install.clone())
        .ok_or_else(|| "there is no pending installation".to_string())?;
    let before_activations = snapshot_activations()?;
    let before_policy = snapshot_runtime_policy()?;
    let before_managed = STATE.with(|state| state.borrow().managed_installs.clone());
    let mut mutated_subjects = Vec::new();
    let mut selected_principals = BTreeMap::new();

    for item in &pending.items {
        let activation = match rintawa::engine::composition::select_artifact(&item.digest, None) {
            Ok(activation) => activation,
            Err(error) => {
                return install_failure_with_rollback(
                    format!(
                        "could not select {} {}: {error:?}",
                        item.subject, item.version
                    ),
                    &before_activations,
                    &before_policy,
                    &mutated_subjects,
                );
            }
        };
        mutated_subjects.push(activation.subject.clone());

        let actual_version = activation.version.as_deref().unwrap_or("<missing>");
        if activation.subject != item.subject || actual_version != item.version {
            return install_failure_with_rollback(
                format!(
                    "selected artifact identity changed after inspection: expected {} {}, got {} {}",
                    item.subject, item.version, activation.subject, actual_version
                ),
                &before_activations,
                &before_policy,
                &mutated_subjects,
            );
        }
        selected_principals.insert(
            item.subject.clone(),
            (activation.scope_id, activation.instance_id),
        );
    }

    for item in &pending.items {
        let (scope_id, instance_id) = selected_principals
            .get(&item.subject)
            .ok_or_else(|| format!("missing selected principal for '{}'", item.subject))?;
        for permission in &item.permissions {
            let result = if permission.selected {
                rintawa::engine::runtime_policy::grant(
                    scope_id,
                    instance_id,
                    &permission.component_id,
                    &permission.permission,
                )
            } else {
                rintawa::engine::runtime_policy::revoke(
                    scope_id,
                    instance_id,
                    &permission.component_id,
                    &permission.permission,
                )
            };
            if let Err(error) = result {
                return install_failure_with_rollback(
                    format!(
                        "could not {} permission '{}' for {}:{}: {error:?}",
                        if permission.selected {
                            "grant"
                        } else {
                            "revoke"
                        },
                        permission.permission,
                        item.subject,
                        permission.component_id
                    ),
                    &before_activations,
                    &before_policy,
                    &mutated_subjects,
                );
            }
        }
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        for item in &pending.items {
            let record =
                managed_install_record_for(item, state.managed_installs.get(&item.subject));
            state.managed_installs.insert(item.subject.clone(), record);
        }
    });
    if let Err(error) = persist_managed_installs() {
        STATE.with(|state| state.borrow_mut().managed_installs = before_managed);
        return install_failure_with_rollback(
            error,
            &before_activations,
            &before_policy,
            &mutated_subjects,
        );
    }

    let installed = pending
        .items
        .iter()
        .map(|item| format!("{} {}", item.name, item.version))
        .collect::<Vec<_>>()
        .join(", ");
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.pending_install = None;
        state.direct_url.clear();
    });
    refresh_installed();
    set_status(format!(
        "Installed {installed} from {}. Restart may be required for runtime changes.",
        pending.origin
    ));
    Ok(())
}

fn install_failure_with_rollback<T>(
    primary: String,
    before_activations: &BTreeMap<String, InstalledPackage>,
    before_policy: &PolicySnapshot,
    mutated_subjects: &[String],
) -> Result<T, String> {
    let failures = rollback_install(before_activations, before_policy, mutated_subjects);
    if failures.is_empty() {
        Err(primary)
    } else {
        Err(format!(
            "{primary}; rollback also failed: {}",
            failures.join("; ")
        ))
    }
}

fn rollback_install(
    before_activations: &BTreeMap<String, InstalledPackage>,
    before_policy: &PolicySnapshot,
    mutated_subjects: &[String],
) -> Vec<String> {
    let mut failures = Vec::new();
    let mutated = mutated_subjects.iter().cloned().collect::<BTreeSet<_>>();

    for subject in mutated.iter().rev() {
        let result = if let Some(previous) = before_activations.get(subject) {
            rintawa::engine::composition::select_artifact(&previous.digest, Some(previous.enabled))
                .map(|_| ())
        } else {
            rintawa::engine::composition::remove_activation(subject)
        };
        if let Err(error) = result {
            failures.push(format!(
                "composition rollback for '{subject}' failed: {error:?}"
            ));
        }
    }

    let affected_principals = match snapshot_activations() {
        Ok(activations) => activations
            .into_iter()
            .filter(|(subject, _)| mutated.contains(subject))
            .map(|(_, activation)| (activation.scope_id, activation.instance_id))
            .collect::<BTreeSet<_>>(),
        Err(error) => {
            failures.push(format!(
                "could not read composition principals during rollback: {error}"
            ));
            return failures;
        }
    };

    let current = match rintawa::engine::runtime_policy::list_components() {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(format!("could not read policy during rollback: {error:?}"));
            return failures;
        }
    };

    let current_by_key = current
        .into_iter()
        .filter(|entry| {
            affected_principals.contains(&(entry.scope_id.clone(), entry.instance_id.clone()))
        })
        .map(|entry| {
            (
                (entry.scope_id, entry.instance_id, entry.component_id),
                entry.granted.into_iter().collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for (key, current_grants) in &current_by_key {
        let desired = before_policy.get(key).cloned().unwrap_or_default();
        for permission in current_grants.difference(&desired) {
            if let Err(error) =
                rintawa::engine::runtime_policy::revoke(&key.0, &key.1, &key.2, permission)
            {
                failures.push(format!(
                    "could not revoke '{}' while rolling back {}:{}: {error:?}",
                    permission, key.1, key.2
                ));
            }
        }
        for permission in desired.difference(current_grants) {
            if let Err(error) =
                rintawa::engine::runtime_policy::grant(&key.0, &key.1, &key.2, permission)
            {
                failures.push(format!(
                    "could not restore '{}' while rolling back {}:{}: {error:?}",
                    permission, key.1, key.2
                ));
            }
        }
    }

    for (key, desired) in before_policy {
        if !affected_principals.contains(&(key.0.clone(), key.1.clone()))
            || desired.is_empty()
            || current_by_key.contains_key(key)
        {
            continue;
        }
        failures.push(format!(
            "component {}:{} disappeared while restoring runtime policy",
            key.1, key.2
        ));
    }

    failures
}

fn set_pending_permission(
    item_index: usize,
    permission_index: usize,
    selected: bool,
) -> Result<(), String> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let pending = state
            .pending_install
            .as_mut()
            .ok_or_else(|| "there is no pending installation".to_string())?;
        let item = pending
            .items
            .get_mut(item_index)
            .ok_or_else(|| "pending package no longer exists".to_string())?;
        let permission = item
            .permissions
            .get_mut(permission_index)
            .ok_or_else(|| "pending permission no longer exists".to_string())?;
        permission.selected = selected;
        Ok(())
    })
}

fn cancel_pending_install() {
    STATE.with(|state| {
        state.borrow_mut().pending_install = None;
    });
    set_status(
        "Installation cancelled. Validated bytes may remain in immutable CAS cache.".to_string(),
    );
}

fn install_direct_url() -> Result<(), String> {
    let url = STATE.with(|state| state.borrow().direct_url.trim().to_string());
    validate_rtw_url(&url)?;
    let response = rintawa::engine::http_fetch::get(&url, MAX_ARTIFACT_BYTES as u32)
        .map_err(|error| format!("direct RTW download failed: {error:?}"))?;
    if response.status != 200 {
        return Err(format!("direct RTW URL returned HTTP {}", response.status));
    }
    if response.body.is_empty() {
        return Err("direct RTW URL returned an empty body".to_string());
    }

    let imported = rintawa::engine::artifact_store::import_rtw(&response.body)
        .map_err(|error| format!("RTW validation/import failed: {error:?}"))?;
    if imported.content != "rintawa.extension@1" {
        return Err(format!(
            "direct install supports extensions, not '{}'",
            imported.content
        ));
    }
    let policy = rintawa::engine::runtime_policy::inspect_artifact(&imported.digest)
        .map_err(|error| format!("could not inspect imported RTW: {error:?}"))?;
    let item =
        prepared_item_from_policy(imported.digest, policy, format!("direct URL {url}"), None);
    STATE.with(|state| {
        state.borrow_mut().pending_install = Some(PendingInstall {
            origin: format!("direct URL {url}"),
            items: vec![item],
        });
    });
    set_status("Review requested permissions before installing.".to_string());
    Ok(())
}

fn truncate_display_text(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_string();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = value[..end].to_string();
    truncated.push('…');
    truncated
}

fn installed_package_matches_search(
    state: &ManagerState,
    package: &InstalledPackage,
    query: &str,
) -> bool {
    if query.is_empty()
        || package.name.to_lowercase().contains(query)
        || package.subject.to_lowercase().contains(query)
        || package
            .version_text
            .as_deref()
            .is_some_and(|version| version.to_lowercase().contains(query))
    {
        return true;
    }

    state
        .catalog
        .packages
        .get(&package.subject)
        .map(|catalog| repository_label(&catalog.repository_url))
        .unwrap_or_else(|| "Local / baseline".to_string())
        .to_lowercase()
        .contains(query)
}

fn installed_provider(state: &ManagerState, package: &InstalledPackage) -> String {
    state
        .catalog
        .packages
        .get(&package.subject)
        .map(|catalog| repository_label(&catalog.repository_url))
        .unwrap_or_else(|| "Local / baseline".to_string())
}

fn installed_status(state: &ManagerState, package: &InstalledPackage) -> InstalledStatus {
    let blockers = dependency_blockers_locked(state, &package.subject);
    if !blockers.required_by.is_empty() {
        return InstalledStatus::RequiredDependency;
    }
    if !blockers.unknown.is_empty() {
        return InstalledStatus::DependencyMetadataUnknown;
    }

    let update_available = package.version.as_ref().is_some_and(|installed| {
        state
            .catalog
            .packages
            .get(&package.subject)
            .is_some_and(|available| installed < &available.latest)
    });
    if update_available {
        InstalledStatus::UpdateAvailable
    } else if package.enabled {
        InstalledStatus::Enabled
    } else {
        InstalledStatus::Disabled
    }
}

fn visible_installed_packages<'a>(
    state: &'a ManagerState,
    query: &str,
) -> Vec<&'a InstalledPackage> {
    let mut packages = state
        .installed
        .values()
        .filter(|package| installed_package_matches_search(state, package, query))
        .collect::<Vec<_>>();

    packages.sort_by(|left, right| {
        let ordering = match state.installed_sort {
            InstalledSortColumn::Enabled => left.enabled.cmp(&right.enabled),
            InstalledSortColumn::Name => left
                .name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase()),
            InstalledSortColumn::Version => left.version.cmp(&right.version),
            InstalledSortColumn::Provider => installed_provider(state, left)
                .to_ascii_lowercase()
                .cmp(&installed_provider(state, right).to_ascii_lowercase()),
            InstalledSortColumn::Status => {
                installed_status(state, left).cmp(&installed_status(state, right))
            }
        }
        .then_with(|| left.subject.cmp(&right.subject));

        match state.installed_sort_direction {
            SortDirection::Ascending => ordering,
            SortDirection::Descending => ordering.reverse(),
        }
    });
    packages
}

fn filtered_catalog_packages(state: &ManagerState) -> Vec<&CatalogPackage> {
    let query = state.search.trim().to_ascii_lowercase();
    let mut packages = state
        .catalog
        .packages
        .values()
        .filter(|package| {
            query.is_empty()
                || package.id.to_ascii_lowercase().contains(&query)
                || package.name.to_ascii_lowercase().contains(&query)
                || package.description.to_ascii_lowercase().contains(&query)
                || package
                    .authors
                    .iter()
                    .any(|author| author.to_ascii_lowercase().contains(&query))
        })
        .filter(|package| {
            state
                .repository_filter
                .as_ref()
                .is_none_or(|repository| &package.repository_url == repository)
        })
        .filter(|package| {
            let installed = state.installed.get(&package.id);
            match state.catalog_filter {
                CatalogFilter::All => true,
                CatalogFilter::Available => installed.is_none(),
                CatalogFilter::Installed => installed.is_some(),
                CatalogFilter::Updates => installed
                    .and_then(|item| item.version.as_ref())
                    .is_some_and(|version| version < &package.latest),
            }
        })
        .collect::<Vec<_>>();

    packages.sort_by(|left, right| match state.catalog_sort {
        CatalogSort::Name => left
            .name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id)),
        CatalogSort::Version => right
            .latest
            .cmp(&left.latest)
            .then_with(|| left.name.cmp(&right.name)),
        CatalogSort::Repository => left
            .repository_url
            .cmp(&right.repository_url)
            .then_with(|| left.name.cmp(&right.name)),
    });
    packages
}

fn fetch_verified_asset(asset: &PublishedAsset) -> Result<Vec<u8>, String> {
    let max_bytes = u32::try_from(asset.size)
        .map_err(|_| format!("asset '{}' size does not fit host fetch limit", asset.file))?;
    let response = rintawa::engine::http_fetch::get(&asset.url, max_bytes)
        .map_err(|error| format!("asset fetch failed for '{}': {error:?}", asset.file))?;
    if response.status != 200 {
        return Err(format!(
            "asset '{}' returned HTTP {}",
            asset.file, response.status
        ));
    }
    if response.body.len() as u64 != asset.size {
        return Err(format!(
            "asset '{}' size mismatch: registry {}, downloaded {}",
            asset.file,
            asset.size,
            response.body.len()
        ));
    }
    let actual = format!("sha256:{:x}", Sha256::digest(&response.body));
    if !actual.eq_ignore_ascii_case(&asset.sha256) {
        return Err(format!("asset '{}' SHA-256 mismatch", asset.file));
    }
    Ok(response.body)
}

fn fetch_cached_image(asset: &PublishedAsset) -> Result<CachedImage, String> {
    let bytes = fetch_verified_asset(asset)?;
    let valid = match asset.media_type.as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    };
    if !valid {
        return Err(format!(
            "asset '{}' bytes do not match media type {}",
            asset.file, asset.media_type
        ));
    }
    Ok(CachedImage {
        media_type: asset.media_type.clone(),
        data_base64: BASE64_STANDARD.encode(bytes),
    })
}

fn fetch_cached_readme(asset: &PublishedAsset) -> Result<String, String> {
    let bytes = fetch_verified_asset(asset)?;
    let mut source = String::from_utf8(bytes)
        .map_err(|_| format!("asset '{}' README is not UTF-8", asset.file))?;
    if source.len() > MAX_EMBEDDED_README_BYTES {
        let mut end = MAX_EMBEDDED_README_BYTES;
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        source.truncate(end);
        source.push_str(
            "

_… README truncated in this view._",
        );
    }
    Ok(source)
}

fn prepare_presentation_assets() {
    let (logos, readme) = STATE.with(|state| {
        let state = state.borrow();
        let mut candidates = match state.view {
            View::Browse => filtered_catalog_packages(&state)
                .into_iter()
                .take(MAX_SEARCH_RESULTS)
                .filter_map(|package| package.logo.clone())
                .collect::<Vec<_>>(),
            View::Installed => state
                .installed
                .keys()
                .filter_map(|id| state.catalog.packages.get(id))
                .filter_map(|package| package.logo.clone())
                .collect::<Vec<_>>(),
            View::Repositories | View::DirectUrl => Vec::new(),
        };
        candidates.extend(
            state
                .loaded_repositories
                .values()
                .filter_map(|index| index.repository.as_ref())
                .filter_map(|repository| repository.icon.clone()),
        );

        let mut seen = BTreeSet::new();
        let logos = candidates
            .into_iter()
            .filter(|asset| seen.insert(asset.sha256.clone()))
            .filter(|asset| !state.logo_cache.contains_key(&asset.sha256))
            .collect::<Vec<_>>();

        let readme = if state.view == View::Browse {
            state
                .selected_package
                .as_ref()
                .and_then(|id| state.catalog.packages.get(id))
                .and_then(|package| package.readme.clone())
                .filter(|asset| !state.readme_cache.contains_key(&asset.sha256))
        } else {
            None
        };
        (logos, readme)
    });

    for asset in logos {
        let result = fetch_cached_image(&asset);
        STATE.with(|state| {
            state
                .borrow_mut()
                .logo_cache
                .insert(asset.sha256.clone(), result);
        });
    }

    if let Some(asset) = readme {
        let result = fetch_cached_readme(&asset);
        STATE.with(|state| {
            state
                .borrow_mut()
                .readme_cache
                .insert(asset.sha256.clone(), result);
        });
    }
}

fn render_surface() {
    prepare_presentation_assets();
    let rendered = STATE.with(|state| build_surface(&state.borrow()));
    let node_ids = rendered
        .nodes
        .iter()
        .filter_map(|node| node.get("id").and_then(Value::as_str).map(str::to_string))
        .collect::<BTreeSet<_>>();

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let next_revision = state.revision.saturating_add(1).max(1);
        let result = if state.mounted {
            let mut patches = rendered
                .nodes
                .iter()
                .map(|node| json!({"type": "upsert-node", "node": node}))
                .collect::<Vec<_>>();
            for stale in state.previous_node_ids.difference(&node_ids) {
                patches.push(json!({"type": "remove-node", "node_id": stale}));
            }
            let batch = json!({
                "surface_id": SURFACE_ID,
                "base_revision": state.revision,
                "next_revision": next_revision,
                "patches": patches,
            });
            serde_json::to_vec(&batch)
                .map_err(|error| format!("could not encode UI patch: {error}"))
                .and_then(|bytes| {
                    rintawa::engine::portable_ui::patch_surface(&bytes)
                        .map_err(|error| format!("could not patch Package Manager UI: {error:?}"))
                })
        } else {
            mount_snapshot(next_revision, &rendered.nodes)
        };

        if let Err(error) = result {
            let _ = rintawa::engine::portable_ui::unmount_surface(SURFACE_ID);
            match mount_snapshot(next_revision, &rendered.nodes) {
                Ok(()) => {
                    state.mounted = true;
                    state.status = format!("UI patch recovered after: {error}");
                }
                Err(mount_error) => {
                    let message = format!("{error}; remount failed: {mount_error}");
                    rintawa::engine::host::log(
                        rintawa::engine::host::LogLevel::Error,
                        &format!("Package Manager UI mount failed: {message}"),
                    );
                    state.status = message;
                    return;
                }
            }
        } else {
            state.mounted = true;
        }

        state.revision = next_revision;
        state.previous_node_ids = node_ids;
        state.action_targets = rendered.action_targets;
    });
}

fn mount_snapshot(revision: u64, nodes: &[Value]) -> Result<(), String> {
    let snapshot = json!({
        "surface_id": SURFACE_ID,
        "revision": revision,
        "root": "root",
        "nodes": nodes,
    });
    let bytes = serde_json::to_vec(&snapshot)
        .map_err(|error| format!("could not encode Package Manager UI: {error}"))?;
    rintawa::engine::portable_ui::mount_surface(&bytes)
        .map_err(|error| format!("could not mount Package Manager UI: {error:?}"))
}

struct RenderedSurface {
    nodes: Vec<Value>,
    action_targets: BTreeMap<String, ActionTarget>,
}

struct GridColumn {
    key: Option<String>,
    label: String,
    weight: u32,
    sort_action: Option<String>,
    sort_direction: Option<String>,
}

impl GridColumn {
    fn plain(label: impl Into<String>, weight: u32) -> Self {
        Self {
            key: None,
            label: label.into(),
            weight,
            sort_action: None,
            sort_direction: None,
        }
    }

    fn sortable(
        key: impl Into<String>,
        label: impl Into<String>,
        weight: u32,
        direction: Option<SortDirection>,
    ) -> Self {
        Self {
            key: Some(key.into()),
            label: label.into(),
            weight,
            sort_action: Some("installed.sort".to_string()),
            sort_direction: direction.map(|direction| direction.value().to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ButtonAppearance {
    Default,
    Primary,
    Subtle,
    Danger,
}

impl ButtonAppearance {
    fn value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Primary => "primary",
            Self::Subtle => "subtle",
            Self::Danger => "danger",
        }
    }
}

struct UiBuilder {
    next_id: usize,
    nodes: Vec<Value>,
    action_targets: BTreeMap<String, ActionTarget>,
    embedded_image_base64_bytes: usize,
}

impl UiBuilder {
    fn new() -> Self {
        Self {
            next_id: 0,
            nodes: Vec::new(),
            action_targets: BTreeMap::new(),
            embedded_image_base64_bytes: 0,
        }
    }

    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}.{}", self.next_id)
    }

    fn presentation_semantic(&mut self, id: String, semantic: &str, traits: &[&str]) -> String {
        let Some(node) = self
            .nodes
            .iter_mut()
            .find(|node| node.get("id").and_then(Value::as_str) == Some(id.as_str()))
        else {
            rintawa::engine::host::log(
                rintawa::engine::host::LogLevel::Error,
                &format!("Portable UI builder lost node '{id}' before semantic annotation"),
            );
            return id;
        };
        let Some(object) = node.as_object_mut() else {
            rintawa::engine::host::log(
                rintawa::engine::host::LogLevel::Error,
                &format!("Portable UI node '{id}' is not an object"),
            );
            return id;
        };

        object.insert(
            "semantic".to_string(),
            json!({"id": semantic, "version": 1}),
        );
        object.insert("traits".to_string(), json!(traits));
        id
    }

    fn text(&mut self, text: impl Into<String>) -> String {
        let id = self.id("text");
        self.nodes.push(json!({
            "id": id,
            "kind": {"type": "text", "data": {"text": text.into()}}
        }));
        id
    }

    fn markdown(&mut self, source: impl Into<String>) -> String {
        let id = self.id("markdown");
        self.nodes.push(json!({
            "id": id,
            "kind": {"type": "markdown", "data": {"source": source.into()}}
        }));
        id
    }

    fn button(
        &mut self,
        label: impl Into<String>,
        action: &str,
        enabled: bool,
        target: Option<ActionTarget>,
    ) -> String {
        self.button_with_appearance(label, action, enabled, target, ButtonAppearance::Default)
    }

    fn button_with_appearance(
        &mut self,
        label: impl Into<String>,
        action: &str,
        enabled: bool,
        target: Option<ActionTarget>,
        appearance: ButtonAppearance,
    ) -> String {
        let id = self.id("button");
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "button",
                "data": {
                    "label": label.into(),
                    "action": action,
                    "is_enabled": enabled,
                    "appearance": appearance.value(),
                }
            }
        }));
        if let Some(target) = target {
            self.action_targets.insert(id.clone(), target);
        }
        id
    }

    fn icon(&mut self, slot: &str, label: Option<&str>, size: Option<u32>) -> String {
        let id = self.id("icon");
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "icon",
                "data": {
                    "slot": slot,
                    "label": label,
                    "size": size
                }
            }
        }));
        id
    }

    fn image(
        &mut self,
        image: &CachedImage,
        alt: impl Into<String>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Option<String> {
        let next_size = self
            .embedded_image_base64_bytes
            .checked_add(image.data_base64.len())?;
        if next_size > MAX_EMBEDDED_IMAGE_BASE64_BYTES {
            return None;
        }
        self.embedded_image_base64_bytes = next_size;

        let id = self.id("image");
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "image",
                "data": {
                    "media_type": image.media_type,
                    "data_base64": image.data_base64,
                    "alt": alt.into(),
                    "width": width,
                    "height": height
                }
            }
        }));
        Some(id)
    }

    fn checkbox(
        &mut self,
        label: impl Into<String>,
        checked: bool,
        action: &str,
        enabled: bool,
        target: Option<ActionTarget>,
    ) -> String {
        let id = self.id("checkbox");
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "checkbox",
                "data": {
                    "label": label.into(),
                    "checked": checked,
                    "change_action": action,
                    "is_enabled": enabled
                }
            }
        }));
        if let Some(target) = target {
            self.action_targets.insert(id.clone(), target);
        }
        id
    }

    fn select(
        &mut self,
        value: &str,
        options: impl IntoIterator<Item = (String, String)>,
        action: &str,
        enabled: bool,
        target: Option<ActionTarget>,
    ) -> String {
        let id = self.id("select");
        let options = options
            .into_iter()
            .map(|(value, label)| json!({"value": value, "label": label}))
            .collect::<Vec<_>>();
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "select",
                "data": {
                    "value": value,
                    "options": options,
                    "change_action": action,
                    "is_enabled": enabled
                }
            }
        }));
        if let Some(target) = target {
            self.action_targets.insert(id.clone(), target);
        }
        id
    }

    fn input(
        &mut self,
        value: &str,
        placeholder: &str,
        change_action: &str,
        submit_action: Option<&str>,
    ) -> String {
        let id = self.id("input");
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "text-input",
                "data": {
                    "value": value,
                    "placeholder": placeholder,
                    "change_action": change_action,
                    "submit_action": submit_action,
                    "is_enabled": true
                }
            }
        }));
        id
    }

    fn named_split(&mut self, id: &str, axis: &str, panes: Vec<(String, u32)>) -> String {
        self.split_with_id(id.to_string(), axis, panes)
    }

    fn split_with_id(&mut self, id: String, axis: &str, panes: Vec<(String, u32)>) -> String {
        let (children, weights): (Vec<_>, Vec<_>) = panes.into_iter().unzip();
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "split",
                "data": {
                    "children": children,
                    "weights": weights,
                    "axis": axis
                }
            }
        }));
        id
    }

    fn row(&mut self, children: Vec<String>) -> String {
        self.container("row", children)
    }

    fn column(&mut self, children: Vec<String>) -> String {
        self.container("column", children)
    }

    fn list(&mut self, children: Vec<String>) -> String {
        self.container("list", children)
    }

    fn named_data_grid(
        &mut self,
        id: &str,
        columns: Vec<GridColumn>,
        rows: Vec<Vec<String>>,
        selected_rows: Vec<u32>,
        row_keys: Vec<String>,
        row_action: Option<&str>,
    ) -> String {
        self.data_grid_with_id(
            id.to_string(),
            columns,
            rows,
            selected_rows,
            row_keys,
            row_action,
        )
    }

    fn data_grid_with_id(
        &mut self,
        id: String,
        columns: Vec<GridColumn>,
        rows: Vec<Vec<String>>,
        selected_rows: Vec<u32>,
        row_keys: Vec<String>,
        row_action: Option<&str>,
    ) -> String {
        let columns = columns
            .into_iter()
            .map(|column| {
                json!({
                    "key": column.key,
                    "label": column.label,
                    "weight": column.weight,
                    "sort_action": column.sort_action,
                    "sort_direction": column.sort_direction,
                })
            })
            .collect::<Vec<_>>();
        let cells = rows.into_iter().flatten().collect::<Vec<_>>();
        self.nodes.push(json!({
            "id": id,
            "kind": {
                "type": "data-grid",
                "data": {
                    "columns": columns,
                    "cells": cells,
                    "selected_rows": selected_rows,
                    "row_keys": row_keys,
                    "row_action": row_action,
                }
            }
        }));
        id
    }

    fn container(&mut self, kind: &str, children: Vec<String>) -> String {
        let id = self.id(kind);
        self.nodes.push(json!({
            "id": id,
            "kind": {"type": kind, "data": {"children": children}}
        }));
        id
    }

    fn finish(mut self, root_children: Vec<String>) -> RenderedSurface {
        self.nodes.push(json!({
            "id": "root",
            "kind": {"type": "column", "data": {"children": root_children}}
        }));
        RenderedSurface {
            nodes: self.nodes,
            action_targets: self.action_targets,
        }
    }
}

fn build_surface(state: &ManagerState) -> RenderedSurface {
    let mut ui = UiBuilder::new();
    let status = ui.text(state.status.clone());

    if let Some(pending) = state.pending_install.as_ref() {
        let title = ui.markdown(format!("## {}", message(Message::InstallationReview)));
        let pending = build_pending_install(&mut ui, pending);
        let content = ui.column(vec![title, pending, status]);
        return ui.finish(vec![content]);
    }

    let workspace = match state.view {
        View::Browse => build_browse(&mut ui, state),
        View::Installed => build_installed(&mut ui, state),
        View::Repositories => build_repositories(&mut ui, state),
        View::DirectUrl => build_direct_url(&mut ui, state),
    };
    let content = ui.column(vec![workspace, status]);
    ui.finish(vec![content])
}

fn build_pending_install(ui: &mut UiBuilder, pending: &PendingInstall) -> String {
    let intro = ui.markdown(format!(
        "**Review installation**\n\nOrigin: {}\n\nNo baseline activation changes have been made yet.",
        pending.origin
    ));
    let mut packages = Vec::new();

    for (item_index, item) in pending.items.iter().enumerate() {
        let heading = ui.text(format!("{}  {}", item.name, item.version));
        let identity = ui.text(format!("{} · {}", item.subject, item.digest));
        let origin = ui.text(format!("Origin: {}", item.origin));

        let permissions = if item.permissions.is_empty() {
            let none = ui.text("No runtime permissions requested.");
            ui.column(vec![none])
        } else {
            let mut rows = Vec::new();
            for (permission_index, permission) in item.permissions.iter().enumerate() {
                rows.push(ui.checkbox(
                    format!("{} · {}", permission.component_id, permission.permission),
                    permission.selected,
                    "install.permission-toggle",
                    true,
                    Some(ActionTarget::Permission {
                        item: item_index,
                        permission: permission_index,
                    }),
                ));
            }
            ui.column(rows)
        };

        packages.push(ui.column(vec![heading, identity, origin, permissions]));
    }

    let package_list = ui.list(packages);
    let note = ui.markdown(
        "Selected permissions will be explicitly granted to the exact component principals. Unselected requested permissions remain denied.",
    );
    let confirm = ui.button_with_appearance(
        message(Message::ConfirmInstall),
        "install.confirm",
        true,
        None,
        ButtonAppearance::Primary,
    );
    let cancel = ui.button(message(Message::Cancel), "install.cancel", true, None);
    let actions = ui.row(vec![confirm, cancel]);
    ui.column(vec![intro, package_list, note, actions])
}

fn package_visual(
    ui: &mut UiBuilder,
    state: &ManagerState,
    package: &CatalogPackage,
    size: u32,
) -> String {
    if let Some(asset) = package.logo.as_ref()
        && let Some(Ok(image)) = state.logo_cache.get(&asset.sha256)
        && let Some(node) = ui.image(
            image,
            format!("{} logo", package.name),
            Some(size),
            Some(size),
        )
    {
        return node;
    }
    ui.icon("fallback.package", Some(&package.name), Some(size))
}

fn build_repository_browser(ui: &mut UiBuilder, state: &ManagerState) -> String {
    let title = ui.text(message(Message::Repositories));
    let all_icon = ui.icon("repository.generic", Some("All repositories"), Some(18));
    let all = ui.button_with_appearance(
        if state.repository_filter.is_none() {
            "All repositories ✓"
        } else {
            "All repositories"
        },
        "catalog.repository-all",
        true,
        None,
        ButtonAppearance::Subtle,
    );
    let all_row = ui.row(vec![all_icon, all]);

    let mut entries = vec![all_row];
    for (index, repository) in state.repositories.iter().enumerate() {
        let metadata = state
            .loaded_repositories
            .get(&repository.url)
            .and_then(|index| index.repository.as_ref());
        let icon = metadata
            .and_then(|metadata| metadata.icon.as_ref())
            .and_then(|asset| state.logo_cache.get(&asset.sha256))
            .and_then(|result| result.as_ref().ok())
            .and_then(|image| {
                ui.image(
                    image,
                    format!(
                        "{} repository icon",
                        metadata
                            .map(|value| value.name.as_str())
                            .unwrap_or("Repository")
                    ),
                    Some(20),
                    Some(20),
                )
            })
            .unwrap_or_else(|| ui.icon("repository.generic", Some("Repository"), Some(18)));
        let selected = state.repository_filter.as_deref() == Some(repository.url.as_str());
        let label = metadata
            .map(|metadata| metadata.name.clone())
            .unwrap_or_else(|| repository_label(&repository.url));
        let button = ui.button_with_appearance(
            format!(
                "{}{}{}",
                if selected { "✓ " } else { "" },
                label,
                if repository.enabled {
                    ""
                } else {
                    " (disabled)"
                }
            ),
            "catalog.repository-target",
            repository.enabled,
            Some(ActionTarget::Repository(index)),
            ButtonAppearance::Subtle,
        );
        let status = state.repository_status.get(&repository.url);
        let detail = match status {
            Some(status) if status.loaded => format!("{} packages", status.package_count),
            Some(status) => status
                .error
                .as_deref()
                .map(|error| format!("Error: {error}"))
                .unwrap_or_else(|| "Repository error".to_string()),
            None => "Not refreshed".to_string(),
        };
        let detail = ui.text(detail);
        let head = ui.row(vec![icon, button]);
        entries.push(ui.column(vec![head, detail]));
    }

    let list = ui.list(entries);
    let manage = ui.button(
        message(Message::ManageRepositories),
        "view.repositories",
        true,
        None,
    );
    let direct = ui.button(message(Message::AddFromUrl), "view.direct-url", true, None);
    ui.column(vec![title, list, manage, direct])
}

fn build_catalog_card(
    ui: &mut UiBuilder,
    state: &ManagerState,
    package: &CatalogPackage,
) -> String {
    let selected = state.selected_packages.contains(&package.id);
    let checkbox = ui.checkbox(
        "",
        selected,
        "package.selection",
        true,
        Some(ActionTarget::Package(package.id.clone())),
    );
    let visual = package_visual(ui, state, package, 32);
    let heading = ui.button_with_appearance(
        package.name.clone(),
        "package.details",
        true,
        Some(ActionTarget::Package(package.id.clone())),
        ButtonAppearance::Subtle,
    );
    let version = ui.text(format!("Latest {}", package.latest));
    let description = ui.text(truncate_display_text(
        &package.description,
        MAX_CARD_DESCRIPTION_BYTES,
    ));
    let authors = if package.authors.is_empty() {
        "Unknown author".to_string()
    } else {
        truncate_display_text(&package.authors.join(", "), MAX_CARD_AUTHORS_BYTES)
    };
    let metadata = ui.text(format!(
        "{} · {} · {}",
        authors, package.license, package.id
    ));

    let info = ui.column(vec![heading, version, description, metadata]);
    ui.row(vec![checkbox, visual, info])
}

fn build_package_version_controls(
    ui: &mut UiBuilder,
    state: &ManagerState,
    package: &CatalogPackage,
) -> Vec<String> {
    let selected_version = state
        .selected_version
        .as_ref()
        .filter(|selected| {
            package
                .versions
                .iter()
                .any(|release| &release.version == *selected)
        })
        .unwrap_or(&package.latest);
    let version_options = package
        .versions
        .iter()
        .map(|release| {
            (
                release.version.to_string(),
                format!("Version {}", release.version),
            )
        })
        .collect::<Vec<_>>();
    let version = ui.select(
        &selected_version.to_string(),
        version_options,
        "package.version",
        true,
        Some(ActionTarget::Package(package.id.clone())),
    );

    let installed_version = state
        .installed
        .get(&package.id)
        .and_then(|item| item.version.as_ref());
    let install_enabled = installed_version != Some(selected_version);
    let action_label = match installed_version {
        Some(installed) if installed < selected_version => {
            format!("Update to {selected_version}")
        }
        Some(installed) if installed > selected_version => {
            format!("Switch to {selected_version}")
        }
        Some(_) => format!("Installed {selected_version}"),
        None => format!("Install {selected_version}"),
    };
    let install = ui.button_with_appearance(
        action_label,
        "package.install-version",
        install_enabled,
        Some(ActionTarget::Package(package.id.clone())),
        ButtonAppearance::Primary,
    );
    let label = ui.text("Version selected:");
    vec![label, version, install]
}

fn build_package_details(
    ui: &mut UiBuilder,
    state: &ManagerState,
    package: &CatalogPackage,
) -> String {
    let visual = package_visual(ui, state, package, 48);
    let name = ui.text(package.name.clone());
    let identity = ui.text(package.id.clone());
    let heading_text = ui.column(vec![name, identity]);
    let heading = ui.row(vec![visual, heading_text]);
    let description = ui.text(package.description.clone());

    let authors = if package.authors.is_empty() {
        "Unknown author".to_string()
    } else {
        package.authors.join(", ")
    };
    let metadata = ui.text(format!(
        "{} · {} · {}",
        authors, package.license, package.repository_url
    ));

    let links = match (&package.homepage, &package.source_url) {
        (Some(homepage), Some(source)) => {
            Some(ui.markdown(format!("[Homepage]({homepage}) · [Source]({source})")))
        }
        (Some(homepage), None) => Some(ui.markdown(format!("[Homepage]({homepage})"))),
        (None, Some(source)) => Some(ui.markdown(format!("[Source]({source})"))),
        (None, None) => None,
    };

    let readme_title = ui.text("README");
    let readme = match package.readme.as_ref() {
        Some(asset) => match state.readme_cache.get(&asset.sha256) {
            Some(Ok(source)) => ui.markdown(source.clone()),
            Some(Err(error)) => ui.text(format!("README unavailable: {error}")),
            None => ui.text("Loading README…"),
        },
        None => ui.text("This package does not provide a README."),
    };

    let mut children = vec![heading, description, metadata];
    if let Some(links) = links {
        children.push(links);
    }
    children.push(readme_title);
    children.push(readme);
    ui.column(children)
}

fn build_browse(ui: &mut UiBuilder, state: &ManagerState) -> String {
    let title = ui.text(message(Message::DownloadExtensions));
    let close = ui.button_with_appearance(
        message(Message::Close),
        "view.close",
        true,
        None,
        ButtonAppearance::Subtle,
    );
    let refresh = ui.button(message(Message::Refresh), "catalog.refresh", true, None);
    let title_row = ui.row(vec![title, refresh, close]);

    let filter = ui.select(
        state.catalog_filter.value(),
        vec![
            ("all".to_string(), message(Message::All).to_string()),
            (
                "available".to_string(),
                message(Message::NotInstalled).to_string(),
            ),
            (
                "installed".to_string(),
                message(Message::Installed).to_string(),
            ),
            (
                "updates".to_string(),
                message(Message::UpdatesAvailable).to_string(),
            ),
        ],
        "catalog.filter",
        true,
        None,
    );
    let search = ui.input(
        &state.search,
        message(Message::SearchCatalog),
        "search.change",
        None,
    );
    let search_row = ui.row(vec![filter, search]);

    let visible = filtered_catalog_packages(state);
    let visible_count = visible.len();
    let cards = visible
        .into_iter()
        .take(MAX_SEARCH_RESULTS)
        .map(|package| build_catalog_card(ui, state, package))
        .collect::<Vec<_>>();
    let summary = if state.catalog.packages.is_empty() {
        ui.markdown("No catalog is loaded yet. Use **Refresh** to fetch enabled repositories.")
    } else {
        ui.text(format!(
            "{} matching extensions · {} total · showing up to {}",
            visible_count,
            state.catalog.packages.len(),
            MAX_SEARCH_RESULTS
        ))
    };
    let catalog_list = ui.list(cards);

    let sort = ui.select(
        state.catalog_sort.value(),
        vec![
            ("name".to_string(), message(Message::SortName).to_string()),
            (
                "version".to_string(),
                message(Message::SortVersion).to_string(),
            ),
            (
                "repository".to_string(),
                message(Message::SortRepository).to_string(),
            ),
        ],
        "catalog.sort",
        true,
        None,
    );
    let mut footer_items = vec![sort];
    if let Some(package) = state
        .selected_package
        .as_ref()
        .and_then(|id| state.catalog.packages.get(id))
    {
        footer_items.extend(build_package_version_controls(ui, state, package));
    }
    if state.selected_packages.is_empty() {
        footer_items.push(ui.text(message(Message::NoExtensionsSelected)));
    } else {
        footer_items.push(ui.text(format!("{} selected", state.selected_packages.len())));
        footer_items.push(ui.button_with_appearance(
            message(Message::InstallSelected),
            "selection.install",
            true,
            None,
            ButtonAppearance::Primary,
        ));
        footer_items.push(ui.button(
            message(Message::ClearSelection),
            "selection.clear",
            true,
            None,
        ));
    }
    let footer = ui.row(footer_items);
    let catalog = ui.column(vec![title_row, search_row, summary, catalog_list, footer]);
    let catalog = ui.presentation_semantic(
        catalog,
        "management.extensions.catalog.results",
        &["collection", "primary-content"],
    );

    let repositories = build_repository_browser(ui, state);
    let repositories = ui.presentation_semantic(
        repositories,
        "management.extensions.catalog.repositories",
        &["navigation", "source-list"],
    );
    let details_content = match state
        .selected_package
        .as_ref()
        .and_then(|id| state.catalog.packages.get(id))
    {
        Some(package) => build_package_details(ui, state, package),
        None => {
            let icon = ui.icon("fallback.package", Some("Extension details"), Some(40));
            let hint = ui.text("Select an extension to view details and README.");
            ui.column(vec![icon, hint])
        }
    };
    let details_title = ui.text(message(Message::Details));
    let details = ui.column(vec![details_title, details_content]);
    let details = ui.presentation_semantic(
        details,
        "management.extensions.catalog.details",
        &["inspector", "selection-context"],
    );

    let layout = ui.named_split(
        "browse.split",
        "horizontal",
        vec![(repositories, 28), (catalog, 100), (details, 44)],
    );
    ui.presentation_semantic(
        layout,
        "management.extensions.catalog.layout",
        &["resizable", "workspace-layout"],
    )
}

fn repository_label(url: &str) -> String {
    if url == BUILTIN_REPOSITORY {
        "rtwKit".to_string()
    } else {
        url.to_string()
    }
}

fn build_installed_permissions(
    ui: &mut UiBuilder,
    state: &ManagerState,
    package: &InstalledPackage,
) -> String {
    let title = ui.markdown(format!("### {}", message(Message::RuntimePermissions)));
    if let Some(error) = state.installed_policy_error.as_deref() {
        let error = ui.text(error.to_string());
        return ui.column(vec![title, error]);
    }

    let key = (package.scope_id.clone(), package.instance_id.clone());
    let Some(components) = state.installed_policy.get(&key) else {
        let empty = ui.text("No runtime permission policy is registered for this extension.");
        return ui.column(vec![title, empty]);
    };

    let mut rows = Vec::new();
    for component in components {
        if component.requested.is_empty() {
            rows.push(ui.text(format!(
                "{} · no runtime permissions requested",
                component.component_id
            )));
            continue;
        }

        let granted = component.granted.iter().cloned().collect::<BTreeSet<_>>();
        let denied = component
            .requested
            .iter()
            .filter(|permission| !granted.contains(*permission))
            .cloned()
            .collect::<Vec<_>>();
        let requested = ui.text(format!(
            "{} · requested: {}",
            component.component_id,
            component.requested.join(", ")
        ));
        let granted = ui.text(format!(
            "Granted: {}",
            if component.granted.is_empty() {
                "none".to_string()
            } else {
                component.granted.join(", ")
            }
        ));
        let denied = ui.text(format!(
            "Denied: {}",
            if denied.is_empty() {
                "none".to_string()
            } else {
                denied.join(", ")
            }
        ));
        rows.push(ui.column(vec![requested, granted, denied]));
    }

    let permissions = ui.list(rows);
    ui.column(vec![title, permissions])
}

fn build_installed_details(
    ui: &mut UiBuilder,
    state: &ManagerState,
    package: &InstalledPackage,
) -> String {
    let catalog_package = state.catalog.packages.get(&package.subject);
    let visual = match catalog_package {
        Some(catalog_package) => package_visual(ui, state, catalog_package, 32),
        None => ui.icon("fallback.package", Some(&package.name), Some(32)),
    };
    let name = ui.text(package.name.clone());
    let version = package
        .version_text
        .as_deref()
        .map(|value| format!("Version {value}"))
        .unwrap_or_else(|| "Version unknown".to_string());
    let provider = installed_provider(state, package);
    let metadata = ui.text(format!("{version} · {provider} · {}", package.subject));

    let mut summary = vec![name, metadata];
    if let Some(catalog_package) = catalog_package {
        summary.push(ui.text(catalog_package.description.clone()));
    }
    let summary = ui.column(summary);
    ui.row(vec![visual, summary])
}

fn build_installed_commands(
    ui: &mut UiBuilder,
    state: &ManagerState,
    selected: Option<&InstalledPackage>,
) -> String {
    let general = ui.text(message(Message::GeneralActions));
    let download = ui.button(
        message(Message::DownloadExtensions),
        "view.browse",
        true,
        None,
    );
    let refresh = ui.button(
        message(Message::CheckForUpdates),
        "catalog.refresh",
        true,
        None,
    );
    let add_file = ui.button(message(Message::AddFile), "package.add-file", false, None);
    let direct = ui.button(message(Message::AddFromUrl), "view.direct-url", true, None);
    let repositories = ui.button(
        message(Message::Repositories),
        "view.repositories",
        true,
        None,
    );
    let mut actions = vec![general, download, refresh, add_file, direct, repositories];

    if let Some(package) = selected {
        actions.push(ui.text(message(Message::SelectedExtension)));

        let blockers = dependency_blockers_locked(state, &package.subject);
        let destructive_safe = blockers.required_by.is_empty() && blockers.unknown.is_empty();
        let enable_issues = if package.enabled {
            Ok(Vec::new())
        } else {
            dependency_issues_for_enable_locked(state, &package.subject)
        };
        let toggle_enabled = if package.enabled {
            destructive_safe
        } else {
            enable_issues.as_ref().is_ok_and(Vec::is_empty)
        };

        actions.push(ui.button(
            if package.enabled {
                message(Message::Disable)
            } else {
                message(Message::Enable)
            },
            "package.toggle",
            toggle_enabled,
            Some(ActionTarget::Activation(package.subject.clone())),
        ));

        if let Some(catalog_package) = state.catalog.packages.get(&package.subject) {
            actions.push(ui.button(
                message(Message::ChangeVersion),
                "package.details",
                true,
                Some(ActionTarget::Package(package.subject.clone())),
            ));
            if package
                .version
                .as_ref()
                .is_some_and(|version| version < &catalog_package.latest)
            {
                actions.push(ui.button(
                    format!("Update to {}", catalog_package.latest),
                    "package.update",
                    true,
                    Some(ActionTarget::Package(package.subject.clone())),
                ));
            }
        }

        actions.push(ui.button_with_appearance(
            message(Message::Remove),
            "package.uninstall",
            destructive_safe,
            Some(ActionTarget::Activation(package.subject.clone())),
            ButtonAppearance::Danger,
        ));

        actions.push(ui.text(message(Message::Details)));
        actions.push(ui.text(format!("Artifact {}", package.digest)));
        actions.push(ui.text(format!(
            "Instance {} · scope {}",
            package.instance_id, package.scope_id
        )));

        let dependencies = match state.managed_installs.get(&package.subject) {
            Some(record) if record.dependencies_known && record.dependencies.is_empty() => {
                "Dependencies: none".to_string()
            }
            Some(record) if record.dependencies_known => {
                format!("Dependencies: {}", record.dependencies.join(", "))
            }
            _ => "Dependencies: exact metadata unavailable".to_string(),
        };
        actions.push(ui.text(dependencies));
        actions.push(build_installed_permissions(ui, state, package));

        if let Some(catalog_package) = state.catalog.packages.get(&package.subject) {
            let links = match (&catalog_package.homepage, &catalog_package.source_url) {
                (Some(homepage), Some(source)) => Some(ui.markdown(format!(
                    "[View Homepage]({homepage})\n\n[View Source]({source})"
                ))),
                (Some(homepage), None) => Some(ui.markdown(format!("[View Homepage]({homepage})"))),
                (None, Some(source)) => Some(ui.markdown(format!("[View Source]({source})"))),
                (None, None) => None,
            };
            if let Some(links) = links {
                actions.push(links);
            }
        }
    }

    ui.column(actions)
}

fn build_installed(ui: &mut UiBuilder, state: &ManagerState) -> String {
    let selected = state
        .selected_package
        .as_ref()
        .and_then(|subject| state.installed.get(subject));
    let title = match selected {
        Some(_) => ui.text(format!(
            "{} ({} installed, 1 selected)",
            message(Message::Extensions),
            state.installed.len()
        )),
        None => ui.text(format!(
            "{} ({} installed)",
            message(Message::Extensions),
            state.installed.len()
        )),
    };
    let search = ui.input(
        &state.installed_search,
        message(Message::SearchInstalled),
        "installed.search",
        None,
    );
    let search = ui.presentation_semantic(
        search,
        "management.extensions.installed.search",
        &["filter", "user-input"],
    );

    if state.installed.is_empty() {
        let icon = ui.icon(
            "fallback.package",
            Some(message(Message::Extensions)),
            Some(40),
        );
        let empty = ui.text("No baseline extensions are installed.");
        let hint = ui.text("Use Download Extensions to browse configured repositories.");
        let body = ui.column(vec![title, icon, empty, hint, search]);
        let body = ui.presentation_semantic(
            body,
            "management.extensions.installed.content",
            &["primary-content"],
        );
        let commands = build_installed_commands(ui, state, None);
        let commands = ui.presentation_semantic(
            commands,
            "management.extensions.installed.commands",
            &["command-set"],
        );
        let layout = ui.named_split(
            "installed.split",
            "horizontal",
            vec![(body, 100), (commands, 24)],
        );
        return ui.presentation_semantic(
            layout,
            "management.extensions.installed.layout",
            &["resizable", "workspace-layout"],
        );
    }

    let query = state.installed_search.trim().to_lowercase();
    let visible = visible_installed_packages(state, &query);
    let visible_count = visible.len();
    let mut rows = Vec::new();
    let mut selected_rows = Vec::new();
    let mut row_keys = Vec::new();

    for (row_index, package) in visible.into_iter().take(MAX_INSTALLED_RESULTS).enumerate() {
        if selected.is_some_and(|selected| selected.subject == package.subject)
            && let Ok(row_index) = u32::try_from(row_index)
        {
            selected_rows.push(row_index);
        }
        row_keys.push(package.subject.clone());

        let catalog_package = state.catalog.packages.get(&package.subject);
        let visual = match catalog_package {
            Some(catalog_package) => package_visual(ui, state, catalog_package, 32),
            None => ui.icon("fallback.package", Some(&package.name), Some(32)),
        };

        let blockers = dependency_blockers_locked(state, &package.subject);
        let destructive_safe = blockers.required_by.is_empty() && blockers.unknown.is_empty();
        let enable_issues = if package.enabled {
            Ok(Vec::new())
        } else {
            dependency_issues_for_enable_locked(state, &package.subject)
        };
        let toggle_enabled = if package.enabled {
            destructive_safe
        } else {
            enable_issues.as_ref().is_ok_and(Vec::is_empty)
        };
        let enabled = ui.checkbox(
            "",
            package.enabled,
            "package.enabled",
            toggle_enabled,
            Some(ActionTarget::Activation(package.subject.clone())),
        );
        let name = ui.text(package.name.clone());
        let version = ui.text(
            package
                .version_text
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
        );
        let provider = ui.text(installed_provider(state, package));
        let status = ui.text(message(installed_status(state, package).message()));
        rows.push(vec![enabled, visual, name, version, provider, status]);
    }

    let active_direction =
        |column| (state.installed_sort == column).then_some(state.installed_sort_direction);
    let grid = ui.named_data_grid(
        "installed.grid",
        vec![
            GridColumn::sortable(
                InstalledSortColumn::Enabled.key(),
                message(Message::ColumnEnable),
                8,
                active_direction(InstalledSortColumn::Enabled),
            ),
            GridColumn::plain(message(Message::ColumnImage), 9),
            GridColumn::sortable(
                InstalledSortColumn::Name.key(),
                message(Message::ColumnName),
                32,
                active_direction(InstalledSortColumn::Name),
            ),
            GridColumn::sortable(
                InstalledSortColumn::Version.key(),
                message(Message::ColumnVersion),
                15,
                active_direction(InstalledSortColumn::Version),
            ),
            GridColumn::sortable(
                InstalledSortColumn::Provider.key(),
                message(Message::ColumnProvider),
                20,
                active_direction(InstalledSortColumn::Provider),
            ),
            GridColumn::sortable(
                InstalledSortColumn::Status.key(),
                message(Message::ColumnStatus),
                24,
                active_direction(InstalledSortColumn::Status),
            ),
        ],
        rows,
        selected_rows,
        row_keys,
        Some("installed.row-select"),
    );
    let grid = ui.presentation_semantic(
        grid,
        "management.extensions.installed.items",
        &["collection", "primary-content", "resizable"],
    );

    let details = match selected {
        Some(package) => build_installed_details(ui, state, package),
        None => ui.text("Select an installed extension to view details."),
    };
    let details = ui.presentation_semantic(
        details,
        "management.extensions.installed.selection-details",
        &["inspector", "selection-context"],
    );
    let mut content = vec![title, search];
    if visible_count > MAX_INSTALLED_RESULTS {
        content.push(ui.text(format!(
            "Showing first {MAX_INSTALLED_RESULTS} of {visible_count} matches. Narrow the search to find more."
        )));
    }
    content.extend([grid, details]);
    let content = ui.column(content);
    let content = ui.presentation_semantic(
        content,
        "management.extensions.installed.content",
        &["primary-content"],
    );
    let commands = build_installed_commands(ui, state, selected);
    let commands = ui.presentation_semantic(
        commands,
        "management.extensions.installed.commands",
        &["command-set", "selection-context"],
    );
    let layout = ui.named_split(
        "installed.split",
        "horizontal",
        vec![(content, 100), (commands, 24)],
    );
    ui.presentation_semantic(
        layout,
        "management.extensions.installed.layout",
        &["resizable", "workspace-layout"],
    )
}

fn build_repositories(ui: &mut UiBuilder, state: &ManagerState) -> String {
    let close = ui.button_with_appearance(
        message(Message::Close),
        "view.close",
        true,
        None,
        ButtonAppearance::Subtle,
    );
    let heading = ui.markdown(format!("## {}", message(Message::Repositories)));
    let title_row = ui.row(vec![heading, close]);
    let intro = ui.markdown(
        "Repositories provide registry index.json files. Earlier repositories have higher priority when package ids collide.",
    );
    let input = ui.input(
        &state.repository_input,
        "https://example.org/rintawa/index.json",
        "repository.input",
        None,
    );
    let add = ui.button_with_appearance(
        message(Message::AddRepository),
        "repository.add",
        true,
        None,
        ButtonAppearance::Primary,
    );
    let add_row = ui.row(vec![input, add]);

    let mut rows = Vec::new();
    for (index, repository) in state.repositories.iter().enumerate() {
        let built_in = repository.url == BUILTIN_REPOSITORY;
        let status = state.repository_status.get(&repository.url);
        let detail = match status {
            Some(status) if status.loaded => {
                format!("loaded · {} packages", status.package_count)
            }
            Some(status) => format!(
                "error · {}",
                status
                    .error
                    .as_deref()
                    .unwrap_or("unknown repository error")
            ),
            None => "not refreshed this session".to_string(),
        };
        let title = ui.text(format!(
            "{}{}",
            repository.url,
            if built_in { " · built-in" } else { "" }
        ));
        let detail = ui.text(detail);
        let toggle = ui.button(
            if repository.enabled {
                message(Message::Disable)
            } else {
                message(Message::Enable)
            },
            "repository.toggle",
            true,
            Some(ActionTarget::Repository(index)),
        );
        let mut actions = vec![toggle];
        if !built_in {
            actions.push(ui.button(
                message(Message::Remove),
                "repository.remove",
                true,
                Some(ActionTarget::Repository(index)),
            ));
        }
        let actions = ui.row(actions);
        rows.push(ui.column(vec![title, detail, actions]));
    }
    let list = ui.list(rows);
    ui.column(vec![title_row, intro, list, add_row])
}

fn build_direct_url(ui: &mut UiBuilder, state: &ManagerState) -> String {
    let close = ui.button_with_appearance(
        message(Message::Close),
        "view.close",
        true,
        None,
        ButtonAppearance::Subtle,
    );
    let heading = ui.markdown(format!("## {}", message(Message::InstallFromUrl)));
    let title_row = ui.row(vec![heading, close]);
    let intro = ui.markdown(
        "Install a Rintawa extension directly from an **HTTPS .rtw URL**. The Core still validates the RTW before it enters CAS.",
    );
    let input = ui.input(
        &state.direct_url,
        "https://example.org/extension.rtw",
        "direct.input",
        None,
    );
    let install = ui.button(
        message(Message::DownloadAndInstall),
        "direct.install",
        true,
        None,
    );
    ui.column(vec![title_row, intro, input, install])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(subject: &str, version: &str, enabled: bool) -> InstalledPackage {
        InstalledPackage {
            subject: subject.to_string(),
            name: subject.to_string(),
            version_text: Some(version.to_string()),
            version: Some(Version::parse(version).unwrap()),
            digest: format!("sha256:{subject}"),
            instance_id: subject.to_string(),
            scope_id: "host".to_string(),
            enabled,
        }
    }

    fn prepared(dependencies: Option<Vec<&str>>) -> PreparedInstallItem {
        PreparedInstallItem {
            digest: "sha256:test".to_string(),
            subject: "example.package".to_string(),
            name: "Example Package".to_string(),
            version: "2.0.0".to_string(),
            origin: "test".to_string(),
            dependencies: dependencies
                .map(|items| items.into_iter().map(str::to_string).collect::<Vec<_>>()),
            permissions: vec![
                PreparedPermission {
                    component_id: "runtime".to_string(),
                    permission: "http-fetch".to_string(),
                    selected: true,
                },
                PreparedPermission {
                    component_id: "runtime".to_string(),
                    permission: "background-task".to_string(),
                    selected: true,
                },
            ],
        }
    }

    fn seed_dependency_state(runtime_enabled: bool) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            *state = ManagerState::default();
            state.installed.insert(
                "rintawa.web-runtime".to_string(),
                installed("rintawa.web-runtime", "0.0.1", runtime_enabled),
            );
            state.installed.insert(
                "rintawa.web-ui".to_string(),
                installed("rintawa.web-ui", "0.0.3", true),
            );
            state.managed_installs.insert(
                "rintawa.web-ui".to_string(),
                ManagedInstallRecord {
                    version: "0.0.3".to_string(),
                    dependencies: vec!["rintawa.web-runtime".to_string()],
                    dependencies_known: true,
                },
            );
        });
    }

    #[test]
    fn test_should_preserve_known_dependencies_for_direct_update() {
        let previous = ManagedInstallRecord {
            version: "1.0.0".to_string(),
            dependencies: vec!["example.runtime".to_string()],
            dependencies_known: true,
        };

        let direct = managed_install_record_for(&prepared(None), Some(&previous));
        assert_eq!(direct.version, "2.0.0");
        assert_eq!(direct.dependencies, vec!["example.runtime".to_string()]);
        assert!(direct.dependencies_known);

        let repository = managed_install_record_for(
            &prepared(Some(vec!["example.new-runtime"])),
            Some(&previous),
        );
        assert_eq!(
            repository.dependencies,
            vec!["example.new-runtime".to_string()]
        );
        assert!(repository.dependencies_known);
    }

    #[test]
    fn test_should_update_only_targeted_pending_permission() {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            *state = ManagerState::default();
            state.pending_install = Some(PendingInstall {
                origin: "test".to_string(),
                items: vec![prepared(None)],
            });
        });

        set_pending_permission(0, 1, false).unwrap();

        STATE.with(|state| {
            let state = state.borrow();
            let permissions = &state.pending_install.as_ref().unwrap().items[0].permissions;
            assert!(permissions[0].selected);
            assert!(!permissions[1].selected);
        });
    }

    #[test]
    fn test_should_block_removing_managed_dependency_without_registry() {
        seed_dependency_state(true);
        assert_eq!(
            dependency_blockers("rintawa.web-runtime"),
            DependencyBlockers {
                required_by: vec!["rintawa.web-ui".to_string()],
                unknown: Vec::new(),
            }
        );
        assert_eq!(
            dependency_blockers("rintawa.web-ui"),
            DependencyBlockers {
                required_by: Vec::new(),
                unknown: vec!["rintawa.web-runtime".to_string()],
            }
        );
    }

    #[test]
    fn test_should_report_disabled_dependency_before_enabling_consumer() {
        seed_dependency_state(false);
        assert_eq!(
            dependency_issues_for_enable("rintawa.web-ui").unwrap(),
            vec!["rintawa.web-runtime (disabled)".to_string()]
        );
    }

    #[test]
    fn test_should_clear_installed_selection_when_search_hides_it() {
        seed_dependency_state(true);
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.selected_package = Some("rintawa.web-ui".to_string());
            state.selected_version = Some(Version::parse("0.0.3").unwrap());
        });

        set_installed_search("runtime".to_string());

        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.installed_search, "runtime");
            assert!(state.selected_package.is_none());
            assert!(state.selected_version.is_none());
        });
    }

    #[test]
    fn test_should_toggle_installed_sort_direction_and_reset_for_new_column() {
        STATE.with(|state| *state.borrow_mut() = ManagerState::default());

        set_installed_sort(InstalledSortColumn::Name);
        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.installed_sort, InstalledSortColumn::Name);
            assert_eq!(state.installed_sort_direction, SortDirection::Descending);
        });

        set_installed_sort(InstalledSortColumn::Provider);
        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.installed_sort, InstalledSortColumn::Provider);
            assert_eq!(state.installed_sort_direction, SortDirection::Ascending);
        });
    }

    #[test]
    fn test_should_select_installed_extension_from_stable_row_identity() {
        seed_dependency_state(true);

        select_installed_package("rintawa.web-ui").unwrap();

        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.selected_package.as_deref(), Some("rintawa.web-ui"));
            assert_eq!(
                state.selected_version.as_ref(),
                Some(&Version::parse("0.0.3").unwrap())
            );
            assert_eq!(state.view, View::Installed);
        });
    }

    #[test]
    fn test_should_close_secondary_views_in_lifo_order() {
        STATE.with(|state| *state.borrow_mut() = ManagerState::default());

        open_view(View::Browse);
        open_view(View::Repositories);
        close_view();
        STATE.with(|state| assert_eq!(state.borrow().view, View::Browse));

        close_view();
        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.view, View::Installed);
            assert!(state.view_history.is_empty());
        });
    }
}

#[cfg(target_arch = "wasm32")]
export!(PackageManager);
