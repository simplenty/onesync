# OneSync Gap Tasks

This document tracks the main gaps between the current Rust GTK implementation in `src/main.rs` and the reference `OneDriveGUI` project in `OneDriveGUI/`.

## Current Baseline

- The current app is a single-file GTK/libadwaita MVP in `src/main.rs`.
- Implemented today:
  - account list and selected account state
  - account creation with a minimal `sync_dir` config
  - authentication via `onedrive --auth-files`
  - one-time sync via `onedrive --sync --verbose`
  - monitor mode via `onedrive --monitor --verbose`
  - basic transfer-output parsing and progress rows
  - JSON account store under the OneSync config directory
- The reference GUI is a fuller profile manager around the abraunegg `onedrive` CLI, with profile import, config editing, tray behavior, SharePoint helpers, version checks, and richer error handling.

## P0: Make Daily Sync Safe

- [ ] Refactor the current single-file implementation into a normal Rust project structure.
  - Keep `src/main.rs` as a thin application entry point only.
  - Move application bootstrap and high-level wiring into `src/app.rs`.
  - Move account models and JSON persistence into `src/account.rs`.
  - Move OneDrive process spawning, monitor handles, and backend events into `src/onedrive.rs`.
  - Move transfer output parsing into `src/transfer_parser.rs` with its existing tests.
  - Move OneDrive config read/write helpers into `src/config.rs`.
  - Move GTK/libadwaita widget construction into `src/ui/` modules.
  - Ensure this refactor is behavior-preserving before adding larger features.

- [ ] Add `onedrive` client discovery and version checks before enabling sync actions.
  - Detect missing binary.
  - Run `onedrive --version`.
  - Enforce a minimum supported version.
  - Surface actionable UI errors instead of only printing to stderr.

- [ ] Add import of existing OneDrive profiles/config files.
  - Let the user choose an existing `config` file.
  - Derive or select the config directory.
  - Read existing `sync_dir`.
  - Preserve existing tokens and profile files.
  - Reject duplicate profile names and duplicate sync directories.

- [ ] Replace the minimal config writer with a real OneDrive config parser/writer.
  - Read config files without native INI section headers.
  - Support multi-line `skip_file` and `skip_dir`.
  - Preserve or intentionally normalize supported keys.
  - Avoid writing default/empty options that make the CLI reject config.
  - Back up config before saving.

- [ ] Add profile edit UI.
  - Edit `sync_dir`.
  - Edit skip files and skip dirs.
  - Edit sync list file.
  - Save/discard changes.
  - Warn about unsaved changes.

- [ ] Add profile lifecycle actions.
  - Rename profile.
  - Remove profile.
  - Logout profile with `onedrive --logout`.
  - Stop running monitor before destructive profile actions.

- [ ] Improve error parsing from `onedrive` output.
  - Expired or missing `refresh_token`.
  - Network connection failures.
  - Unknown config keys.
  - Failed upload/download item list.
  - CLI crash messages.
  - Authentication failures.

- [ ] Add explicit confirmation flows for dangerous/special CLI states.
  - `--resync is required`.
  - Big delete authorization.
  - Download-only cleanup warnings.
  - Upload-only/no-remote-delete compatibility.

## P1: Match Reference GUI Core Features

- [ ] Implement a real settings window.
  - General GUI settings.
  - Start minimized setting.
  - Frameless window setting, if still wanted.
  - OneDrive binary path override.

- [ ] Add auto-sync support per profile.
  - Persist `auto_sync`.
  - Start monitor automatically after app launch.
  - Make startup failures visible per profile.

- [ ] Add system tray/background behavior.
  - Minimize to tray on close when available.
  - Tray menu: show/hide, settings, quit.
  - Aggregate tray state across profiles: stopped, syncing, idle, error.
  - Gracefully stop monitors only when the user chooses to quit.

- [ ] Add richer profile status.
  - Account type.
  - Remaining free space.
  - Current status message from CLI.
  - Last error tooltip/details.
  - Running/stopped state independent of authentication state.

- [ ] Improve transfer history.
  - Completion timestamps.
  - Relative time display.
  - Failed transfer details.
  - File name/path eliding.
  - Distinguish upload, download, delete, move, rename, modified file.

- [ ] Add logging controls.
  - Enable/disable logging config.
  - Log directory picker.
  - Debug HTTPS option.
  - Monitor log frequency.

## P2: Advanced OneDrive Capabilities

- [ ] Add SharePoint Shared Library workflow.
  - Select an authenticated Business profile.
  - Run `--get-sharepoint-drive-id 'non-existent-library'` to list sites.
  - Run `--get-sharepoint-drive-id '<site>'` to list libraries and drive IDs.
  - Create a new profile with `drive_id`.

- [ ] Add Business shared item support.
  - List shared items with `--list-shared-items`.
  - Configure supported shared item sync options.
  - Surface deprecation warnings where applicable.

- [ ] Add advanced sync options.
  - `download_only`.
  - `upload_only`.
  - `local_first`.
  - `no_remote_delete`.
  - `dry_run`.
  - `rate_limit`.
  - `threads`.
  - timeout settings.
  - inotify delay and full scan frequency.

- [ ] Add webhook options.
  - Enable/disable webhooks.
  - Listening host and port.
  - Public URL.
  - Expiration and renewal intervals.

- [ ] Add national cloud and tenant options.
  - Azure AD endpoint.
  - Azure tenant ID.
  - Drive ID.
  - User agent.

- [ ] Add recycle bin options.
  - Enable recycle bin.
  - Pick recycle bin path.
  - Validate path before saving.

## P3: Rust Project Quality

- [ ] Move business logic out of GTK callbacks.
  - Keep UI callbacks thin.
  - Return structured errors from account/config/process functions.
  - Convert process output to typed events before UI handling.

- [ ] Strengthen error handling.
  - Replace broad `eprintln!` paths with user-visible errors and logged details.
  - Use custom error types where useful.
  - Avoid treating process exit failure as a generic sync failure when a known remediation exists.

- [ ] Improve process management.
  - Avoid orphaned child processes.
  - Track per-profile active operation.
  - Prevent overlapping one-time sync, monitor, logout, and config writes.
  - Add graceful termination before kill where possible.

- [ ] Add tests around high-risk logic.
  - Config parse/write round trips.
  - Existing profile import.
  - Transfer parser coverage for real CLI output.
  - Error parser coverage.
  - Account store migration/compatibility.

- [ ] Add packaging and desktop integration.
  - Desktop file.
  - App icon.
  - AppStream metadata, if distributing on Linux.
  - Release build profile.

## Suggested Implementation Order

1. Extract `transfer_parser`, `account_store`, and `onedrive_process` modules without changing behavior.
2. Add version/binary checks and better process error events.
3. Add existing profile import and real config parser/writer.
4. Add profile settings window for the most important options: `sync_dir`, skip lists, sync list, auto sync, logout.
5. Add remove/rename profile actions.
6. Add resync and big delete confirmation flows.
7. Add tray/background behavior and auto-sync.
8. Add SharePoint and advanced configuration pages.
