//! The platform boundary. On Android these call the `DayInstaller` Java bridge through
//! day-android's cached JVM/Context; everywhere else (the desktop mock build) they are stubs so
//! the app still compiles and the UI can be exercised.

use std::path::PathBuf;

#[cfg(target_os = "android")]
const INSTALLER_CLASS: &str = "org/appfair/app/appfair/DayInstaller";

/// The app's private data directory, resolved once and cached. JNI `FindClass` for app classes
/// only works on the UI thread (a background thread gets the system classloader), so this must be
/// first called from the main thread — `root()` does that before any sync thread starts.
#[cfg(target_os = "android")]
pub fn data_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        use day_android::{DayEnv, as_jstring, with_env};
        with_env(|env| {
            let obj = env
                .dcall_static(INSTALLER_CLASS, "filesDir", "()Ljava/lang/String;", &[])
                .ok()?
                .l()
                .ok()?;
            if obj.is_null() {
                return None;
            }
            env.dstr(&as_jstring(obj)).ok()
        })
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/app-fair"))
    })
    .clone()
}

#[cfg(not(target_os = "android"))]
pub fn data_dir() -> PathBuf {
    std::env::temp_dir().join("app-fair")
}

/// The installed versionCode for `pkg`, or `None` when the app is not installed.
#[cfg(target_os = "android")]
pub fn installed_version(pkg: &str) -> Option<i64> {
    use day_android::jni::objects::JValue;
    use day_android::{DayEnv, with_env};
    with_env(|env| {
        let p = env.new_string(pkg).ok()?;
        let code = env
            .dcall_static(
                INSTALLER_CLASS,
                "installedVersion",
                "(Ljava/lang/String;)J",
                &[JValue::Object(&p)],
            )
            .ok()?
            .j()
            .ok()?;
        (code >= 0).then_some(code)
    })
}

#[cfg(not(target_os = "android"))]
pub fn installed_version(_pkg: &str) -> Option<i64> {
    None
}

/// Launch an installed app. `Ok(())` on success; `Err(reason)` carries a human-readable message
/// (no launcher activity, no context, or a thrown exception) for the caller to show in a dialog.
#[cfg(target_os = "android")]
pub fn launch_app(pkg: &str) -> Result<(), String> {
    use day_android::jni::objects::JValue;
    use day_android::{DayEnv, as_jstring, with_env};
    let msg = with_env(|env| {
        let p = env.new_string(pkg).ok()?;
        let obj = env
            .dcall_static(
                INSTALLER_CLASS,
                "launchApp",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&p)],
            )
            .ok()?
            .l()
            .ok()?;
        if obj.is_null() {
            return Some(String::new());
        }
        env.dstr(&as_jstring(obj)).ok()
    });
    match msg {
        Some(m) if m.is_empty() => Ok(()),
        Some(m) => Err(m),
        None => Err("Couldn't reach the launcher.".to_string()),
    }
}

#[cfg(not(target_os = "android"))]
pub fn launch_app(_pkg: &str) -> Result<(), String> {
    Err("Launching apps is only supported on Android.".to_string())
}

/// Ask the system to uninstall `pkg` (#9), showing its uninstall confirmation. `Ok` once the
/// request is fired (the user still confirms); `Err` with a reason if it couldn't be started. Runs
/// on the UI thread (called from a button action), so it reaches the app classloader directly.
#[cfg(target_os = "android")]
pub fn uninstall(pkg: &str) -> Result<(), String> {
    use day_android::jni::objects::JValue;
    use day_android::{DayEnv, as_jstring, with_env};
    let msg = with_env(|env| {
        let p = env.new_string(pkg).ok()?;
        let obj = env
            .dcall_static(
                INSTALLER_CLASS,
                "uninstall",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&p)],
            )
            .ok()?
            .l()
            .ok()?;
        if obj.is_null() {
            return Some(String::new());
        }
        env.dstr(&as_jstring(obj)).ok()
    });
    match msg {
        Some(m) if m.is_empty() => Ok(()),
        Some(m) => Err(m),
        None => Err("Couldn't reach the uninstaller.".to_string()),
    }
}

