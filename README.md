# App Fair

An open, privacy-respecting app store for F-Droid–compatible catalogs, built with
[Day](https://daybrite.dev) — one Rust codebase, native widgets on every platform.

App Fair syncs F-Droid Index V2 catalogs into an on-device SQLite store, then lets you browse,
search, and switch between catalogs and install, update, and launch apps. It talks only to the
catalogs you add. There are no accounts or analytics, and what you browse or install stays on the
device.

## What's inside

`root()` (`src/lib.rs`) is a three-tab shell — **Catalogs**, **Updates**, **Settings**. Catalogs
and Updates are each a navigation stack that pushes an app detail page; an install overlay floats
above everything.

- `src/ui.rs` — the Catalogs tab: catalog switcher, search, category/sort pickers, the app list.
- `src/detail.rs` — an app's detail page (icon, description, permissions, signing key, Install).
- `src/install.rs` / `src/install_sheet.rs` — download-verify-install orchestration and its sheet.
- `src/updates.rs` — the Updates tab and background update checks.
- `src/settings.rs` / `src/catalogs_add.rs` — Settings pages (catalogs, update checks, anti-feature
  filters, the image cache, About) and the add-catalog page.
- `src/catalog.rs` — the on-device catalog: [day-persistence](https://github.com/daybrite/day-persistence)
  models over bundled SQLite, owned by one database worker so imports and searches stay off the UI
  thread. `src/model.rs` holds the values the worker hands to the pages.
- `src/fdroid.rs` / `src/net.rs` / `src/sync.rs` — F-Droid Index V2 parsing, HTTP, and catalog sync.
- `src/platform.rs` + `android/java/.../DayInstaller.java` — the Android install/query bridge.
- `resource/icons/icon.svg` — the app icon; `day prepare` renders every platform's icons from it.
- `resource/locales/{en,fr}/app.ftl` — every user-facing string ([localization](https://daybrite.dev/docs/localization)).
- `mock/` — a bundled offline catalog for deterministic screenshots (`--env APP_FAIR_MOCK=1`).
- `store/storefront.toml` — the App Store and Google Play listing, in English and French.
- `Day.toml` / `Cargo.toml` — app metadata, targets, permissions, signing, and dependencies.

## Build and run

`day launch --git` clones this repo, builds it, and runs it, with no checkout needed. App Fair
installs Android packages, so name that target rather than taking the desktop default:

```sh
cargo install day-cli
day doctor                                             # what's installed, what's missing
day launch --git https://github.com/marcprux/App-Fair.git -t android-material
```

`day doctor` prints the fix for anything it can't find. `day launch --git` prints where it put the
checkout, so you can `cd` there and edit the code.

From inside a clone, name the target directly — Day compiles one backend per binary, and the Day
CLI supplies the right feature:

```sh
day launch -t android-material      # build + run on a device/emulator
day launch -t android-material --script dayscript/walkthrough.yaml --env APP_FAIR_MOCK=1   # scripted, offline
cargo test                          # the catalog, sync, and parsing tests
```

## Release

Signing keys are referenced from `Day.toml` (`[signing.android]`) via environment variables:

```sh
export DAY_ANDROID_KEYSTORE=/path/to/keystore.jks DAY_ANDROID_KEY_ALIAS=app \
       DAY_KS_PASS=… DAY_KEY_PASS=…
day sign --check                               # confirm the config resolves
day pack -t android-material                   # signed .apk + .aab in build/day/dist/
day store stage                                # the Play and App Store listings, ready for fastlane
```

See [`RELEASE.md`](RELEASE.md) for the full release checklist and the Google Play policy review notes.
