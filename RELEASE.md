# App Fair — release & Google Play review notes

Version 1.0.0 (versionCode 1). This file covers how to build and submit the Android release, and —
importantly — the **app-review risks** you should weigh before submitting to Google Play.

## Build a signed release

```sh
export DAY_ANDROID_KEYSTORE=/opt/src/github/appfair/misc/keys/keystore.jks \
       DAY_ANDROID_KEY_ALIAS=app \
       DAY_KS_PASS=…  DAY_KEY_PASS=…          # from misc/keys/keystore.properties
day sign --check                              # expect: android: ok (release signing ready)
day pack -p android-widget --profile release  # → build/day/dist/app-fair-1.0.0.{apk,aab}
```

Verified: the `.apk`/`.aab` are signed with the release key (signer SHA-256
`08:2E:7B:…:38`, matching `keystore.jks`), `apksigner verify` passes, and `bundletool` builds
installable APKs from the `.aab`. The release (R8-minified) build installs and runs on a device.

## Store listing & submission (fastlane)

`android/fastlane/` holds the Play metadata and lanes:

- `Appfile` — `package_name` + the Google Play service account (`misc/keys/google-play-apikey.json`).
- `metadata/android/{en-US,fr-FR}/` — `title`, `short_description`, `full_description`,
  `changelogs/1.txt`, and `images/` (512 `icon.png`, 1024×500 `featureGraphic.png`, 6 phone screenshots).
- `Fastfile`:
  - `fastlane android validate` — **dry run**: validates the listing + AAB against the Play API
    with `validate_only` (never publishes).
  - `fastlane android internal_draft` — uploads to the **internal** track as a draft (still not
    released to users; a human promotes it).

**Dry-run result (run 2026-07-21):** the service account authenticated with Google Play
successfully; `supply` failed only with `Package not found: org.appfair.AppFair`. That is expected
and confirms the whole pipeline is correct — Google has no API to create a brand-new app listing,
so **the app must first be created once in the Play Console** for this package name. After that,
`fastlane android validate` will validate against the real listing.

> The credential the task pointed at, `apple-appstore-apikey.json`, is the **Apple** App Store
> Connect key — it is not used for Google Play. The Play upload uses the service account
> `google-play-apikey.json`. If you also ship the iOS build, the Apple key drives `fastlane deliver`
> from the `platform/ios` project.

## ⚠️ App-review risks — read before submitting

### 1. App Fair is a third-party app store that installs other apps — the biggest risk
App Fair downloads APKs from F-Droid catalogs and installs them. Google Play heavily restricts apps
that **distribute or install other apps** (Developer Program Policy: *Device and Network Abuse* →
"apps that install other apps"). Historically, alternative app stores and F-Droid clients are
routinely rejected from Google Play (F-Droid itself is not distributed on Play for this reason).
**This may block approval regardless of everything else below.** It cannot be "fixed" in code.

What already works in App Fair's favor (keep it this way, and cite it in the review notes):
- It installs **only** through Android's standard `PackageInstaller`, which shows the system's own
  confirmation dialog for **every** install — no silent/background installs.
- It installs **only** from catalogs the user explicitly adds, over HTTPS, and **verifies each APK
  against the catalog's published SHA-256** before installing.
- It ships no bundled payloads and downloads no executable code for itself.

Recommended paths: (a) submit to a **closed testing track** first and be ready to justify the store
functionality to review; (b) plan for likely rejection on production and keep **F-Droid / direct
APK / the `.aab` on GitHub Releases** as the primary distribution; (c) if you pursue Play, expect to
complete the store/installer declarations below and possibly an appeal.

### 2. `REQUEST_INSTALL_PACKAGES` — requires a Play Console declaration
Used to hand a downloaded APK to the system installer. Play requires a **permission declaration**
justifying it; "app store / app installer" is a recognized use case but is reviewed. **Action:**
declare it in Play Console → App content → *Sensitive app permissions*.

### 3. `QUERY_ALL_PACKAGES` — sensitive permission, requires a declaration and is narrowly allowed
Used to show *Install* vs *Update* and to detect already-installed apps. Google restricts this to a
short list of use cases (an app store/launcher/device-management app "that needs awareness of all
apps" can qualify) and rejects apps that don't. **Action:** either (a) declare it as an app store in
Play Console and justify it, or (b) reduce scope — App Fair could instead check only the specific
packages it shows using a `<queries>` element (per-package `<package>` entries or intent filters)
and `getPackageInfo`, avoiding `QUERY_ALL_PACKAGES` entirely. Option (b) is the safer path if
feasible for the catalog sizes involved; it removes one whole review surface.

### 4. Required Play Console items that are not code (must be completed in the console)
- **Privacy policy URL** — required. `PRIVACY.md` is written; **host it at a public URL** and enter
  that URL in Play Console. (Update the placeholder contact address first.)
- **Data safety form** — App Fair collects no personal data; declare "No data collected" and note it
  facilitates user-initiated downloads. Be accurate.
- **Content rating** questionnaire, **Target audience** (not directed at children), **App category**
  (Tools / or Libraries & Demo), **contact email**, and a **store contact website**.
- **Play App Signing** — enroll; the `keystore.jks` here becomes your **upload key** (Google holds
  the app signing key). Keep the upload key safe.
- **First release / new accounts** — new personal developer accounts must run **closed testing with
  ≥12 testers for 14 days** before production access. Use the `internal` track for your own testing.

### 5. Smaller items
- Signing cert DN is generic (`CN=App, OU=Unknown, …`). Cosmetic; fine for an upload key.
- Store screenshots are honest device captures (1080×2400, 20:9) and include the emulator status
  bar — acceptable, but you may want cleaner captures. If the upload flags the aspect ratio (Play
  historically capped phone screenshots at 2:1), pad them to 1200×2400.
- `day lint` reports two `duplicate-id` warnings (`detail-install`, `install-dismiss`). These are
  intentional: a helper renders one of two mutually-exclusive branches that share a stable id so the
  dayscript/tests can target it. Only one renders at runtime — not a bug, left as-is.
- Target SDK 35 / min SDK 24 meet Play's current requirements; the `.apk` passes 16 KB
  page-alignment (`day pack` runs `zipalign`), which Android 15+ devices require.
- Confirm the "App Fair" name/trademark is clear for your account (these keys belong to the App Fair
  project, so this should be fine).

## Bottom line
The build, signing, bundle, icon, and listing are release-ready and the fastlane pipeline is proven
up to the one manual step (creating the app in Play Console). The **material risk is policy, not
mechanics**: App Fair is an app store that installs other apps, which Google Play restricts heavily.
Decide the distribution strategy (Play closed testing + declarations vs. F-Droid/direct/GitHub)
before investing in the production submission.