#[cfg(not(target_os = "android"))]
pub fn uninstall(_pkg: &str) -> Result<(), String> {
    Err("Uninstalling apps is only supported on Android.".to_string())
}

/// The device's Android API level (`Build.VERSION.SDK_INT`), resolved once and cached. `0` when it
/// can't be read (off Android, or before the bridge is ready) — callers treat `0` as "unknown".
#[cfg(target_os = "android")]
pub fn device_sdk() -> i64 {
    static SDK: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *SDK.get_or_init(|| {
        use day_android::{DayEnv, with_env};
        with_env(|env| {
            env.dcall_static(INSTALLER_CLASS, "deviceSdk", "()I", &[])
                .ok()?
                .i()
                .ok()
                .map(|v| v as i64)
        })
        .unwrap_or(0)
    })
}

#[cfg(not(target_os = "android"))]
pub fn device_sdk() -> i64 {
    0
}

/// The device's supported native-code ABIs (`Build.SUPPORTED_ABIS`), for version-compatibility
/// filtering (#5). Cached; must be first called on the UI thread (`DayInstaller` is an app class),
/// which `root()` does. Empty off Android or when unreadable — callers treat empty as "any".
#[cfg(target_os = "android")]
pub fn device_abis() -> Vec<String> {
    static ABIS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    ABIS.get_or_init(|| {
        use day_android::{DayEnv, as_jstring, with_env};
        with_env(|env| {
            let obj = env
                .dcall_static(INSTALLER_CLASS, "deviceAbis", "()Ljava/lang/String;", &[])
                .ok()?
                .l()
                .ok()?;
            if obj.is_null() {
                return None;
            }
            env.dstr(&as_jstring(obj)).ok()
        })
        .map(|s| {
            s.split('\t')
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
    })
    .clone()
}

#[cfg(not(target_os = "android"))]
pub fn device_abis() -> Vec<String> {
    Vec::new()
}

/// Whether this platform can verify repository signatures (Android, via JarFile). Desktop/mock
/// builds can't, so they fall back to the plain unsigned index for dev.
pub fn verifies_signatures() -> bool {
    cfg!(target_os = "android")
}

/// Run `f` on the UI thread and block until it returns. Needed because catalog signature checks run
/// on a sync thread but `DayInstaller` is an app class (JNI `FindClass` for it only resolves on the
/// UI thread). `None` if the result doesn't come back within the timeout.
#[cfg(target_os = "android")]
fn on_main_blocking<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    let (tx, rx) = std::sync::mpsc::channel();
    day_reactive::on_main(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(30)).ok()
}

/// Verify a repo's signed `entry.jar` against `fingerprint` and return the contained `entry.json`
/// text, or `None` if the signature/cert doesn't verify (#1). Writes the JAR to a temp file and
/// runs Android's `JarFile` verification on the UI thread.
#[cfg(target_os = "android")]
pub fn verify_entry_jar(jar_bytes: &[u8], fingerprint: &str) -> Option<String> {
    let path = data_dir().join("entry.jar.tmp");
    std::fs::write(&path, jar_bytes).ok()?;
    let path_str = path.to_string_lossy().to_string();
    let fp = fingerprint.to_string();
    let result = on_main_blocking(move || {
        use day_android::jni::objects::JValue;
        use day_android::{DayEnv, as_jstring, with_env};
        with_env(|env| {
            let p = env.new_string(&path_str).ok()?;
            let f = env.new_string(&fp).ok()?;
            let obj = env
                .dcall_static(
                    INSTALLER_CLASS,
                    "verifyEntryJar",
                    "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&p), JValue::Object(&f)],
                )
                .ok()?
                .l()
                .ok()?;
            if obj.is_null() {
                return None;
            }
            env.dstr(&as_jstring(obj)).ok()
        })
    });
    let _ = std::fs::remove_file(&path);
    result.flatten().filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "android"))]
pub fn verify_entry_jar(_jar_bytes: &[u8], _fingerprint: &str) -> Option<String> {
    None
}

