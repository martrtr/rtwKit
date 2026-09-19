//! Repository metadata, catalog normalization, and package dependency resolution.

use std::collections::{BTreeMap, BTreeSet};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

pub(crate) const BUILTIN_REPOSITORY: &str = "https://martrtr.github.io/rtwKit/index.json";
pub(crate) const SUPPORTED_REGISTRY_SCHEMAS: &[u32] = &[1, 2, 3];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepositoryPreference {
    pub(crate) url: String,
    pub(crate) enabled: bool,
}

impl RepositoryPreference {
    pub(crate) fn builtin() -> Self {
        Self {
            url: BUILTIN_REPOSITORY.to_string(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RegistryIndex {
    pub(crate) schema: u32,
    #[serde(default)]
    pub(crate) repository: Option<PublishedRepository>,
    pub(crate) packages: Vec<RegistryPackage>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PublishedRepository {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) homepage: Option<String>,
    #[serde(default)]
    pub(crate) icon: Option<PublishedAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RegistryPackage {
    pub(crate) id: String,
    pub(crate) slug: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) license: String,
    #[serde(default)]
    pub(crate) authors: Vec<String>,
    #[serde(default)]
    pub(crate) homepage: Option<String>,
    #[serde(default)]
    pub(crate) source_url: Option<String>,
    #[serde(default)]
    pub(crate) logo: Option<PublishedAsset>,
    #[serde(default)]
    pub(crate) readme: Option<PublishedAsset>,
    pub(crate) latest: Version,
    pub(crate) versions: Vec<RegistryVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RegistryVersion {
    pub(crate) version: Version,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) dependencies: Vec<PackageDependency>,
    pub(crate) artifact: PublishedArtifact,
    pub(crate) source: PublishedSource,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PackageDependency {
    pub(crate) id: String,
    pub(crate) requirement: VersionReq,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PublishedArtifact {
    pub(crate) file: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct PublishedAsset {
    pub(crate) file: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
    pub(crate) media_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PublishedSource {
    pub(crate) repository: String,
    pub(crate) tag: String,
    pub(crate) commit: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CatalogPackage {
    pub(crate) repository_url: String,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) license: String,
    pub(crate) authors: Vec<String>,
    pub(crate) homepage: Option<String>,
    pub(crate) source_url: Option<String>,
    pub(crate) logo: Option<PublishedAsset>,
    pub(crate) readme: Option<PublishedAsset>,
    pub(crate) latest: Version,
    pub(crate) versions: Vec<RegistryVersion>,
}

#[derive(Debug, Clone)]
pub(crate) struct Catalog {
    pub(crate) packages: BTreeMap<String, CatalogPackage>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct InstallSelection {
    pub(crate) package_id: String,
    pub(crate) repository_url: String,
    pub(crate) version: RegistryVersion,
}

#[derive(Debug, Clone)]
pub(crate) struct InstallPlan {
    pub(crate) selections: Vec<InstallSelection>,
    pub(crate) resolved: BTreeMap<String, Version>,
}

pub(crate) fn normalize_repositories(
    stored: Option<Vec<RepositoryPreference>>,
) -> Vec<RepositoryPreference> {
    let mut repositories = stored.unwrap_or_else(|| vec![RepositoryPreference::builtin()]);
    if let Some(position) = repositories
        .iter()
        .position(|repository| repository.url == BUILTIN_REPOSITORY)
    {
        let builtin = repositories.remove(position);
        repositories.insert(0, builtin);
    } else {
        repositories.insert(0, RepositoryPreference::builtin());
    }

    let mut seen = BTreeSet::new();
    repositories.retain(|repository| seen.insert(repository.url.clone()));
    repositories
}

pub(crate) fn validate_repository_url(url: &str) -> Result<(), String> {
    validate_https_url(url, "repository URL")?;
    if url.len() > 2048 {
        return Err("repository URL exceeds 2048 bytes".to_string());
    }
    Ok(())
}

pub(crate) fn validate_rtw_url(url: &str) -> Result<(), String> {
    validate_https_url(url, "RTW URL")?;
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    if !without_query.to_ascii_lowercase().ends_with(".rtw") {
        return Err("direct install URL must point to a .rtw file".to_string());
    }
    if url.len() > 4096 {
        return Err("RTW URL exceeds 4096 bytes".to_string());
    }
    Ok(())
}

fn validate_presentation_asset(
    asset: &PublishedAsset,
    label: &str,
    max_size: u64,
    allowed_media_types: &[&str],
) -> Result<(), String> {
    validate_https_url(&asset.url, label)?;
    validate_sha256(&asset.sha256)?;
    if asset.file.trim().is_empty() {
        return Err(format!("{label} file name cannot be empty"));
    }
    if asset.size == 0 || asset.size > max_size {
        return Err(format!(
            "{label} size {} is outside the allowed 1..={max_size} bytes",
            asset.size
        ));
    }
    if !allowed_media_types
        .iter()
        .any(|media_type| asset.media_type.eq_ignore_ascii_case(media_type))
    {
        return Err(format!(
            "{label} media type '{}' is unsupported",
            asset.media_type
        ));
    }
    Ok(())
}

fn validate_https_url(url: &str, label: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("{label} must use HTTPS"));
    }
    let authority = &url["https://".len()..];
    if authority.is_empty()
        || authority.starts_with('/')
        || authority
            .split('/')
            .next()
            .is_some_and(|host| host.is_empty() || host.contains('@'))
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

pub(crate) fn parse_registry(source: &[u8]) -> Result<RegistryIndex, String> {
    let index: RegistryIndex = serde_json::from_slice(source)
        .map_err(|error| format!("invalid registry JSON: {error}"))?;
    validate_registry(&index)?;
    Ok(index)
}

fn validate_registry(index: &RegistryIndex) -> Result<(), String> {
    if !SUPPORTED_REGISTRY_SCHEMAS.contains(&index.schema) {
        return Err(format!("unsupported registry schema {}", index.schema));
    }
    if let Some(repository) = index.repository.as_ref() {
        if repository.id.trim().is_empty()
            || repository.id.len() > 128
            || repository.name.trim().is_empty()
            || repository.name.len() > 128
        {
            return Err("repository identity metadata is invalid".to_string());
        }
        if let Some(homepage) = repository.homepage.as_deref() {
            validate_https_url(homepage, "repository homepage")?;
        }
        if let Some(icon) = repository.icon.as_ref() {
            validate_presentation_asset(
                icon,
                "repository icon",
                512 * 1024,
                &["image/png", "image/webp"],
            )?;
        }
    }
    let mut ids = BTreeSet::new();
    for package in &index.packages {
        if package.id.trim().is_empty() || package.name.trim().is_empty() {
            return Err("registry package id/name cannot be empty".to_string());
        }
        if package.slug.trim().is_empty() {
            return Err(format!("package '{}' has an empty slug", package.id));
        }
        if !ids.insert(package.id.clone()) {
            return Err(format!("duplicate package id '{}'", package.id));
        }
        if package.authors.len() > 32
            || package
                .authors
                .iter()
                .any(|author| author.trim().is_empty() || author.len() > 128)
        {
            return Err(format!(
                "package '{}' has invalid authors metadata",
                package.id
            ));
        }
        if let Some(url) = package.homepage.as_deref() {
            validate_https_url(url, "package homepage")?;
        }
        if let Some(url) = package.source_url.as_deref() {
            validate_https_url(url, "package source URL")?;
        }
        if let Some(asset) = package.logo.as_ref() {
            validate_presentation_asset(
                asset,
                "package logo",
                512 * 1024,
                &["image/png", "image/webp"],
            )?;
        }
        if let Some(asset) = package.readme.as_ref() {
            validate_presentation_asset(
                asset,
                "package README",
                1024 * 1024,
                &["text/markdown; charset=utf-8", "text/markdown"],
            )?;
        }
        if package.versions.is_empty() {
            return Err(format!("package '{}' has no versions", package.id));
        }
        if !package
            .versions
            .iter()
            .any(|version| version.version == package.latest)
        {
            return Err(format!(
                "package '{}' latest {} is absent from versions",
                package.id, package.latest
            ));
        }

        let mut versions = BTreeSet::new();
        for version in &package.versions {
            if version.content != "rintawa.extension@1" {
                return Err(format!(
                    "package '{}' version {} has unsupported content '{}'",
                    package.id, version.version, version.content
                ));
            }
            if !versions.insert(version.version.clone()) {
                return Err(format!(
                    "package '{}' repeats version {}",
                    package.id, version.version
                ));
            }
            validate_rtw_url(&version.artifact.url)?;
            if version.artifact.file.trim().is_empty()
                || !version.artifact.file.to_ascii_lowercase().ends_with(".rtw")
            {
                return Err(format!(
                    "package '{}' version {} has an invalid artifact file name",
                    package.id, version.version
                ));
            }
            if version.source.repository.trim().is_empty()
                || version.source.tag.trim().is_empty()
                || version.source.commit.trim().is_empty()
            {
                return Err(format!(
                    "package '{}' version {} has incomplete source provenance",
                    package.id, version.version
                ));
            }
            validate_sha256(&version.artifact.sha256)?;
            if version.artifact.size == 0 {
                return Err(format!(
                    "package '{}' version {} has zero-sized artifact",
                    package.id, version.version
                ));
            }

            let mut dependency_ids = BTreeSet::new();
            for dependency in &version.dependencies {
                if dependency.id == package.id {
                    return Err(format!(
                        "package '{}' version {} depends on itself",
                        package.id, version.version
                    ));
                }
                if !dependency_ids.insert(dependency.id.clone()) {
                    return Err(format!(
                        "package '{}' version {} repeats dependency '{}'",
                        package.id, version.version, dependency.id
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_sha256(value: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err("artifact digest must use sha256:<hex>".to_string());
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("artifact SHA-256 must contain exactly 64 hex digits".to_string());
    }
    Ok(())
}

pub(crate) fn merge_catalog(
    repositories: impl IntoIterator<Item = (String, RegistryIndex)>,
) -> Catalog {
    let mut packages: BTreeMap<String, CatalogPackage> = BTreeMap::new();
    let mut warnings = Vec::new();

    for (repository_url, index) in repositories {
        for package in index.packages {
            if let Some(existing) = packages.get(&package.id) {
                warnings.push(format!(
                    "package '{}' from {} ignored; {} has higher repository priority",
                    package.id, repository_url, existing.repository_url
                ));
                continue;
            }

            let mut versions = package.versions;
            versions.sort_by(|left, right| right.version.cmp(&left.version));
            packages.insert(
                package.id.clone(),
                CatalogPackage {
                    repository_url: repository_url.clone(),
                    id: package.id,
                    name: package.name,
                    description: package.description,
                    license: package.license,
                    authors: package.authors,
                    homepage: package.homepage,
                    source_url: package.source_url,
                    logo: package.logo,
                    readme: package.readme,
                    latest: package.latest,
                    versions,
                },
            );
        }
    }

    Catalog { packages, warnings }
}

pub(crate) fn solve_install_plan(
    catalog: &Catalog,
    installed: &BTreeMap<String, Version>,
    root_id: &str,
    target: Option<&Version>,
) -> Result<InstallPlan, String> {
    let root = catalog
        .packages
        .get(root_id)
        .ok_or_else(|| format!("package '{root_id}' is not in the loaded catalog"))?;
    let root_version = target.cloned().unwrap_or_else(|| root.latest.clone());
    if !root
        .versions
        .iter()
        .any(|version| version.version == root_version)
    {
        return Err(format!(
            "package '{}' does not contain version {}",
            root_id, root_version
        ));
    }

    let mut requirements: BTreeMap<String, Vec<VersionReq>> = BTreeMap::new();
    requirements
        .entry(root_id.to_string())
        .or_default()
        .push(VersionReq::parse(&format!("={root_version}")).map_err(|error| error.to_string())?);

    let chosen = solve_recursive(catalog, installed, root_id, requirements, BTreeMap::new())?;
    let order = dependency_order(catalog, &chosen, root_id)?;

    let mut selections = Vec::new();
    for package_id in order {
        let package = catalog
            .packages
            .get(&package_id)
            .ok_or_else(|| format!("package '{package_id}' disappeared from catalog"))?;
        let version = chosen
            .get(&package_id)
            .ok_or_else(|| format!("package '{package_id}' has no solved version"))?;
        if installed.get(&package_id) == Some(version) {
            continue;
        }
        let release = package
            .versions
            .iter()
            .find(|candidate| &candidate.version == version)
            .ok_or_else(|| format!("solved version {version} of '{package_id}' disappeared"))?
            .clone();
        selections.push(InstallSelection {
            package_id: package_id.clone(),
            repository_url: package.repository_url.clone(),
            version: release,
        });
    }

    Ok(InstallPlan {
        selections,
        resolved: chosen,
    })
}

fn solve_recursive(
    catalog: &Catalog,
    installed: &BTreeMap<String, Version>,
    root_id: &str,
    requirements: BTreeMap<String, Vec<VersionReq>>,
    chosen: BTreeMap<String, Version>,
) -> Result<BTreeMap<String, Version>, String> {
    for (id, version) in &chosen {
        if let Some(requirements_for_id) = requirements.get(id)
            && !requirements_for_id
                .iter()
                .all(|requirement| requirement.matches(version))
        {
            return Err(format!(
                "chosen version {} of '{}' no longer satisfies dependency constraints",
                version, id
            ));
        }
    }

    let unresolved = requirements
        .keys()
        .find(|id| !chosen.contains_key(*id))
        .cloned();
    let Some(package_id) = unresolved else {
        return Ok(chosen);
    };

    let package = catalog
        .packages
        .get(&package_id)
        .ok_or_else(|| format!("missing dependency '{package_id}' in loaded repositories"))?;
    let constraints = requirements.get(&package_id).ok_or_else(|| {
        format!("dependency solver lost constraints for unresolved package '{package_id}'")
    })?;

    let mut candidates: Vec<&RegistryVersion> = package
        .versions
        .iter()
        .filter(|release| {
            constraints
                .iter()
                .all(|requirement| requirement.matches(&release.version))
        })
        .collect();
    if candidates.is_empty() {
        let joined = constraints
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" AND ");
        return Err(format!(
            "no version of '{}' satisfies {}",
            package_id, joined
        ));
    }

    candidates.sort_by(|left, right| right.version.cmp(&left.version));
    if package_id != root_id
        && let Some(installed_version) = installed.get(&package_id)
        && let Some(position) = candidates
            .iter()
            .position(|release| &release.version == installed_version)
    {
        let installed_candidate = candidates.remove(position);
        candidates.insert(0, installed_candidate);
    }

    let mut last_error = None;
    for candidate in candidates {
        let mut next_requirements = requirements.clone();
        for dependency in &candidate.dependencies {
            next_requirements
                .entry(dependency.id.clone())
                .or_default()
                .push(dependency.requirement.clone());
        }
        let mut next_chosen = chosen.clone();
        next_chosen.insert(package_id.clone(), candidate.version.clone());

        match solve_recursive(catalog, installed, root_id, next_requirements, next_chosen) {
            Ok(solution) => return Ok(solution),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| format!("could not resolve dependencies for package '{package_id}'")))
}

fn dependency_order(
    catalog: &Catalog,
    chosen: &BTreeMap<String, Version>,
    root_id: &str,
) -> Result<Vec<String>, String> {
    fn visit(
        id: &str,
        catalog: &Catalog,
        chosen: &BTreeMap<String, Version>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.to_string()) {
            return Err(format!("package dependency cycle contains '{id}'"));
        }

        let package = catalog
            .packages
            .get(id)
            .ok_or_else(|| format!("missing package '{id}' while ordering install plan"))?;
        let version = chosen
            .get(id)
            .ok_or_else(|| format!("missing chosen version for '{id}'"))?;
        let release = package
            .versions
            .iter()
            .find(|release| &release.version == version)
            .ok_or_else(|| format!("missing chosen release {version} for '{id}'"))?;

        for dependency in &release.dependencies {
            if chosen.contains_key(&dependency.id) {
                visit(&dependency.id, catalog, chosen, visiting, visited, order)?;
            }
        }

        visiting.remove(id);
        visited.insert(id.to_string());
        order.push(id.to_string());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    visit(
        root_id,
        catalog,
        chosen,
        &mut visiting,
        &mut visited,
        &mut order,
    )?;
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str, dependencies: &[(&str, &str)]) -> RegistryVersion {
        RegistryVersion {
            version: Version::parse(version).unwrap(),
            content: "rintawa.extension@1".to_string(),
            dependencies: dependencies
                .iter()
                .map(|(id, requirement)| PackageDependency {
                    id: (*id).to_string(),
                    requirement: VersionReq::parse(requirement).unwrap(),
                })
                .collect(),
            artifact: PublishedArtifact {
                file: "x.rtw".to_string(),
                url: "https://example.invalid/x.rtw".to_string(),
                sha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
                size: 1,
            },
            source: PublishedSource {
                repository: "example/repo".to_string(),
                tag: "v1".to_string(),
                commit: "deadbeef".to_string(),
            },
        }
    }

    fn package(id: &str, latest: &str, versions: Vec<RegistryVersion>) -> (String, CatalogPackage) {
        (
            id.to_string(),
            CatalogPackage {
                repository_url: BUILTIN_REPOSITORY.to_string(),
                id: id.to_string(),
                name: id.to_string(),
                description: "test".to_string(),
                license: "GPL-3.0-only".to_string(),
                authors: Vec::new(),
                homepage: None,
                source_url: None,
                logo: None,
                readme: None,
                latest: Version::parse(latest).unwrap(),
                versions,
            },
        )
    }

    #[test]
    fn test_should_normalize_builtin_repository_to_first_position() {
        let repositories = normalize_repositories(Some(vec![
            RepositoryPreference {
                url: "https://example.invalid/index.json".to_string(),
                enabled: true,
            },
            RepositoryPreference {
                url: BUILTIN_REPOSITORY.to_string(),
                enabled: false,
            },
        ]));
        assert_eq!(repositories[0].url, BUILTIN_REPOSITORY);
        assert!(!repositories[0].enabled);
    }

    #[test]
    fn test_should_parse_registry_schema_one_without_dependencies() {
        let source = br#"{
          "schema": 1,
          "packages": [{
            "id":"rintawa.demo",
            "slug":"demo",
            "name":"Demo",
            "description":"Demo",
            "license":"GPL-3.0-only",
            "latest":"1.0.0",
            "versions":[{
              "version":"1.0.0",
              "content":"rintawa.extension@1",
              "artifact":{
                "file":"demo.rtw",
                "url":"https://example.invalid/demo.rtw",
                "sha256":"sha256:0000000000000000000000000000000000000000000000000000000000000000",
                "size":1
              },
              "source":{"repository":"example/repo","tag":"v1","commit":"deadbeef"}
            }]
          }]
        }"#;
        let index = parse_registry(source).unwrap();
        assert!(index.packages[0].versions[0].dependencies.is_empty());
    }

    #[test]
    fn test_should_parse_repository_identity_metadata() {
        let source = br#"{
          "schema": 3,
          "repository": {
            "id": "example",
            "name": "Example Repository",
            "homepage": "https://example.invalid/",
            "icon": {
              "file": "repository.logo.png",
              "url": "https://example.invalid/repository.logo.png",
              "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
              "size": 1,
              "media_type": "image/png"
            }
          },
          "packages": []
        }"#;
        let index = parse_registry(source).expect("repository metadata should parse");
        let repository = index
            .repository
            .expect("repository identity should be present");
        assert_eq!(repository.id, "example");
        assert_eq!(repository.name, "Example Repository");
    }

    #[test]
    fn test_should_install_dependency_before_root() {
        let catalog = Catalog {
            packages: BTreeMap::from([
                package(
                    "rintawa.runtime",
                    "1.1.0",
                    vec![release("1.1.0", &[]), release("1.0.0", &[])],
                ),
                package(
                    "rintawa.ui",
                    "2.0.0",
                    vec![release("2.0.0", &[("rintawa.runtime", ">=1.0.0, <2.0.0")])],
                ),
            ]),
            warnings: Vec::new(),
        };

        let plan = solve_install_plan(&catalog, &BTreeMap::new(), "rintawa.ui", None).unwrap();
        assert_eq!(
            plan.selections
                .iter()
                .map(|item| item.package_id.as_str())
                .collect::<Vec<_>>(),
            vec!["rintawa.runtime", "rintawa.ui"]
        );
    }

    #[test]
    fn test_should_prefer_compatible_installed_dependency() {
        let catalog = Catalog {
            packages: BTreeMap::from([
                package(
                    "rintawa.runtime",
                    "1.1.0",
                    vec![release("1.1.0", &[]), release("1.0.0", &[])],
                ),
                package(
                    "rintawa.ui",
                    "2.0.0",
                    vec![release("2.0.0", &[("rintawa.runtime", ">=1.0.0, <2.0.0")])],
                ),
            ]),
            warnings: Vec::new(),
        };
        let installed = BTreeMap::from([(
            "rintawa.runtime".to_string(),
            Version::parse("1.0.0").unwrap(),
        )]);

        let plan = solve_install_plan(&catalog, &installed, "rintawa.ui", None).unwrap();
        assert_eq!(plan.selections.len(), 1);
        assert_eq!(plan.selections[0].package_id, "rintawa.ui");
    }

    #[test]
    fn test_should_reject_dependency_cycles() {
        let catalog = Catalog {
            packages: BTreeMap::from([
                package(
                    "rintawa.a",
                    "1.0.0",
                    vec![release("1.0.0", &[("rintawa.b", "*")])],
                ),
                package(
                    "rintawa.b",
                    "1.0.0",
                    vec![release("1.0.0", &[("rintawa.a", "*")])],
                ),
            ]),
            warnings: Vec::new(),
        };
        let error = solve_install_plan(&catalog, &BTreeMap::new(), "rintawa.a", None).unwrap_err();
        assert!(error.contains("cycle"));
    }
}
