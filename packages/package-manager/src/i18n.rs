//! English fallback copy and stable message identities for Package Manager UI.
//!
//! Portable UI carries already-localized display text. Feature behavior must use
//! stable action, row, and column identities instead of comparing translated text.
//! A future locale provider can replace this fallback catalog without changing UI
//! contracts or Package Manager state transitions.

/// Stable message identity owned by the Package Manager feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Message {
    Extensions,
    DownloadExtensions,
    CheckForUpdates,
    AddFile,
    AddFromUrl,
    Repositories,
    ManageRepositories,
    Close,
    Refresh,
    GeneralActions,
    SelectedExtension,
    Details,
    Enable,
    Disable,
    Remove,
    ChangeVersion,
    RuntimePermissions,
    SearchInstalled,
    SearchCatalog,
    ColumnEnable,
    ColumnImage,
    ColumnName,
    ColumnVersion,
    ColumnProvider,
    ColumnStatus,
    StatusEnabled,
    StatusDisabled,
    StatusUpdateAvailable,
    StatusPublishedBuildDiffers,
    StatusRequiredDependency,
    StatusDependencyMetadataUnknown,
    LocalBaseline,
    LocalUnpublishedBuild,
    All,
    NotInstalled,
    Installed,
    UpdatesAvailable,
    SortName,
    SortVersion,
    SortRepository,
    NoExtensionsSelected,
    InstallSelected,
    ClearSelection,
    AddRepository,
    InstallFromUrl,
    DownloadAndInstall,
    InstallationReview,
    ConfirmInstall,
    Cancel,
}

/// Resolves one Package Manager message using the built-in English fallback.
///
/// Keeping lookup behind this function ensures display copy can later depend on a
/// generic host locale without changing portable UI node identities.
pub(crate) fn message(id: Message) -> &'static str {
    match id {
        Message::Extensions => "Extensions",
        Message::DownloadExtensions => "Download Extensions",
        Message::CheckForUpdates => "Check for Updates",
        Message::AddFile => "Add File",
        Message::AddFromUrl => "Add from URL",
        Message::Repositories => "Repositories",
        Message::ManageRepositories => "Manage repositories",
        Message::Close => "Close",
        Message::Refresh => "Refresh",
        Message::GeneralActions => "General",
        Message::SelectedExtension => "Selected extension",
        Message::Details => "Details",
        Message::Enable => "Enable",
        Message::Disable => "Disable",
        Message::Remove => "Remove",
        Message::ChangeVersion => "Change Version",
        Message::RuntimePermissions => "Runtime permissions",
        Message::SearchInstalled => "Search installed extensions",
        Message::SearchCatalog => "Search extensions by name, id, author, or description",
        Message::ColumnEnable => "Enable",
        Message::ColumnImage => "Image",
        Message::ColumnName => "Name",
        Message::ColumnVersion => "Version",
        Message::ColumnProvider => "Provider",
        Message::ColumnStatus => "Status",
        Message::StatusEnabled => "Enabled",
        Message::StatusDisabled => "Disabled",
        Message::StatusUpdateAvailable => "Update available",
        Message::StatusPublishedBuildDiffers => "Published build differs",
        Message::StatusRequiredDependency => "Required dependency",
        Message::StatusDependencyMetadataUnknown => "Dependency metadata unknown",
        Message::LocalBaseline => "Local / baseline",
        Message::LocalUnpublishedBuild => "Local / unpublished build",
        Message::All => "All",
        Message::NotInstalled => "Not installed",
        Message::Installed => "Installed",
        Message::UpdatesAvailable => "Updates available",
        Message::SortName => "Sort: Name",
        Message::SortVersion => "Sort: Version",
        Message::SortRepository => "Sort: Repository",
        Message::NoExtensionsSelected => "No extensions selected",
        Message::InstallSelected => "Install selected",
        Message::ClearSelection => "Clear selection",
        Message::AddRepository => "Add repository",
        Message::InstallFromUrl => "Install from URL",
        Message::DownloadAndInstall => "Download and install",
        Message::InstallationReview => "Installation review",
        Message::ConfirmInstall => "Confirm install",
        Message::Cancel => "Cancel",
    }
}

/// Formats the action used to update an installed package to a newer release.
pub(crate) fn update_to(version: &str) -> String {
    format!("Update to {version}")
}

/// Formats the action used to select an older published package release.
pub(crate) fn switch_to(version: &str) -> String {
    format!("Switch to {version}")
}

/// Formats the action used to install one selected package release.
pub(crate) fn install_version(version: &str) -> String {
    format!("Install {version}")
}

/// Formats the disabled action shown for an already-installed exact release.
pub(crate) fn installed_release(version: &str) -> String {
    format!("Installed {version}")
}

/// Formats the action used when the semver matches but the installed artifact digest does not.
pub(crate) fn replace_with_published_build(version: &str) -> String {
    format!("Replace with published {version}")
}
