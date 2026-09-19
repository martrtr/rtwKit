//! RTW package assembly and release artifact validation for registry publishing.

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    str,
};

use anyhow::{Context, Result, bail, ensure};
use rintawa_artifacts::{RtwArchive, RtwLimits, pack_directory};
use sha2::{Digest, Sha256};

use crate::model::{
    PackageManifest, PublishedArtifact, PublishedAsset, PublishedPackage, PublishedSource,
    REGISTRY_SCHEMA, RTWKIT_MANIFEST_SCHEMA, ReleaseMetadata,
};

const PACKAGES_DIRECTORY: &str = "packages";
const MANIFEST_FILE: &str = "rtwkit.toml";
const MAX_LOGO_BYTES: u64 = 512 * 1024;
const MAX_README_BYTES: u64 = 1024 * 1024;

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
    let logo = package
        .manifest
        .package
        .logo
        .as_deref()
        .map(|path| {
            publish_logo(
                &package,
                path,
                output_directory,
                release_repository,
                release_tag,
            )
        })
        .transpose()?;
    let readme = package
        .manifest
        .package
        .readme
        .as_deref()
        .map(|path| {
            publish_readme(
                &package,
                path,
                output_directory,
                release_repository,
                release_tag,
            )
        })
        .transpose()?;
    let metadata = ReleaseMetadata {
        schema: REGISTRY_SCHEMA,
        package: PublishedPackage {
            id: package.manifest.package.id.clone(),
            slug: slug.to_string(),
            name: package.manifest.package.name.clone(),
            version: package.manifest.package.version.clone(),
            description: package.manifest.package.description.clone(),
            license: package.manifest.package.license.clone(),
            authors: package.manifest.package.authors.clone(),
            homepage: package.manifest.package.homepage.clone(),
            source_url: package.manifest.package.source_url.clone(),
            logo,
            readme,
        },
        content,
        dependencies: package.manifest.package.dependencies.clone(),
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

fn publish_logo(
    package: &LoadedPackage,
    relative: &str,
    output_directory: &Path,
    release_repository: &str,
    release_tag: &str,
) -> Result<PublishedAsset> {
    validate_relative_path(relative)?;
    let source = package.directory.join(relative);
    let metadata = fs::symlink_metadata(&source)
        .with_context(|| format!("failed to inspect logo {}", source.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "package logo must be a regular file"
    );
    ensure!(
        metadata.len() > 0 && metadata.len() <= MAX_LOGO_BYTES,
        "package logo must be between 1 and {MAX_LOGO_BYTES} bytes"
    );

    let bytes = fs::read(&source)?;
    let (extension, media_type) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("png", "image/png")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        ("webp", "image/webp")
    } else {
        bail!("package logo must be PNG or WebP");
    };

    publish_asset_bytes(
        output_directory,
        format!("{}.logo.{extension}", package.slug),
        &bytes,
        media_type,
        release_repository,
        release_tag,
    )
}

fn publish_readme(
    package: &LoadedPackage,
    relative: &str,
    output_directory: &Path,
    release_repository: &str,
    release_tag: &str,
) -> Result<PublishedAsset> {
    validate_relative_path(relative)?;
    let source = package.directory.join(relative);
    let metadata = fs::symlink_metadata(&source)
        .with_context(|| format!("failed to inspect README {}", source.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "package README must be a regular file"
    );
    ensure!(
        metadata.len() > 0 && metadata.len() <= MAX_README_BYTES,
        "package README must be between 1 and {MAX_README_BYTES} bytes"
    );

    let bytes = fs::read(&source)?;
    str::from_utf8(&bytes).context("package README must be UTF-8")?;
    publish_asset_bytes(
        output_directory,
        format!("{}.README.md", package.slug),
        &bytes,
        "text/markdown; charset=utf-8",
        release_repository,
        release_tag,
    )
}

fn publish_asset_bytes(
    output_directory: &Path,
    file_name: String,
    bytes: &[u8],
    media_type: &str,
    release_repository: &str,
    release_tag: &str,
) -> Result<PublishedAsset> {
    let output = output_directory.join(&file_name);
    fs::write(&output, bytes)?;
    let sha256 = format!("sha256:{:x}", Sha256::digest(bytes));
    let size = u64::try_from(bytes.len()).context("asset size does not fit into u64")?;
    let url = format!(
        "https://github.com/{release_repository}/releases/download/{release_tag}/{file_name}"
    );
    Ok(PublishedAsset {
        file: file_name,
        url,
        sha256,
        size,
        media_type: media_type.to_string(),
    })
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
    let mut dependencies = HashSet::new();
    for dependency in &manifest.package.dependencies {
        validate_package_id(&dependency.id)?;
        ensure!(
            dependency.id != manifest.package.id,
            "package cannot depend on itself"
        );
        ensure!(
            dependencies.insert(dependency.id.clone()),
            "duplicate dependency '{}'",
            dependency.id
        );
    }
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
    ensure!(
        manifest.package.authors.len() <= 32
            && manifest
                .package
                .authors
                .iter()
                .all(|author| !author.trim().is_empty() && author.len() <= 128),
        "package authors must contain at most 32 non-empty values up to 128 bytes"
    );
    if let Some(url) = manifest.package.homepage.as_deref() {
        validate_https_metadata_url(url, "homepage")?;
    }
    if let Some(url) = manifest.package.source_url.as_deref() {
        validate_https_metadata_url(url, "source-url")?;
    }
    if let Some(path) = manifest.package.logo.as_deref() {
        validate_relative_path(path)?;
    }
    if let Some(path) = manifest.package.readme.as_deref() {
        validate_relative_path(path)?;
    }
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

fn validate_https_metadata_url(value: &str, label: &str) -> Result<()> {
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
