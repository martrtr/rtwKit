# Character Library

Character Library provides reusable character templates and an import flow for
Rintawa Worlds. It supports Tavern-compatible character data and keeps imported
templates in the persistent user-content library.

The package contains:

- a content handler for `rintawa.character-template@1`;
- a library interface for browsing and importing templates;
- a World creation provider that materializes a selected template into a new World;
- support for associating validated portrait assets with character identities.

The creation flow requires the Character Library artifact to be selected in the
`default-world` composition before a World is created. This ensures that each
created World retains the exact extension version used for its character data.

JSON import and persistent World creation are supported. Direct PNG card
selection through the UI requires additional file-picker integration.
