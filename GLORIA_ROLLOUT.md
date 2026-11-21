# GloriaSetup rollout

devCleanup is a Tauri 2 app (WebView2). Pushing a tag like `v1.2.0` now builds it and attaches `devCleanup_<version>_x64-setup.exe` to that release via `projectGloria/GloriaSetup@v0.1.0`.

## Please check

- Add the repo secrets `WINDOWS_PFX_BASE64` and `WINDOWS_PFX_PASSWORD`; without them the setup is unsigned.
- `data_folders` lists only `%LOCALAPPDATA%\com.projectgloria.devclean` (the WebView2 profile). The code writes no settings of its own and I found no use of `%APPDATA%\com.projectgloria.devclean`, so I left it out.
- `legacy_names = ["devClean"]` is the old NSIS DisplayName (Tauri `productName`). Confirm it matches what earlier installs registered.
- The payload is only `devclean-app.exe`; the Tauri app has no resources. The CLI `devclean.exe` is still published as a standalone release asset, not inside the setup.
- The old NSIS installer step is gone; the standalone exes and `SHA256SUMS.txt` are kept and uploaded with `gh release upload`.
- The workflow still requires the tag version to equal `app/src-tauri/tauri.conf.json`. `gloria.toml` `version` (1.0.0) should be bumped with it, though the tag overrides it at pack time.
- The setup is named `devCleanup_...` (per the catalog); the old installer was `devClean_...`.
- Not built or tested locally, as requested.
