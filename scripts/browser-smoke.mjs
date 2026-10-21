import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

// This smoke exercises the explicit browser preview. It does not verify native WeChat compatibility.
const port = process.env.SMOKE_PORT || "1431";
const origin = `http://127.0.0.1:${port}`;
const server = spawn(
  process.execPath,
  [
    "node_modules/vite/bin/vite.js",
    "--host",
    "127.0.0.1",
    "--port",
    port,
    "--strictPort",
  ],
  { stdio: ["ignore", "pipe", "pipe"], windowsHide: true },
);
let serverOutput = "";
server.stdout.on("data", (data) => {
  serverOutput += data;
});
server.stderr.on("data", (data) => {
  serverOutput += data;
});
let browser;
try {
  for (let attempt = 0; ; attempt += 1) {
    try {
      if ((await fetch(origin)).ok) break;
    } catch {
      /* Wait for the owned dev server. */
    }
    assert(
      attempt < 100 && server.exitCode === null,
      `Vite did not start: ${serverOutput}`,
    );
    await new Promise((done) => setTimeout(done, 100));
  }
  const channel =
    process.env.PLAYWRIGHT_BROWSER_CHANNEL ||
    (process.platform === "win32" ? "msedge" : undefined);
  browser = await chromium.launch({ headless: true, channel });
  const page = await browser.newPage({
    viewport: { width: 1040, height: 780 },
    reducedMotion: "reduce",
    locale: "zh-CN",
  });
  const errors = [];
  const externalRequests = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (!request.url().startsWith(origin) && !request.url().startsWith("data:"))
      externalRequests.push(request.url());
  });
  await page.goto(origin);
  await page.getByText("界面预览 · 不会启动微信", { exact: true }).waitFor();
  await page.getByText("微信 WeChat", { exact: true }).waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "启动至 2 个实例", exact: true })
      .isEnabled(),
    false,
  );
  assert.equal(
    await page
      .getByRole("button", { name: "重新检测", exact: true })
      .isEnabled(),
    true,
  );
  assert(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
    "Desktop layout overflows horizontally",
  );
  await mkdir("artifacts", { recursive: true });
  const screenshotPath = process.argv.includes("--screenshots")
    ? "docs/images/desktop-preview.png"
    : "artifacts/desktop-preview.png";
  await mkdir(resolve(screenshotPath, ".."), { recursive: true });
  await page.screenshot({ path: screenshotPath, fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  assert(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
    "Minimum window layout overflows horizontally",
  );
  await page.screenshot({ path: "artifacts/min-width.png", fullPage: true });
  await page.setViewportSize({ width: 1040, height: 780 });
  await page
    .getByRole("button", { name: "先启动一个微信", exact: true })
    .click();
  const launch = page.getByRole("button", { name: "启动微信", exact: true });
  await launch.waitFor();
  assert.equal(await launch.isEnabled(), true);
  await launch.focus();
  await page.keyboard.press("Enter");
  await page.getByRole("progressbar", { name: "微信启动进度" }).waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "正在启动，请稍候…", exact: true })
      .isEnabled(),
    false,
  );
  await page
    .getByRole("status")
    .filter({ hasText: "演示完成：真实微信未启动。" })
    .waitFor();
  assert.equal(
    await page.getByLabel("目标总数量", { exact: true }).textContent(),
    "1",
  );
  await page.getByRole("button", { name: "诊断详情", exact: true }).click();
  assert.equal(
    await page
      .getByRole("button", { name: "诊断详情", exact: true })
      .getAttribute("aria-expanded"),
    "true",
  );
  assert.equal(
    await page
      .getByText("诊断导出会隐藏用户目录中的用户名，不包含微信聊天内容。", {
        exact: true,
      })
      .isVisible(),
    true,
  );
  assert.equal(
    await page.evaluate(
      () =>
        getComputedStyle(document.documentElement).scrollbarColor === "auto",
    ),
    false,
    "Global scrollbar styling is missing",
  );
  assert.deepEqual(errors, [], "Browser runtime errors");
  assert.deepEqual(
    externalRequests,
    [],
    "Offline preview requested external assets",
  );
  console.log(
    "Browser preview smoke passed: compatibility gate, single-instance selection, keyboard launch, progress, completion, diagnostics, offline assets, reduced motion, and 760px layout.",
  );
} finally {
  await browser?.close();
  server.kill();
}