/// The SHA-256 of `apk_path`'s signing certificate, for comparison against the catalog's pinned
/// signer (#3). Empty when unreadable (the caller then skips the check).
#[cfg(target_os = "android")]
pub fn apk_signer_sha256(apk_path: &str) -> String {
    let path = apk_path.to_string();
    on_main_blocking(move || {
        use day_android::jni::objects::JValue;
        use day_android::{DayEnv, as_jstring, with_env};
        with_env(|env| {
            let p = env.new_string(&path).ok()?;
            let obj = env
                .dcall_static(
                    INSTALLER_CLASS,
                    "apkSignerSha256",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&p)],
                )
                .ok()?
                .l()
                .ok()?;
            if obj.is_null() {
                return None;
            }
            env.dstr(&as_jstring(obj)).ok()
        })
    })
    .flatten()
    .unwrap_or_default()
}

#[cfg(not(target_os = "android"))]
pub fn apk_signer_sha256(_apk_path: &str) -> String {
    String::new()
}

/// The device's current locale as a BCP-47 tag (e.g. `fr-FR`), resolved once and cached. Used to
/// pick catalog translations so descriptions/screenshots match the device language. `java.util
/// .Locale` is a boot-class, so this JNI call is safe from any thread (unlike the app-class bridge
/// calls). Falls back to `en-US` — F-Droid's primary — when it can't be read.
#[cfg(target_os = "android")]
pub fn device_locale() -> String {
    static LOCALE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LOCALE
        .get_or_init(|| {
            use day_android::{DayEnv, as_jstring, with_env};
            with_env(|env| {
                // java.util.Locale.getDefault().toLanguageTag()
                let locale = env
                    .dcall_static(
                        "java/util/Locale",
                        "getDefault",
                        "()Ljava/util/Locale;",
                        &[],
                    )
                    .ok()?
                    .l()
                    .ok()?;
                let tag = env
                    .dcall(&locale, "toLanguageTag", "()Ljava/lang/String;", &[])
                    .ok()?
                    .l()
                    .ok()?;
                if tag.is_null() {
                    return None;
                }
                env.dstr(&as_jstring(tag)).ok()
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "en-US".to_string())
        })
        .clone()
}

#[cfg(not(target_os = "android"))]
pub fn device_locale() -> String {
    "en-US".to_string()
}

/// Every user-installed app as `(pkg, versionCode)`. Empty off Android.
#[cfg(target_os = "android")]
pub fn installed_packages() -> Vec<(String, i64)> {
    use day_android::{DayEnv, as_jstring, with_env};
    let raw = with_env(|env| {
        let obj = env
            .dcall_static(
                INSTALLER_CLASS,
                "installedPackages",
                "()Ljava/lang/String;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        if obj.is_null() {
            return None;
        }
        env.dstr(&as_jstring(obj)).ok()
    })
    .unwrap_or_default();
    raw.lines()
        .filter_map(|line| {
            let (pkg, code) = line.split_once('\t')?;
            Some((pkg.to_string(), code.parse().ok()?))
        })
        .collect()
}

#[cfg(not(target_os = "android"))]
pub fn installed_packages() -> Vec<(String, i64)> {
    Vec::new()
}

/// The system's localized `(label, description)` for a permission. The label falls back to a
/// tidied form of the raw name; the description is empty when the platform doesn't define one.
#[cfg(target_os = "android")]
pub fn permission_info(name: &str) -> (String, String) {
    use day_android::jni::objects::JValue;
    use day_android::{DayEnv, as_jstring, with_env};
    let raw = with_env(|env| {
        let n = env.new_string(name).ok()?;
        let obj = env
            .dcall_static(
                INSTALLER_CLASS,
                "permissionInfo",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&n)],
            )
            .ok()?
            .l()
            .ok()?;
        if obj.is_null() {
            return None;
        }
        env.dstr(&as_jstring(obj)).ok()
    });
    match raw {
        Some(s) => {
            let (label, desc) = s.split_once('\t').unwrap_or((s.as_str(), ""));
            let label = if label.is_empty() {
                tidy_permission(name)
            } else {
                label.to_string()
            };
            (label, desc.to_string())
        }
        None => (tidy_permission(name), String::new()),
    }
}

#[cfg(not(target_os = "android"))]
pub fn permission_info(name: &str) -> (String, String) {
    (tidy_permission(name), String::new())
}

