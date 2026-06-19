# Linux Distribution

AFS ships on Linux as Tauri-generated `.deb` and `.rpm` packages. The Linux
packages do not need signing, notarization, or stapling, but they do need the
same runtime sidecars that the macOS app bundle carries: the `afs` CLI, the
`afsd` daemon, and the `afs-fuse` projection helper.

## Local Package Build

Build, validate, rename, and checksum both Linux artifacts:

```sh
make publish-linux
```

The Tauri pre-bundle hook runs:

```sh
apps/desktop/scripts/prepare-bundle.sh
```

On Linux that dispatches to `apps/desktop/scripts/prepare-linux-bundle.sh`,
which builds `afs`, `afsd`, and `afs-fuse` in release mode and stages them under
`apps/desktop/src-tauri/linux/`. Tauri includes those staged binaries in both
Linux package formats at:

```text
/usr/bin/afs
/usr/bin/afsd
/usr/bin/afs-fuse
```

Expected local artifacts:

```text
target/release/bundle/deb/*.deb
target/release/bundle/rpm/*.rpm
```

The publish script requires a clean git working tree by default because the
published filename includes the `HEAD` commit. Use `PUBLISH_ALLOW_DIRTY=1` only
for local throwaway builds.

Final artifacts are copied to:

```text
target/release/bundle/linux/AFS-beta-YYYYMMDD-<commit>-<arch>.deb
target/release/bundle/linux/AFS-beta-YYYYMMDD-<commit>-<arch>.deb.sha256
target/release/bundle/linux/AFS-beta-YYYYMMDD-<commit>-<arch>.rpm
target/release/bundle/linux/AFS-beta-YYYYMMDD-<commit>-<arch>.rpm.sha256
target/release/bundle/linux/AFS-beta-linux-<arch>.deb
target/release/bundle/linux/AFS-beta-linux-<arch>.deb.sha256
target/release/bundle/linux/AFS-beta-linux-<arch>.rpm
target/release/bundle/linux/AFS-beta-linux-<arch>.rpm.sha256
```

Useful overrides:

```sh
PUBLISH_CHANNEL=release make publish-linux
PUBLISH_DATE=20260617 make publish-linux
```

## Runtime Requirements

The package metadata declares `fuse3` and `systemd` dependencies. AFS needs
`fusermount3` and `/dev/fuse` for Linux FUSE mounts, and it uses `systemctl
--user` to manage one per-mount FUSE service.

The desktop tray requires either `libayatana-appindicator3` or
`libappindicator3`. Tauri detects that library through pkg-config during
bundling. When a distro provides the runtime library but omits the pkg-config
metadata from the installed package set, `scripts/publish-linux.sh` creates
temporary pkg-config metadata from `ldconfig` so the package build can continue.

Linux package validation checks that both packages contain:

```text
/usr/bin/afs
/usr/bin/afsd
/usr/bin/afs-fuse
```

The existing FUSE smoke test remains the runtime check for actual mount
behavior:

```sh
AFS_FUSE_SMOKE=1 AFS_FUSE_SMOKE_REQUIRED=1 make test-linux-fuse
```

## GitHub Release Workflow

The GitHub workflow in `.github/workflows/release-linux.yml` publishes Linux
packages from a `v*` tag or manual workflow dispatch. It runs on
`ubuntu-24.04`, installs the GTK/WebKit/FUSE/AppIndicator packaging
dependencies, runs `make publish-linux`, and uploads the resulting `.deb`,
`.rpm`, per-artifact checksums, and `SHA256SUMS-linux` to the matching GitHub
Release.

The stable alias assets support latest-release install URLs, for example:

```sh
curl -L -o /tmp/afs.deb https://github.com/codeflash-ai/afs/releases/latest/download/AFS-release-linux-x86_64.deb && sudo apt install /tmp/afs.deb
```

The workflow shares the same release concurrency group as the macOS workflow so
both jobs can target one tag without racing while creating or updating the
GitHub Release.

Release a new Linux package by updating the app version, committing the change,
tagging that commit, and pushing the tag:

```sh
git tag v0.1.1
git push origin v0.1.1
```

The workflow requires the tag to match `apps/desktop/src-tauri/tauri.conf.json`
exactly. For example, version `0.1.1` must be released as `v0.1.1`.

GitHub Releases are the first Linux distribution channel. Apt/Yum repository
metadata, Snap, and Flatpak should be evaluated separately after the packaged
FUSE and per-user systemd behavior has been tested on the target distribution.
