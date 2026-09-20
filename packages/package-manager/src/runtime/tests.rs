//! Unit tests for Package Manager runtime state and release semantics.

use super::*;
use crate::model::{PublishedArtifact, PublishedSource};

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

fn published_package(subject: &str, version: &str, digest: &str) -> CatalogPackage {
    let version = Version::parse(version).unwrap();
    CatalogPackage {
        repository_url: BUILTIN_REPOSITORY.to_string(),
        id: subject.to_string(),
        name: subject.to_string(),
        description: "test".to_string(),
        license: "GPL-3.0-only".to_string(),
        authors: Vec::new(),
        homepage: None,
        source_url: None,
        logo: None,
        readme: None,
        latest: version.clone(),
        versions: vec![RegistryVersion {
            version,
            content: "rintawa.extension@1".to_string(),
            dependencies: Vec::new(),
            artifact: PublishedArtifact {
                file: "package.rtw".to_string(),
                url: "https://example.invalid/package.rtw".to_string(),
                sha256: digest.to_string(),
                size: 1,
            },
            source: PublishedSource {
                repository: "example/repository".to_string(),
                tag: "pkg-example-v0.0.1".to_string(),
                commit: "deadbeef".to_string(),
            },
        }],
    }
}

fn seed_published_build_state(installed_digest: &str, published_digest: &str) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        *state = ManagerState::default();
        let mut package = installed("example.package", "0.0.1", true);
        package.digest = installed_digest.to_string();
        state
            .installed
            .insert("example.package".to_string(), package);
        state.catalog.packages.insert(
            "example.package".to_string(),
            published_package("example.package", "0.0.1", published_digest),
        );
    });
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
fn test_should_trust_core_cas_digest_for_downloaded_rtw_integrity() {
    assert!(
        verify_imported_artifact_digest("example.package", "sha256:abcdef", "SHA256:ABCDEF")
            .is_ok()
    );
    assert_eq!(
        verify_imported_artifact_digest("example.package", "sha256:abcdef", "sha256:123456")
            .unwrap_err(),
        "example.package SHA-256 mismatch; repository metadata or downloaded RTW is inconsistent"
    );
}

#[test]
fn test_should_offer_published_build_when_same_version_digest_differs() {
    seed_published_build_state("sha256:local", "sha256:published");

    STATE.with(|state| {
        let state = state.borrow();
        let package = state.installed.get("example.package").unwrap();
        assert!(installed_build_differs_from_catalog(&state, package));
        assert!(installed_has_catalog_update(&state, package));
        assert_eq!(
            installed_status(&state, package),
            InstalledStatus::PublishedBuildDiffers
        );
        assert_eq!(
            installed_provider(&state, package),
            message(Message::LocalUnpublishedBuild)
        );
        assert!(!installed_versions_for_solver(&state).contains_key("example.package"));

        let plan = solve_install_plan(
            &state.catalog,
            &installed_versions_for_solver(&state),
            "example.package",
            None,
        )
        .unwrap();
        assert_eq!(plan.selections.len(), 1);
        assert_eq!(plan.selections[0].package_id, "example.package");
        assert_eq!(
            plan.selections[0].version.version,
            Version::parse("0.0.1").unwrap()
        );
    });
}

#[test]
fn test_should_recognize_exact_published_build_by_version_and_digest() {
    seed_published_build_state("sha256:published", "SHA256:PUBLISHED");

    STATE.with(|state| {
        let state = state.borrow();
        let package = state.installed.get("example.package").unwrap();
        assert!(!installed_build_differs_from_catalog(&state, package));
        assert!(!installed_has_catalog_update(&state, package));
        assert_eq!(installed_status(&state, package), InstalledStatus::Enabled);
        assert_eq!(installed_provider(&state, package), "rtwKit");
        assert_eq!(
            installed_versions_for_solver(&state).get("example.package"),
            Some(&Version::parse("0.0.1").unwrap())
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
