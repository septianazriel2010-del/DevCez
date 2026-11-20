# CEZECORE — Phase 1 Foundation

**Developer & Network Intelligence** · Rust + Tauri 2 + React + TypeScript

This is a complete Phase 1 starter project. It includes the desktop shell, premium dark graphite dashboard, sidebar navigation, Vite/React setup, Tauri 2 configuration, a Rust command that reads a local system snapshot, and a minimal permission capability.

> Phase 1 scope: application foundation and live CPU/memory/device snapshot. Network diagnostics and developer project management are intentionally marked as upcoming modules; they are not mocked as working features.

## 1. Windows prerequisites

Install these before running the project:

1. **Visual Studio Code**.
2. **Node.js LTS** (npm is included).
3. **Rust stable with the MSVC toolchain**.
4. **Microsoft C++ Build Tools** with the *Desktop development with C++* workload and a Windows 10/11 SDK.
5. **Microsoft Edge WebView2 Runtime**. It is commonly already present on Windows 10/11.

For Microsoft C++ Build Tools, use the Visual Studio Installer and select the workload **Desktop development with C++**. Make sure an MSVC compiler and Windows SDK are selected.

Open a new PowerShell window after installation and verify:

```powershell
node --version
npm --version
rustc --version
cargo --version
```

Set the Rust MSVC target:

```powershell
rustup default stable-x86_64-pc-windows-msvc
rustup show
```

## 2. Open the project

Extract `CEZECORE_Phase1.zip` to a normal development folder, for example:

```text
C:\Users\YourUser\Documents\Projects\CEZECORE_Phase1
```

Open that extracted folder in VS Code. Do **not** open only the `src` or `src-tauri` subfolder.

You can run `START_CEZECORE.bat` from File Explorer to install frontend dependencies (if needed) and launch the desktop development app. Or, in the VS Code terminal (PowerShell), run:

```powershell
npm install
```

This downloads the JavaScript dependencies. The first Rust build will also download and compile Rust crates automatically.

## 3. Run CEZECORE as a Windows desktop app

From the project root:

```powershell
npm run tauri dev
```

Expected result:

- A native CEZECORE desktop window opens.
- The dashboard loads with the Premium Dark Graphite theme.
- CPU, memory, processor thread count, OS, and device name are requested from the Rust backend.
- The dashboard refreshes the system snapshot every six seconds.
- Sidebar pages navigate between the dashboard and the upcoming module placeholders.

The first Rust build can take longer because Cargo needs to download and compile dependencies.

## 4. Optional browser-only UI preview

```powershell
npm run dev
```

Open the local address printed by Vite (normally `http://127.0.0.1:1420`). This previews the React interface only. The Rust system bridge is unavailable in an ordinary browser, so live system metrics require `npm run tauri dev`.

## 5. Validate the frontend

```powershell
npm run build
```

This runs TypeScript validation and creates the frontend production build in `dist/`.

## 6. Build the Windows executable

To compile the app executable without creating an installer bundle, double-click `BUILD_EXE.bat`, or run this command from the project root:

```powershell
npm run tauri build -- --no-bundle
```

Expected executable location:

```text
src-tauri\target\release\cezecore.exe
```

To create the NSIS installer, install NSIS on Windows and make sure `makensis.exe` is available to the build process, then run:

```powershell
npm run tauri build
```

The installer is normally written under:

```text
src-tauri\target\release\bundle\nsis\
```

## 7. Project structure

```text
CEZECORE_Phase1/
├── .vscode/                  # Recommended editor extensions
├── src/
│   ├── components/           # Reusable React UI components
│   ├── lib/                  # Frontend-to-Tauri bridge
│   ├── types/                # TypeScript data contracts
│   ├── App.tsx               # App shell, navigation, dashboard
│   ├── main.tsx              # React entry point
│   └── styles.css            # Premium Dark Graphite design system
├── src-tauri/
│   ├── capabilities/         # Minimal Tauri permissions
│   ├── icons/                # Windows/application icons
│   ├── src/
│   │   ├── commands/         # Tauri commands exposed to React
│   │   ├── lib.rs            # Tauri application builder
│   │   └── main.rs           # Windows executable entry point
│   ├── Cargo.toml            # Rust dependencies
│   ├── build.rs
│   └── tauri.conf.json       # Window, Vite and bundle configuration
├── index.html
├── package.json
├── tsconfig.json
└── vite.config.ts
```

## 8. First-phase acceptance checklist

- [ ] `npm install` completes without errors.
- [ ] `npm run build` completes without TypeScript errors.
- [ ] `npm run tauri dev` opens a desktop window.
- [ ] Dashboard displays CPU and memory metrics from Rust.
- [ ] Refresh button fetches a new system snapshot.
- [ ] Sidebar navigation works.
- [ ] `npm run tauri build -- --no-bundle` creates `cezecore.exe`.

## Notes

- The first snapshot is a point-in-time reading. CPU usage may be low or briefly near zero on the first sample.
- Memory values are displayed in GiB (binary gigabytes) and are read locally.
- The project does not request administrator privileges.
- The Tauri capability currently grants only `core:default`; no unrestricted shell or filesystem permission is enabled.
- Keep `package-lock.json` and `src-tauri/Cargo.lock` in version control after dependencies have been resolved on your development machine.