/// A readable fallback label: `android.permission.ACCESS_FINE_LOCATION` → `Access fine location`.
pub fn tidy_permission(name: &str) -> String {
    let short = name.rsplit('.').next().unwrap_or(name);
    let mut words = short.split('_').map(|w| w.to_ascii_lowercase());
    let mut out = String::new();
    if let Some(first) = words.next() {
        let mut chars = first.chars();
        if let Some(c) = chars.next() {
            out.extend(c.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    for w in words {
        out.push(' ');
        out.push_str(&w);
    }
    out
}

/// Hand a downloaded APK to Android's PackageInstaller. The result arrives asynchronously via the
/// `nativeInstallResult` JNI callback below. Marshalled to the UI thread because `FindClass` for
/// `DayInstaller` only resolves there.
#[cfg(target_os = "android")]
pub fn install_apk(path: &str, label: &str) {
    let (path, label) = (path.to_string(), label.to_string());
    day_reactive::on_main(move || {
        use day_android::jni::objects::JValue;
        use day_android::{DayEnv, with_env};
        with_env(|env| {
            let (Ok(p), Ok(l)) = (env.new_string(&path), env.new_string(&label)) else {
                crate::state::on_native_install_status(1, Some("bad path".into()));
                return;
            };
            let _ = env.dcall_static(
                INSTALLER_CLASS,
                "install",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&p), JValue::Object(&l)],
            );
        });
    });
}

#[cfg(not(target_os = "android"))]
pub fn install_apk(_path: &str, _label: &str) {
    crate::state::on_native_install_status(1, Some("Install is Android-only".into()));
}

/// PackageInstaller result callback, invoked by `DayInstaller`'s BroadcastReceiver. Mirrors the
/// pattern in `day::android_main!`: the FFI-safe `EnvUnowned` is upgraded to a real `Env`, and the
/// body is wrapped so a panic never crosses the JNI boundary.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_appfair_app_appfair_DayInstaller_nativeInstallResult<'local>(
    mut env: day::android::jni::EnvUnowned<'local>,
    _class: day::android::jni::objects::JClass<'local>,
    status: day::android::jni::sys::jint,
    message: day::android::jni::objects::JString<'local>,
) {
    let _ = env.with_env(|env| {
        let msg = day::android::read_jstring(env, &message);
        crate::state::on_native_install_status(status, msg);
        Ok::<(), day::android::jni::errors::Error>(())
    });
}

/// Register the periodic background update check (#7). Runs on the UI thread so `FindClass` for the
/// app's `DayInstaller` resolves.
#[cfg(target_os = "android")]
pub fn schedule_update_checks() {
    day_reactive::on_main(|| {
        use day_android::{DayEnv, with_env};
        with_env(|env| {
            let _ = env.dcall_static(INSTALLER_CLASS, "scheduleUpdateChecks", "()V", &[]);
        });
    });
}

#[cfg(not(target_os = "android"))]
pub fn schedule_update_checks() {}

/// Background update-check entry point, invoked by `UpdateWorker` on its WorkManager thread. Java
/// passes the installed packages (it holds the app classloader on that thread); Rust syncs the
/// catalogs and returns how many installed apps now have an update, which the worker turns into a
/// notification.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_appfair_app_appfair_UpdateWorker_nativeCheckUpdates<'local>(
    mut env: day::android::jni::EnvUnowned<'local>,
    _class: day::android::jni::objects::JClass<'local>,
    installed: day::android::jni::objects::JString<'local>,
) -> day::android::jni::sys::jint {
    env.with_env(|env| -> day::android::jni::errors::Result<i32> {
        let raw = day::android::read_jstring(env, &installed).unwrap_or_default();
        // Lines of "pkg\tversionCode", the same shape `installedPackages()` returns.
        let list: Vec<(String, i64)> = raw
            .lines()
            .filter_map(|line| {
                let (pkg, code) = line.split_once('\t')?;
                Some((pkg.to_string(), code.trim().parse().ok()?))
            })
            .collect();
        Ok(crate::sync::check_updates_now(&list) as i32)
    })
    .resolve::<day::android::jni::errors::ThrowRuntimeExAndDefault>()
}
