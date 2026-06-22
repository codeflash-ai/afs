# Windows Distribution

AFS ships on Windows as a Tauri-generated NSIS installer. The Windows package
contains the desktop app plus the sidecars needed for the product runtime:
`afs.exe`, `afsd.exe`, and `afs-cloud-files.exe`.

## Local Package Build

Build a local NSIS installer from the repo root on Windows:

```powershell
make build-tauri-windows
```

For release-like local packaging, use:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/publish-windows.ps1
```

The Tauri pre-bundle hook runs:

```powershell
apps/desktop/scripts/prepare-windows-bundle.ps1
```

That script builds `afs-cli`, `afsd`, and `afs-cloud-files` in release mode,
stages the three sidecars under `apps/desktop/src-tauri/windows/`, and lets the
NSIS hook copy them next to the installed desktop executable.

Expected local artifacts:

```text
target/release/bundle/nsis/*.exe
target/release/bundle/windows/AFS-beta-windows-x86_64-setup.exe
target/release/bundle/windows/AFS-beta-windows-x86_64-setup.exe.sha256
```

The publish script requires a clean git working tree by default because the
published filename includes the `HEAD` commit. Use `PUBLISH_ALLOW_DIRTY=1` only
for local throwaway builds.

## Runtime Behavior

The Windows desktop app uses the same platform lifecycle layer as the CLI. When
a Windows Cloud Files mount is activated or opened, the app registers the sync
root if needed and supervises `afs-cloud-files.exe run` for that mount.

Installed files:

```text
AFS.exe
afs.exe
afsd.exe
afs-cloud-files.exe
```

The NSIS uninstall hook removes the installed sidecars, the per-user Windows
login item, and AFS-managed terminal command shims. It does not delete
user-visible mount folders.

## Code Signing

Release builds should Authenticode-sign the sidecars before NSIS packaging and
sign the final installer after packaging. Set:

```powershell
$env:AFS_WINDOWS_CODESIGN = "1"
$env:WINDOWS_CODESIGN_CERT_SHA1 = "<certificate-thumbprint>"
```

Optional:

```powershell
$env:WINDOWS_SIGNTOOL = "C:\Path\To\signtool.exe"
$env:WINDOWS_CODESIGN_TIMESTAMP_URL = "http://timestamp.digicert.com"
```

`WINDOWS_CODESIGN_CERT_SUBJECT` or `WINDOWS_CODESIGN_SUBJECT` can be used
instead of `WINDOWS_CODESIGN_CERT_SHA1` for local signing when the certificate
subject is unique. In CI, prefer the SHA-1 thumbprint from an imported `.pfx`.

Set `PUBLISH_REQUIRE_SIGNING=1` to fail the release build if no signing
certificate selector is available.

## Updater Artifacts

When `TAURI_UPDATER_PUBKEY` and `TAURI_SIGNING_PRIVATE_KEY` are set,
`scripts/publish-windows.ps1` asks Tauri to create signed updater artifacts and
copies the stable alias to:

```text
target/release/bundle/updater/AFS-release-windows-x86_64.nsis.zip
target/release/bundle/updater/AFS-release-windows-x86_64.nsis.zip.sig
```

Render the Windows updater manifest with:

```powershell
$env:UPDATER_MANIFEST_OUTPUT = "target/release/bundle/updater/latest-windows.json"
$env:UPDATER_WINDOWS_X86_64_ARTIFACT = "target/release/bundle/updater/AFS-release-windows-x86_64.nsis.zip"
bash scripts/render-tauri-updater-manifest.sh
```

## GitHub Release Workflow

The GitHub workflow in `.github/workflows/release-windows.yml` publishes the
Windows channel from a `v*` tag or manual workflow dispatch. It runs on
`windows-latest`, verifies the tag points at current `main`, verifies the tag
matches the Tauri app version, imports the Windows code-signing certificate,
builds the signed NSIS package, renders `latest-windows.json`, creates or
updates the GitHub Release, and uploads:

```text
AFS-release-windows-x86_64-setup.exe
AFS-release-windows-x86_64-setup.exe.sha256
AFS-release-windows-x86_64.nsis.zip
AFS-release-windows-x86_64.nsis.zip.sig
latest-windows.json
SHA256SUMS-windows
```

Required repository secrets:

- `WINDOWS_CODESIGN_CERT_PFX_BASE64`: base64-encoded Authenticode certificate
  exported as `.pfx`.
- `WINDOWS_CODESIGN_CERT_PASSWORD`: password for that `.pfx`.
- `TAURI_UPDATER_PUBKEY`: public updater signing key.
- `TAURI_SIGNING_PRIVATE_KEY`: private updater signing key.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: updater key password, if one was set.

Optional repository variable:

- `WINDOWS_CODESIGN_TIMESTAMP_URL`: timestamp server URL. Defaults to
  `http://timestamp.digicert.com`.

## Live E2E

The Windows live Notion test is `tests/windows_cloud_files_live.ps1`. It creates
a disposable Notion page, mounts it as `windows-cloud-files`, starts `afsd`,
starts the Cloud Files provider, runs `afs doctor --json` against the live
state, then verifies browse, hydrate, edit, push, create, rename, delete, and
archive behavior through the real mounted directory.

Run locally:

```powershell
$env:NOTION_TOKEN = "..."
$env:AFS_NOTION_LIVE_PARENT_PAGE = "..."
$env:AFS_WINDOWS_CLOUD_FILES_LIVE = "1"
pwsh ./tests/windows_cloud_files_live.ps1
```

GitHub Actions runs the same script from
`.github/workflows/notion-live-e2e.yml` on `windows-latest` when the
`notion-live-e2e` environment provides `NOTION_TOKEN` and
`AFS_NOTION_LIVE_PARENT_PAGE`.
