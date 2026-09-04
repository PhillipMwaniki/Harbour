// End-to-end configuration: drives the *built* Harbour app through
// `tauri-driver`, which proxies WebDriver to the platform's native webview
// driver (WebKitWebDriver on Linux, Edge's msedgedriver on Windows). macOS has
// no such driver, so these run on Linux and Windows only.
//
// The app binary must already be built (`pnpm tauri build --debug --no-bundle`);
// `tauri-driver` launches it per session and closes it afterwards.

import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");

// The debug binary the tauri build produces. `--no-bundle` leaves it here
// without also making a .deb / .AppImage / .msi.
const binary = path.join(
  root,
  "src-tauri",
  "target",
  "debug",
  process.platform === "win32" ? "harbour.exe" : "harbour",
);

let tauriDriver;

export const config = {
  runner: "local",
  specs: [path.join(here, "specs", "**", "*.e2e.js")],
  maxInstances: 1,
  capabilities: [
    {
      // tauri-driver reads this to know which app to launch.
      "tauri:options": { application: binary },
    },
  ],
  hostname: "127.0.0.1",
  port: 4444,
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 120_000 },

  // Start tauri-driver before the session and stop it after, so a run cleans up
  // after itself even when a test fails.
  beforeSession: () => {
    tauriDriver = spawn("tauri-driver", [], {
      stdio: [null, process.stdout, process.stderr],
    });
  },
  afterSession: () => {
    tauriDriver?.kill();
  },
};
