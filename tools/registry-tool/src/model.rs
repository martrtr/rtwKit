//! Registry-tool data model and serialized repository schema definitions.

use semver::Version;
use serde::{Deserialize, Serialize};

pub const RTWKIT_MANIFEST_SCHEMA: u32 = 1;
pub const LEGACY_REGISTRY_SCHEMA: u32 = 1;
pub const DEPENDENCY_REGISTRY_SCHEMA: u32 = 2;
pub const REGISTRY_SCHEMA: u32 = 3;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub schema: u32,
    pub package: PackageMetadata,
    pub build: BuildMetadata,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMetadata {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub description: String,
    pub license: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default, rename = "source-url")]
    pub source_url: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub readme: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<PackageDependency>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackageDependency {
    pub id: String,
    pub requirement: semver::VersionReq,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct BuildMetadata {
    pub artifact_root: String,
    #[serde(default)]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleaseMetadata {
    pub schema: u32,
    pub package: PublishedPackage,
    pub content: String,
    #[serde(default)]
    pub dependencies: Vec<PackageDependency>,
    pub artifact: PublishedArtifact,
    pub source: PublishedSource,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PublishedPackage {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: Version,
    pub description: String,
    pub license: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub logo: Option<PublishedAsset>,
    #[serde(default)]
    pub readme: Option<PublishedAsset>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PublishedArtifact {
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PublishedAsset {
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PublishedSource {
    pub repository: String,
    pub tag: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryIndex {
    pub schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<PublishedRepository>,
    pub packages: Vec<RegistryPackage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryManifest {
    pub schema: u32,
    pub repository: RepositoryMetadata,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RepositoryMetadata {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub homepage: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PublishedRepository {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub icon: Option<PublishedAsset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryPackage {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub license: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub logo: Option<PublishedAsset>,
    #[serde(default)]
    pub readme: Option<PublishedAsset>,
    pub latest: Version,
    pub versions: Vec<RegistryVersion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryVersion {
    pub version: Version,
    pub content: String,
    #[serde(default)]
    pub dependencies: Vec<PackageDependency>,
    pub artifact: PublishedArtifact,
    pub source: PublishedSource,
}
