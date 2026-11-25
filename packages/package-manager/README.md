# Packages

Optional Host-scoped local package manager, opened as Packages in the Home app bar. Core must support `artifact-store.list-rtw`; build Core and
this package from the current sources. No repository downloads or install scripts.

Build with `bash packages/package-manager/build.sh`. Initial bootstrap uses the
existing CLI: install the RTW, grant `background-task` to its `ui`, and grant its
backend the runtime permissions declared in `manifest.toml`, then restart Rintawa.
The UI's Allow access action uses the existing Root consent for its own backend;
Refresh rereads the result. The manager does not receive runtime-policy-write and
cannot approve another extension's permissions. An extension requiring new access
still needs the existing explicit Host/CLI permission setup before execution.

Import stores a validated RTW using disk-backed bounded transfer and does not select
or execute it. The archive list pages by opaque stable keys, preserving malformed entries as
error rows. Names/versions are bounded advisory metadata; selection and explicit
permission inspection verify exact bytes. Unknown content remains inert. Template archives remain available in Home's New World picker.

Choose a target in the left sidebar: Home, New World defaults or a saved World.
Installed shows a compact enabled/disabled list. Local library groups cached builds
by package; search and the version picker preserve every exact digest, including
same-version alternatives. The details panel provides Install/Switch version,
Enable/Disable and Remove. Install opens a review of exact bytes and permissions. Selection preserves the existing
instance on a version switch. Disable/enable/remove change saved configuration;
World code changes on its next start, Home code after Host restart. UI/backend
refuse changing a World currently observed active or pending. Native configuration
APIs retain their existing next-bootstrap semantics; this manual view does not
promise a transaction with another administrator's concurrent lifecycle operation.

Remove from scope retains archive bytes and World data. Missing owners/services can
remain unavailable until explicitly reconfigured. Physical cache deletion, automatic
version/dependency resolution, data migration, repository updates and custom-type
handlers are separate increments. A code downgrade is not a database rollback.
Mutations are never replayed automatically after an unconfirmed result; Refresh is
required before another decision.

Service `rintawa.packages@1` / `rintawa/packages/v1` accepts only its exact sibling
UI instance/component. Upload tokens additionally bind to that UI activation;
at most two handles are retained and abandoned handles expire after 120 seconds.
Only an admitted original transfer can append/status/finish/close. Nothing persists
in a second package catalog or a World database.

Real browser acceptance: `RINTAWA_BINARY=<core> RTWKIT_SMOKE_ARTIFACTS=<local-RTWs>
node scripts/package-manager-smoke.mjs`. The fixture covers independent scopes,
exact-version switching, instance preservation, caller denial, invalid imports and
retention after removal. It uses a disposable home and publishes nothing.

Catalog enumeration stops explicitly above 10,000 entries. It does not read whole
payloads, but a ZIP directory with very many entries can still take longer than
the current short request deadline. A catalog read failure leaves import available;
large/slow exact inspection or selection may require rereading an unconfirmed
outcome. No hard catalog/large-package latency guarantee is claimed by this slice.

The launcher-style layout follows Prism Launcher's mod-management interaction:
select a row, see its version/details, apply a contextual action. Drag RTW files to
import them sequentially; arrow keys move row selection. Narrow views use a target
selector and stacked details. World templates open the existing Root-owned chooser
with their digest selected. This action creates nothing and approves nothing until
the user submits that chooser. Packages keeps its own isolated view and privileges.
Reference: https://prismlauncher.org/wiki/help-pages/loader-mods/
