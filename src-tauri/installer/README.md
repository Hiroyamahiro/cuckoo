# Windows NSIS template

The template is pinned to Tauri CLI 2.12.3:
https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi

Local changes isolate uninstall/installation registry keys by bundle identifier,
install to `LocalAppData/Programs/HiroyamaHiroshi/Cuckoo`, preserve foreign desktop
shortcuts, and avoid removing another application's autostart entry.
The hooks reject directories containing Squirrel's `Update.exe` or an unmarked
`Cuckoo.exe`. The marker identifies this application's installation only.

Keep the production identifier stable across updates. Settings belong to that
identifier, not the executable directory. Development uses a separate `.dev`
identifier. The silent engine keeps settings. The native maintenance UI offers explicitly confirmed production-data removal.

When updating Tauri CLI, compare the upstream template before refreshing this
copy and repeat coexistence/install/update/uninstall checks. Tauri and NSIS
notices are generated into `src/public/THIRD-PARTY-NOTICES.html` and opened from the application license dialog. See `licenses/README.md` for regeneration and dependency scope.

Start menu registration creates only `Programs/Cuckoo.lnk`, without a folder
or selection page. The shortcut name follows `productName`. Legacy folder links
are removed only if they target this installation; nonempty folders and foreign
shortcuts are preserved. Uninstall uses the same target checks.

The template is now a strictly silent internal engine. Build and distribute the native wrapper using setup/Build.ps1; direct NSIS output is not a user-facing installer. Windows uninstall entry points target CuckooMaintenance.exe. The wrapper verifies and prepares WebView2 before invoking the engine.
