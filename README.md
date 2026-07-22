# App Fair

An open, privacy-respecting app store for F-Droid–compatible catalogs, built with
[Day](https://daybrite.dev) — one Rust codebase, native widgets on every platform.

App Fair syncs F-Droid Index V2 catalogs into on-device SQLite, then lets you browse, search,
and switch between catalogs and install, update, and launch apps. It talks only to the catalogs
you add: no accounts, no analytics, and nothing about what you browse or install leaves the device.

## What's inside

`root()` (`src/lib.rs`) is a three-tab shell — **Catalogs**, **Updates**, **Settings**. Catalogs
and Updates are each a navigation stack that pushes an app detail page; an install overlay floats
above everything.

- `src/ui.rs` — the Catalogs tab: catalog switcher, search, category/sort pickers, the app list.
- `src/detail.rs` — an app's detail page (icon, description, permissions, signing key, Install).
- `src/install.rs` / `src/install_sheet.rs` — download-verify-install orchestration and its sheet.
- `src/updates.rs` — the Updates tab and background update checks.
- `src/settings.rs` / `src/catalogs_add.rs` — catalogs, preferences, anti-feature filters, add-catalog.
- `src/db.rs` / `src/schema.rs` / `src/model.rs` — the on-device catalog (Diesel + bundled SQLite).
- `src/fdroid.rs` / `src/net.rs` / `src/sync.rs` — F-Droid Index V2 parsing, HTTP, and catalog sync.
- `src/platform.rs` + `android/java/.../DayInstaller.java` — the Android install/query bridge.
- `resource/locales/{en,fr}/app.ftl` — every user-facing string ([localization](https://daybrite.dev/docs/localization)).
- `mock/` — a bundled offline catalog for deterministic screenshots (`--env APP_FAIR_MOCK=1`).
- `Day.toml` / `Cargo.toml` — app metadata, targets, signing, and dependencies.

## Build and run

Day compiles one backend per binary, so choose a target. The Day CLI supplies the right feature:

```sh
day doctor                        # check the toolchains for your targets
day launch -p android-widget      # build + run on a device/emulator
day launch -p android-widget --script dayscript/ci.yaml --env APP_FAIR_MOCK=1   # scripted, offline
```

## Release

Signing keys are referenced from `Day.toml` (`[signing.android]`) via environment variables:

```sh
export DAY_ANDROID_KEYSTORE=/path/to/keystore.jks DAY_ANDROID_KEY_ALIAS=app \
       DAY_KS_PASS=… DAY_KEY_PASS=…
day sign --check                               # confirm the config resolves
day pack -p android-widget --profile release   # signed app-fair-<version>.apk + .aab in build/day/dist/
```

Play Store listing metadata and the `supply` dry run live in [`android/fastlane/`](android/fastlane).
See [`RELEASE.md`](RELEASE.md) for the full release checklist and the Google Play policy review notes.
