//! Repository index assembly, validation, and deterministic registry publishing.

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

use crate::model::{
    DEPENDENCY_REGISTRY_SCHEMA, LEGACY_REGISTRY_SCHEMA, PublishedAsset, PublishedRepository,
    REGISTRY_SCHEMA, RegistryIndex, RegistryPackage, RegistryVersion, ReleaseMetadata,
    RepositoryManifest,
};

const REPOSITORY_MANIFEST_FILE: &str = "registry.toml";
const REPOSITORY_MANIFEST_SCHEMA: u32 = 1;
const MAX_REPOSITORY_ICON_BYTES: u64 = 512 * 1024;

pub fn build_index(metadata_directory: &Path, output: &Path) -> Result<()> {
    let mut releases = Vec::new();
    if metadata_directory.exists() {
        for entry in fs::read_dir(metadata_directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let bytes = fs::read(&path)?;
            let metadata: ReleaseMetadata = serde_json::from_slice(&bytes)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            ensure!(
                matches!(
                    metadata.schema,
                    LEGACY_REGISTRY_SCHEMA | DEPENDENCY_REGISTRY_SCHEMA | REGISTRY_SCHEMA
                ),
                "unsupported release metadata schema {} in {}",
                metadata.schema,
                path.display()
            );
            releases.push(metadata);
        }
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let repository = load_repository_metadata(metadata_directory, output)?;
    let index = assemble_index(releases, repository)?;
    fs::write(output, serde_json::to_vec_pretty(&index)?)?;
    Ok(())
}

fn assemble_index(
    releases: Vec<ReleaseMetadata>,
    repository: Option<PublishedRepository>,
) -> Result<RegistryIndex> {
    let mut grouped: BTreeMap<String, Vec<ReleaseMetadata>> = BTreeMap::new();
    for release in releases {
        grouped
            .entry(release.package.id.clone())
            .or_default()
            .push(release);
    }

    let mut packages = Vec::new();
    for (id, mut versions) in grouped {
        versions.sort_by(|left, right| right.package.version.cmp(&left.package.version));
        let latest = versions
            .iter()
            .find(|release| release.package.version.pre.is_empty())
            .or_else(|| versions.first())
            .context("package group unexpectedly empty")?
            .clone();
        validate_group(&id, &versions)?;
        packages.push(RegistryPackage {
            id,
            slug: latest.package.slug.clone(),
            name: latest.package.name.clone(),
            description: latest.package.description.clone(),
            license: latest.package.license.clone(),
            authors: latest.package.authors.clone(),
            homepage: latest.package.homepage.clone(),
            source_url: latest.package.source_url.clone(),
            logo: latest.package.logo.clone(),
            readme: latest.package.readme.clone(),
            latest: latest.package.version.clone(),
            versions: versions.into_iter().map(to_registry_version).collect(),
        });
    }
    Ok(RegistryIndex {
        schema: REGISTRY_SCHEMA,
        repository,
        packages,
    })
}

fn load_repository_metadata(
    metadata_directory: &Path,
    output: &Path,
) -> Result<Option<PublishedRepository>> {
    let repository_root = metadata_directory
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let manifest_path = repository_root.join(REPOSITORY_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let source = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: RepositoryManifest = toml::from_str(&source)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    ensure!(
        manifest.schema == REPOSITORY_MANIFEST_SCHEMA,
        "unsupported registry.toml schema {}",
        manifest.schema
    );
    ensure!(
        !manifest.repository.id.trim().is_empty() && manifest.repository.id.len() <= 128,
        "repository id must contain 1..=128 bytes"
    );
    ensure!(
        !manifest.repository.name.trim().is_empty() && manifest.repository.name.len() <= 128,
        "repository name must contain 1..=128 bytes"
    );
    validate_https_url(&manifest.repository.base_url, "repository base-url")?;
    ensure!(
        manifest.repository.base_url.ends_with('/'),
        "repository base-url must end with '/'"
    );
    if let Some(homepage) = manifest.repository.homepage.as_deref() {
        validate_https_url(homepage, "repository homepage")?;
    }

    let icon = manifest
        .repository
        .icon
        .as_deref()
        .map(|relative| {
            publish_repository_icon(
                repository_root,
                output,
                relative,
                &manifest.repository.base_url,
            )
        })
        .transpose()?;

    Ok(Some(PublishedRepository {
        id: manifest.repository.id,
        name: manifest.repository.name,
        homepage: manifest.repository.homepage,
        icon,
    }))
}

fn publish_repository_icon(
    repository_root: &Path,
    output: &Path,
    relative: &str,
    base_url: &str,
) -> Result<PublishedAsset> {
    validate_relative_path(relative)?;
    let source = repository_root.join(relative);
    let metadata = fs::symlink_metadata(&source)
        .with_context(|| format!("failed to inspect repository icon {}", source.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "repository icon must be a regular file"
    );
    ensure!(
        metadata.len() > 0 && metadata.len() <= MAX_REPOSITORY_ICON_BYTES,
        "repository icon must be between 1 and {MAX_REPOSITORY_ICON_BYTES} bytes"
    );

    let bytes = fs::read(&source)?;
    let (extension, media_type) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("png", "image/png")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        ("webp", "image/webp")
    } else {
        bail!("repository icon must be PNG or WebP");
    };
    let file_name = format!("repository.logo.{extension}");
    let output_directory = output
        .parent()
        .context("registry index output must have a parent directory")?;
    fs::write(output_directory.join(&file_name), &bytes)?;

    Ok(PublishedAsset {
        file: file_name.clone(),
        url: format!("{base_url}{file_name}"),
        sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
        size: u64::try_from(bytes.len()).context("repository icon size does not fit into u64")?,
        media_type: media_type.to_string(),
    })
}

fn validate_https_url(value: &str, label: &str) -> Result<()> {
    ensure!(
        value.starts_with("https://") && value.len() <= 2048,
        "{label} must be a bounded HTTPS URL"
    );
    let authority = &value["https://".len()..];
    ensure!(
        !authority.is_empty()
            && !authority.starts_with('/')
            && authority
                .split('/')
                .next()
                .is_some_and(|host| !host.is_empty() && !host.contains('@')),
        "{label} is invalid"
    );
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    ensure!(!value.is_empty(), "repository icon path cannot be empty");
    ensure!(!path.is_absolute(), "repository icon path must be relative");
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("repository icon path must not contain traversal or platform prefixes"),
        }
    }
    Ok(())
}

