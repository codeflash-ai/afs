# AFS Desktop

Tauri desktop shell for AFS onboarding, workspace controls, pending-change
review, and settings.

## Development

```sh
npm install
npm run dev
```

Open the Vite preview at `http://127.0.0.1:1420/`.

Useful preview routes:

- `http://127.0.0.1:1420/` starts at first-run onboarding.
- `http://127.0.0.1:1420/#app` starts at the main app shell.

The Rust side is under `src-tauri` and can be checked from the repo root:

```sh
cargo check -p afs-desktop
```

## Current Scope

This app implements the first desktop UI pass from `docs/desktop-app.md` and
`docs/desktop-ui-screens.md` using typed command stubs. Real daemon, CLI, OAuth,
mount, locate, push, open-folder, and tray-popover wiring is tracked in
`docs/deviations.md`.
