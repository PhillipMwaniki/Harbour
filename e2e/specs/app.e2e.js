// A smoke test against the real, built app: it boots, the session-manager
// sidebar renders, and its dialogs open and close. This is deliberately shallow
// - it proves the webview, the IPC bridge and the window all come up together,
// which unit and component tests cannot - rather than re-testing behaviour the
// component tests already cover.

import { browser, $, expect } from "@wdio/globals";

describe("Harbour boots", () => {
  it("renders the app and the session-manager sidebar", async () => {
    // The React root fills in once the app has mounted.
    const root = await $("#root");
    await root.waitForExist({ timeout: 60_000 });

    // A fresh vault shows the empty-state prompt in the sidebar; the import
    // buttons in its footer are always there.
    const importButton = await $("button*=Import OpenSSH");
    await importButton.waitForDisplayed({ timeout: 30_000 });
    await expect(importButton).toBeDisplayed();
  });

  it("opens and closes the fleet runner", async () => {
    const open = await $("button*=Run on many hosts");
    await open.waitForDisplayed({ timeout: 30_000 });
    await open.click();

    const dialog = await $('[aria-label="Run on many hosts"]');
    await dialog.waitForDisplayed({ timeout: 10_000 });
    await expect(dialog).toBeDisplayed();

    const close = await dialog.$("button=Close");
    await close.click();
    await dialog.waitForDisplayed({ timeout: 10_000, reverse: true });
  });

  it("opens the encrypted-export dialog", async () => {
    const open = await $("button=Export vault");
    await open.waitForDisplayed({ timeout: 30_000 });
    await open.click();

    const dialog = await $('[aria-label="Export vault"]');
    await dialog.waitForDisplayed({ timeout: 10_000 });
    // The passphrase field and its confirmation are the heart of the dialog.
    await expect(await dialog.$("#backup-passphrase")).toBeExisting();

    const cancel = await dialog.$("button=Cancel");
    await cancel.click();
    await dialog.waitForDisplayed({ timeout: 10_000, reverse: true });
  });
});
