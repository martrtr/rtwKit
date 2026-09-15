use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use rintawa_artifacts::{RtwArchive, RtwLimits, pack_directory};
use sha2::{Digest, Sha256};

use crate::model::{
    PackageManifest, PublishedArtifact, PublishedPackage, PublishedSource, REGISTRY_SCHEMA,
    RTWKIT_MANIFEST_SCHEMA, ReleaseMetadata,
};

const PACKAGES_DIRECTORY: &str = "packages";
const MANIFEST_FILE: &str = "rtwkit.toml";

pub struct LoadedPackage {
    pub slug: String,
    pub directory: PathBuf,
    pub manifest: PackageManifest,
}

pub fn load_package(repository: &Path, slug: &str) -> Result<LoadedPackage> {
    validate_slug(slug)?;
    let directory = repository.join(PACKAGES_DIRECTORY).join(slug);
    ensure!(
        directory.is_dir(),
        "package directory '{}' does not exist",
        directory.display()
    );
    let manifest_path = directory.join(MANIFEST_FILE);
    let source = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: PackageManifest = toml::from_str(&source)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    validate_manifest(&manifest, slug)?;
    Ok(LoadedPackage {
        slug: slug.to_string(),
        directory,
        manifest,
    })
}

pub fn validate_repository(repository: &Path) -> Result<()> {
    let packages_root = repository.join(PACKAGES_DIRECTORY);
    fs::create_dir_all(&packages_root)?;
    let mut ids = HashSet::new();
    for entry in fs::read_dir(packages_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let slug = entry.file_name().to_string_lossy().into_owned();
        let package = load_package(repository, &slug)?;
        ensure!(
            ids.insert(package.manifest.package.id.clone()),
            "duplicate package id '{}'",
            package.manifest.package.id
        );
    }
    Ok(())
}

pub fn build_package(
    repository: &Path,
    slug: &str,
    output_directory: &Path,
    release_repository: &str,
    release_tag: &str,
    source_commit: &str,
) -> Result<(PathBuf, PathBuf)> {
    let package = load_package(repository, slug)?;
    validate_release_tag(
        slug,
        &package.manifest.package.version.to_string(),
        release_tag,
    )?;
    run_build(&package)?;

    let artifact_root = package
        .directory
        .join(&package.manifest.build.artifact_root);
    ensure!(
        artifact_root.is_dir(),
        "artifact root '{}' does not exist",
        artifact_root.display()
    );
    fs::create_dir_all(output_directory)?;

    let artifact_name = format!("{slug}.rtw");
    let artifact_path = output_directory.join(&artifact_name);
    if artifact_path.exists() {
        fs::remove_file(&artifact_path)?;
    }
    pack_directory(&artifact_root, &artifact_path, RtwLimits::default())?;

    let archive = RtwArchive::open(&artifact_path, RtwLimits::default())?;
    let content = archive.manifest().content.to_string();
    drop(archive);

    let bytes = fs::read(&artifact_path)?;
    let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
    let size = u64::try_from(bytes.len()).context("artifact size does not fit into u64")?;
    let url = format!(
        "https://github.com/{release_repository}/releases/download/{release_tag}/{artifact_name}"
    );
    let metadata = ReleaseMetadata {
        schema: REGISTRY_SCHEMA,
        package: PublishedPackage {
            id: package.manifest.package.id.clone(),
            slug: slug.to_string(),
            name: package.manifest.package.name.clone(),
            version: package.manifest.package.version.clone(),
            description: package.manifest.package.description.clone(),
            license: package.manifest.package.license.clone(),
        },
        content,
        artifact: PublishedArtifact {
            file: artifact_name,
            url,
            sha256,
            size,
        },
        source: PublishedSource {
            repository: release_repository.to_string(),
            tag: release_tag.to_string(),
            commit: source_commit.to_string(),
        },
    };

    let metadata_name = format!("{slug}.registry.json");
    let metadata_path = output_directory.join(metadata_name);
    fs::write(&metadata_path, serde_json::to_vec_pretty(&metadata)?)?;
    Ok((artifact_path, metadata_path))
}

fn run_build(package: &LoadedPackage) -> Result<()> {
    if package.manifest.build.command.is_empty() {
        return Ok(());
    }
    let (program, arguments) = package
        .manifest
        .build
        .command
        .split_first()
        .context("build command cannot be empty")?;
    let status = Command::new(program)
        .args(arguments)
        .current_dir(&package.directory)
        .status()
        .with_context(|| format!("failed to start build command for '{}'", package.slug))?;
    ensure!(
        status.success(),
        "build command for '{}' failed with {status}",
        package.slug
    );
    Ok(())
}

