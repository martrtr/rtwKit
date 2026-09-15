use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::model::{
    REGISTRY_SCHEMA, RegistryIndex, RegistryPackage, RegistryVersion, ReleaseMetadata,
};

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
                metadata.schema == REGISTRY_SCHEMA,
                "unsupported release metadata schema in {}",
                path.display()
            );
            releases.push(metadata);
        }
    }
    let index = assemble_index(releases)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&index)?)?;
    Ok(())
}

fn assemble_index(releases: Vec<ReleaseMetadata>) -> Result<RegistryIndex> {
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
            latest: latest.package.version.clone(),
            versions: versions.into_iter().map(to_registry_version).collect(),
        });
    }
    Ok(RegistryIndex {
        schema: REGISTRY_SCHEMA,
        packages,
    })
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
        artifact: release.artifact,
        source: release.source,
    }
}

#[cfg(test)]
mod tests {
    use semver::Version;

    use super::*;
    use crate::model::{PublishedArtifact, PublishedPackage, PublishedSource};

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
            },
            content: "rintawa.extension@1".into(),
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
        let index = assemble_index(vec![release("1.2.0"), release("1.10.0")])
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
        let index = assemble_index(vec![release("1.9.0"), release("2.0.0-alpha.1")])
            .expect("registry should assemble");
        assert_eq!(index.packages[0].latest, Version::parse("1.9.0").unwrap());
        assert_eq!(
            index.packages[0].versions[0].version,
            Version::parse("2.0.0-alpha.1").unwrap()
        );
    }

    #[test]
    fn test_should_reject_duplicate_package_versions() {
        let error = assemble_index(vec![release("1.0.0"), release("1.0.0")])
            .expect_err("duplicate versions must fail");
        assert!(error.to_string().contains("duplicate version"));
    }
}
