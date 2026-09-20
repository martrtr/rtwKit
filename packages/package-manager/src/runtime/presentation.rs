//! Renderer-neutral Portable UI composition for Package Manager state.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::runtime::rintawa::engine::host::{self, LogLevel};
use crate::{
    i18n::{
        Message, install_version, installed_release, message, replace_with_published_build,
        switch_to, update_to,
    },
    model::{BUILTIN_REPOSITORY, CatalogPackage},
    runtime::{
        ActionTarget, CachedImage, InstalledPackage, InstalledSortColumn, MAX_CARD_AUTHORS_BYTES,
        MAX_CARD_DESCRIPTION_BYTES, MAX_EMBEDDED_IMAGE_BASE64_BYTES, MAX_INSTALLED_RESULTS,
        MAX_SEARCH_RESULTS, ManagerState, PendingInstall, SortDirection, View,
        dependency_blockers_locked, dependency_issues_for_enable_locked, filtered_catalog_packages,
        installed_has_catalog_update, installed_provider, installed_status, repository_label,
        truncate_display_text, visible_installed_packages,
    },
};

pub(super) struct RenderedSurface {
    pub(super) nodes: Vec<Value>,
    pub(super) action_targets: BTreeMap<String, ActionTarget>,
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
            host::log(
                LogLevel::Error,
                &format!("Portable UI builder lost node '{id}' before semantic annotation"),
            );
            return id;
        };
        let Some(object) = node.as_object_mut() else {
            host::log(
                LogLevel::Error,
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

pub(super) fn build_surface(state: &ManagerState) -> RenderedSurface {
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

    let installed_package = state.installed.get(&package.id);
    let installed_version = installed_package.and_then(|item| item.version.as_ref());
    let selected_release = package
        .versions
        .iter()
        .find(|release| &release.version == selected_version);
    let same_version_different_build = installed_package
        .is_some_and(|item| item.version.as_ref() == Some(selected_version))
        && selected_release.is_some_and(|release| {
            installed_package
                .is_some_and(|item| !item.digest.eq_ignore_ascii_case(&release.artifact.sha256))
        });
    let install_enabled =
        installed_version != Some(selected_version) || same_version_different_build;
    let selected_version_text = selected_version.to_string();
    let action_label = match installed_version {
        Some(installed) if installed < selected_version => update_to(&selected_version_text),
        Some(installed) if installed > selected_version => switch_to(&selected_version_text),
        Some(_) if same_version_different_build => {
            replace_with_published_build(&selected_version_text)
        }
        Some(_) => installed_release(&selected_version_text),
        None => install_version(&selected_version_text),
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
            if installed_has_catalog_update(state, package) {
                let label = match package.version.as_ref() {
                    Some(version) if version == &catalog_package.latest => {
                        replace_with_published_build(&catalog_package.latest.to_string())
                    }
                    _ => update_to(&catalog_package.latest.to_string()),
                };
                actions.push(ui.button(
                    label,
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