fn validate_manifest(manifest: &PackageManifest, slug: &str) -> Result<()> {
    ensure!(
        manifest.schema == RTWKIT_MANIFEST_SCHEMA,
        "unsupported rtwkit.toml schema {}",
        manifest.schema
    );
    validate_package_id(&manifest.package.id)?;
    ensure!(
        !manifest.package.name.trim().is_empty(),
        "package name cannot be empty"
    );
    ensure!(
        !manifest.package.description.trim().is_empty(),
        "package description cannot be empty"
    );
    ensure!(
        !manifest.package.license.trim().is_empty(),
        "package license cannot be empty"
    );
    validate_relative_path(&manifest.build.artifact_root)?;
    if let Some(program) = manifest.build.command.first() {
        ensure!(
            !program.trim().is_empty(),
            "build command program cannot be empty"
        );
    }
    validate_slug(slug)
}

fn validate_slug(slug: &str) -> Result<()> {
    ensure!(!slug.is_empty(), "package slug cannot be empty");
    ensure!(slug.len() <= 64, "package slug cannot exceed 64 bytes");
    ensure!(
        slug.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
        "package slug must use [a-z0-9-]"
    );
    ensure!(
        !slug.starts_with('-') && !slug.ends_with('-'),
        "package slug cannot start or end with '-'"
    );
    Ok(())
}

fn validate_package_id(id: &str) -> Result<()> {
    let segments: Vec<_> = id.split('.').collect();
    ensure!(
        segments.len() >= 2,
        "package id must be namespaced with at least one '.'"
    );
    for segment in segments {
        ensure!(
            !segment.is_empty(),
            "package id contains an empty namespace segment"
        );
        ensure!(
            segment.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'-'
                || byte == b'_'),
            "package id must use [a-z0-9_-] segments"
        );
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    ensure!(!value.is_empty(), "artifact-root cannot be empty");
    ensure!(!path.is_absolute(), "artifact-root must be relative");
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("artifact-root must not contain traversal or platform prefixes"),
        }
    }
    Ok(())
}

pub fn validate_release_tag(slug: &str, version: &str, tag: &str) -> Result<()> {
    let expected = format!("pkg-{slug}-v{version}");
    ensure!(
        tag == expected,
        "release tag '{tag}' does not match expected '{expected}'"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use semver::Version;
    use tempfile::TempDir;

    use super::*;
    use crate::model::ReleaseMetadata;

    #[test]
    fn test_should_build_release_from_static_rtw_root() -> Result<()> {
        let root = TempDir::new()?;
        let package = root.path().join("packages/example");
        let artifact_root = package.join("build/rtw");
        fs::create_dir_all(&artifact_root)?;
        fs::write(
            package.join("rtwkit.toml"),
            concat!(
                "schema = 1\n",
                "[package]\n",
                "id = \"rintawa.example\"\n",
                "name = \"Example\"\n",
                "version = \"0.1.0\"\n",
                "description = \"Example package\"\n",
                "license = \"GPL-3.0-only\"\n",
                "[build]\n",
                "artifact-root = \"build/rtw\"\n"
            ),
        )?;
        fs::write(
            artifact_root.join("rtw.toml"),
            "format = 1\ncontent = \"rintawa.extension@1\"\nentry = \"manifest.toml\"\n",
        )?;
        fs::write(artifact_root.join("manifest.toml"), "id = \"example\"\n")?;

        let output = root.path().join("dist");
        let (artifact, metadata_path) = build_package(
            root.path(),
            "example",
            &output,
            "martrtr/rtwKit",
            "pkg-example-v0.1.0",
            "abc123",
        )?;
        assert!(artifact.exists());
        assert_eq!(
            artifact.file_name().and_then(|name| name.to_str()),
            Some("example.rtw")
        );
        assert_eq!(
            metadata_path.file_name().and_then(|name| name.to_str()),
            Some("example.registry.json")
        );
        let metadata: ReleaseMetadata = serde_json::from_slice(&fs::read(metadata_path)?)?;
        assert_eq!(metadata.package.version, Version::parse("0.1.0")?);
        assert_eq!(metadata.content, "rintawa.extension@1");
        assert_eq!(metadata.artifact.file, "example.rtw");
        assert_eq!(metadata.source.commit, "abc123");
        assert_eq!(
            metadata.artifact.url,
            "https://github.com/martrtr/rtwKit/releases/download/pkg-example-v0.1.0/example.rtw"
        );
        assert!(metadata.artifact.sha256.starts_with("sha256:"));
        Ok(())
    }
}
