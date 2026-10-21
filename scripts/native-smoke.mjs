// Own-app integration test through WebView2's test-only CDP port. Production does
// not enable a debugging port. No input is sent to WeChat or authentication UIs.
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { readFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import assert from "node:assert/strict";

if (process.platform !== "win32")
  throw new Error("This WebView2 smoke test is Windows-only.");
const root = resolve(import.meta.dirname, "..");
const port = 19327;
await mkdir(resolve(root, "artifacts"), { recursive: true });
const child = spawn(
  resolve(root, "src-tauri/target/debug/more-wechat.exe"),
  [],
  {
    cwd: root,
    windowsHide: true,
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);
let browser;
let originalTarget;
let page;
try {
  let connected = false;
  for (let attempt = 0; attempt < 40; attempt++) {
    if (child.exitCode !== null)
      throw new Error(`Native app exited: ${child.exitCode}`);
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) {
        connected = true;
        break;
      }
    } catch {
      /* bounded startup wait */
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  assert(connected, "Own WebView2 test port must become available");
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  for (let attempt = 0; attempt < 20; attempt++) {
    page = browser
      .contexts()
      .flatMap((context) => context.pages())
      .find((page) => !page.url().startsWith("devtools:"));
    if (page) break;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  assert(page, "Own app webview must exist");
  await page.waitForFunction(() => Boolean(window.__TAURI_INTERNALS__), {
    timeout: 15000,
  });
  const invoke = async (command, args = {}) => {
    const result = await page.evaluate(
      async ({ command, args }) => {
        try {
          return {
            ok: true,
            value: await window.__TAURI_INTERNALS__.invoke(command, args),
          };
        } catch (error) {
          return { ok: false, error };
        }
      },
      { command, args },
    );
    if (!result.ok) throw result.error;
    return result.value;
  };
  const snapshot = await invoke("get_snapshot");
  assert.equal(snapshot.platform, "windows");
  assert.equal(snapshot.compatibility.status, "unverified");
  originalTarget = snapshot.settings.targetCount;
  await invoke("save_target", { target: 1 });
  assert.equal((await invoke("get_snapshot")).settings.targetCount, 1);
  await assert.rejects(
    invoke("save_target", { target: 6 }),
    (error) => error.code === "invalid_target",
  );
  await assert.rejects(
    invoke("select_installation", {
      path: resolve(root, "does-not-exist.exe"),
    }),
    (error) => error.code === "invalid_installation",
  );
  if (snapshot.instances.length >= 1 && snapshot.instances.length <= 5) {
    const result = await invoke("launch", {
      target: snapshot.instances.length,
    });
    assert.equal(result.status, "unchanged");
    assert.equal(result.after, snapshot.instances.length);
  }
  const diagnosticPath = resolve(root, "artifacts/native-diagnostics.json");
  assert.equal(
    await invoke("export_diagnostics", { path: diagnosticPath }),
    diagnosticPath,
  );
  const reportText = await readFile(diagnosticPath, "utf8");
  const report = JSON.parse(reportText);
  assert.equal(report.application, "more-WeChat");
  assert.equal(report.snapshot.platform, "windows");
  if (process.env.USERPROFILE)
    assert(
      !reportText.toLowerCase().includes(process.env.USERPROFILE.toLowerCase()),
      "Home path must be hidden",
    );
  await invoke("save_target", { target: originalTarget });
  await page.getByRole("button", { name: "重新检测" }).click();
  await page
    .getByRole("button", { name: "重新检测" })
    .waitFor({ state: "visible" });
  await page.waitForFunction(
    () => !document.querySelector(".refresh-button")?.disabled,
  );
  assert.equal(
    await page.getByText("界面预览 · 不会启动微信", { exact: false }).count(),
    0,
  );
  await page.screenshot({
    path: resolve(root, "artifacts/windows-native.png"),
    fullPage: true,
  });
  console.log(
    `Native IPC smoke passed; ${snapshot.instances.length} real main process(es), version ${snapshot.installation?.version ?? "not installed"}, compatibility ${snapshot.compatibility.status}.`,
  );
} finally {
  if (page && originalTarget) {
    try {
      await page.evaluate(
        (target) =>
          window.__TAURI_INTERNALS__.invoke("save_target", { target }),
        originalTarget,
      );
    } catch {
      /* preserve original test failure */
    }
  }
  if (browser) await browser.close();
  child.kill();
}
