# 1claw-vault-explorer

A small desktop app for poking around your [1Claw](https://1claw.co) vaults. Written in Rust with egui, so it's a single native binary with no Electron and no browser tab.

## What it does

- **Vaults**: list, filter, create, and delete them. Right-click a vault to copy its ID.
- **Secrets**: shown as a folder tree built from their paths (`db/prod/password` ends up under `db › prod`). You can filter by path.
- **Secret details**: type, version, expiry and metadata. Values stay masked until you click **Reveal**.
  - **Copy** puts the value on the clipboard and wipes it again after 30 seconds.
  - **New version** writes an update. **History** lists older versions so you can view each one.
- **Policies**: each policy's path pattern, principal, permissions, allow/deny and expiry.
- **Activity**: the audit log, either for the current vault or the whole org, with a filter on action and actor.
- Light and dark theme. Press F5 to refresh.

## Running it

```sh
cargo run --release
```

Paste either a personal API key (`1ck_...`) or an agent key (`ocv_...`) and hit Connect. The key gets exchanged for a short-lived JWT. The JWT stays in memory and is refreshed when it expires.

You can also skip the form:

```sh
ONECLAW_API_KEY=1ck_xxx cargo run --release
```

Tick **Remember me** to stay signed in. The key then gets saved in the config file, which is `~/.config/vault-explorer/config.json` on Linux and the equivalent folder on other platforms. The same file holds the base URL, key type and theme. It's plain JSON, so on Linux/macOS the file is set to `0600` (only your user can read it). **Sign out** removes the saved key. If you leave the box unticked, the key is never written to disk.

The base URL is `https://api.1claw.co` unless you change it under *Advanced* (for example to `http://localhost:8443` for a local dev server).

### Linux deps

egui needs the usual X11/Wayland libs. On Debian/Ubuntu:

```sh
sudo apt install libxkbcommon-dev libgl1-mesa-dev libwayland-dev
```

## Layout

```
src/
  api.rs      blocking REST client (auth, vaults, secrets, versions, policies, audit)
  worker.rs   background thread so the ui never waits on the network
  app.rs      app state, login screen, top bar, vault sidebar
  views.rs    secrets / policies / activity tabs
  config.rs   saved settings (+ the key, if you tick remember me)
```

## Notes

- Every reveal or copy is a real read against the API, so it shows up in your audit log. That's on purpose.
- MPC 2-of-2 vaults need a client share to read values. This app doesn't handle that yet, so reads from those vaults will fail with an error from the API.

MIT licensed.
