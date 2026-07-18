# App Fair — R8/ProGuard keep rules, folded into the release build by `day build` (declared in
# Cargo.toml under [package.metadata.day.android].proguard). See docs/extending.md for the
# convention: any component that hands Java classes to native code by name ships one of these.
#
# These classes are reached from Rust BY NAME, so R8 must not rename them or their members:
#   - DayInstaller: found via JNI FindClass, and its static methods are invoked by name
#     (dcall_static "install" / "launchApp" / "deviceSdk" / …); it also hosts the native callback
#     nativeInstallResult (C symbol Java_org_appfair_app_appfair_DayInstaller_nativeInstallResult).
#   - UpdateWorker: WorkManager instantiates it from its class name, and it hosts nativeCheckUpdates.
-keep class org.appfair.app.appfair.DayInstaller { *; }
-keep class org.appfair.app.appfair.UpdateWorker { *; }

# App Fair's WorkManager dependency (background update checks, #7) persists its queue in a Room
# database that Room instantiates reflectively — it looks up "<database>_Impl" by name — so R8's
# renaming breaks the lookup ("Failed to create an instance of WorkDatabase"). Keep the database
# classes and the generated implementation. This is App Fair's own dependency, not Day's, so its
# rule lives here.
-keep class * extends androidx.room.RoomDatabase { *; }
-keep class androidx.work.impl.WorkDatabase_Impl { *; }
