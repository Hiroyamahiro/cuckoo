# Native Windows setup

Updates require explicit approval before requesting `--request-update-exit` from
the exact installed executable. Process handles, not window visibility, confirm
exit, including when the WebView is destroyed in the tray. There is no force-kill
fallback. Versions before 2.0.1 require manual tray exit. Refusal, timeout or an
unconfirmed process state pauses replacement. A per-installation semaphore blocks
new app starts during replacement and is retained if the engine state is unknown.
Updates restart after file and registration verification; install never opens the
settings, presets or custom-audio storage.

The application checks the fixed official GitHub Releases endpoint in a separate
worker at startup and roughly every 24 hours. Only higher stable SemVer releases
with the exact Windows x64 EXE and SHA256SUMS.txt URLs are eligible. HTTPS
redirects are restricted to GitHub delivery hosts. Bounds, cancellation, exact
size and hash are checked before launching a detached installer with no inherited
stdio. Automatic network errors are silent; progress and manual retries stay in
the existing app menu. Builds never publish or modify GitHub Releases.

Build prerequisites: the normal Cuckoo toolchain and the .NET Framework 4.8 compiler (`Framework64/v4.0.30319/csc.exe`). The installer UI uses WinForms and never requires WebView2 to display. Run from the repository root:

```powershell
npm.cmd run test:installer
npm.cmd run build:installer
```

Distribute only `setup/dist/Cuckoo_<version>_x64-setup.exe` and `SHA256SUMS.txt`. The Tauri-generated NSIS executable is an internal, always-silent engine; do not distribute it separately. Build inputs, previews, logs and output binaries are ignored by Git. The Microsoft WebView2 bootstrapper is downloaded during packaging, its Microsoft Authenticode signature is checked, and its bytes are bound into the payload manifest. A missing runtime requires internet access at installation time.

`Program.cs` selects the setup or maintenance entry point. `Flow.cs` owns state transitions; `Backend.cs` owns staging, verification, subprocesses and Windows integration; `SetupForm.cs` owns the single-page UI. Diagnostic previews and fault injection live only in the separate test executable.

Installation is per user. Updating preserves the production identifier, install location and settings. The wrapper uses the internal engine's `/UPDATE` path, avoiding recursive calls through the Windows maintenance registration. Replacement requires confirmed exit after user approval; versions before 2.0.1 require manual exit. Cuckoo is never force-terminated. Completion requires expected installed-file hashes, registration and the root Start Menu shortcut to match. Tauri's temporary `UNK`/`NSS` bundle-marker transformation is accounted for explicitly when generating the installed EXE digest; an unexpected marker fails the build.

Windows uninstall registration opens the native maintenance UI. It runs from a private temporary copy so the installed maintenance executable can be removed. By default uninstall preserves settings and sounds. The unchecked optional removal checkbox requires confirmation and removes only the fixed production LocalAppData namespace after application removal is confirmed. Reparse points are rejected; incomplete deletion is not reported as success. Quiet uninstall always preserves user data. Temporary maintenance copies may remain under the user's `CuckooSetup/Runs` directory after exit; they are not application settings and are not public build inputs.

Process time limits are warnings, not forced termination. Unknown results block retry. While a child or its output pipes remain active, close and duplicate operations stay blocked and staging is retained. SHA checks detect corruption, not publisher authenticity: distributions remain unsigned until a signing certificate is configured.

`test:installer` checks state transitions, runtime-install failure injection and integrity/path safeguards, and builds `SetupPreview.exe`. Preview arguments are `<ready|update|missing|working|complete|failed|unknown> <output.png> <scale>`. Scaling previews supplement real DPI testing; they do not replace a clean Windows test of runtime installation or network failure.
