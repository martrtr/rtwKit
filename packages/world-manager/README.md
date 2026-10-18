# World Manager

World Manager is the standard interface for browsing and managing Rintawa Worlds.
It lists stored Worlds, initiates World creation and controls activation and focus
through the Host's world-session capabilities.

The package renders its interface through Portable UI and requests
`world-session-read` and `world-session-write` permissions. Its runtime component
uses the public `world-sessions` and `portable-ui` contracts.