fn validate_group(id: &str, versions: &[ReleaseMetadata]) -> Result<()> {
    let first = versions
        .first()
        .context("package group unexpectedly empty")?;
    let mut seen_versions = std::collections::HashSet::new();
    for release in versions {
        ensure!(
            release.package.id == id,
            "package id changed inside registry group"
        );
        ensure!(
            release.package.slug == first.package.slug,
            "package '{id}' changed slug across releases"
        );
        ensure!(
            seen_versions.insert(release.package.version.clone()),
            "duplicate version '{}' for package '{id}'",
            release.package.version
        );
    }
    Ok(())
}

fn to_registry_version(release: ReleaseMetadata) -> RegistryVersion {
    RegistryVersion {
        version: release.package.version,
        content: release.content,
        dependencies: release.dependencies,
        artifact: release.artifact,
        source: release.source,
    }
}

#[cfg(test)]
mod tests {
    use semver::Version;

    use super::*;
    use crate::model::{PackageDependency, PublishedArtifact, PublishedPackage, PublishedSource};

    fn release(version: &str) -> ReleaseMetadata {
        ReleaseMetadata {
            schema: REGISTRY_SCHEMA,
            package: PublishedPackage {
                id: "rintawa.example".into(),
                slug: "example".into(),
                name: "Example".into(),
                version: Version::parse(version).expect("test version must parse"),
                description: "Example package".into(),
                license: "GPL-3.0-only".into(),
                authors: Vec::new(),
                homepage: None,
                source_url: None,
                logo: None,
                readme: None,
            },
            content: "rintawa.extension@1".into(),
            dependencies: Vec::new(),
            artifact: PublishedArtifact {
                file: "example.rtw".into(),
                url: format!("https://example.invalid/pkg-example-v{version}/example.rtw"),
                sha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .into(),
                size: 1,
            },
            source: PublishedSource {
                repository: "martrtr/rtwKit".into(),
                tag: format!("pkg-example-v{version}"),
                commit: "deadbeef".into(),
            },
        }
    }

    #[test]
    fn test_should_select_highest_semver_as_latest() {
        let index = assemble_index(vec![release("1.2.0"), release("1.10.0")], None)
            .expect("registry should assemble");
        assert_eq!(index.packages.len(), 1);
        assert_eq!(index.packages[0].latest, Version::parse("1.10.0").unwrap());
        assert_eq!(
            index.packages[0].versions[0].version,
            Version::parse("1.10.0").unwrap()
        );
    }

    #[test]
    fn test_should_prefer_latest_stable_over_newer_prerelease() {
        let index = assemble_index(vec![release("1.9.0"), release("2.0.0-alpha.1")], None)
            .expect("registry should assemble");
        assert_eq!(index.packages[0].latest, Version::parse("1.9.0").unwrap());
        assert_eq!(
            index.packages[0].versions[0].version,
            Version::parse("2.0.0-alpha.1").unwrap()
        );
    }

    #[test]
    fn test_should_preserve_version_dependencies() {
        let mut item = release("1.2.0");
        item.dependencies.push(PackageDependency {
            id: "rintawa.runtime".into(),
            requirement: ">=1.0.0, <2.0.0".parse().unwrap(),
        });

        let index = assemble_index(vec![item], None).expect("registry should assemble");
        assert_eq!(index.schema, REGISTRY_SCHEMA);
        assert_eq!(index.packages[0].versions[0].dependencies.len(), 1);
        assert_eq!(
            index.packages[0].versions[0].dependencies[0].id,
            "rintawa.runtime"
        );
    }

    #[test]
    fn test_should_reject_duplicate_package_versions() {
        let error = assemble_index(vec![release("1.0.0"), release("1.0.0")], None)
            .expect_err("duplicate versions must fail");
        assert!(error.to_string().contains("duplicate version"));
    }
}
