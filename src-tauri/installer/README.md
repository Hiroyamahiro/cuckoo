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
identifier. Uninstall keeps settings unless the user explicitly requests removal.

When updating Tauri CLI, compare the upstream template before refreshing this
copy and repeat coexistence/install/update/uninstall checks. Tauri and NSIS
notices are bundled under `licenses/third-party`.
