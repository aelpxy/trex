// Run inside the sandbox by the browse tool: `node --input-type=module - <request json>`.
// Prints one JSON line: what the page showed, where its screenshot is, and what went wrong.
import { mkdir } from "node:fs/promises";

const PLAYWRIGHT = "/usr/local/lib/node_modules/playwright/index.mjs";
const VIEWPORTS = { desktop: { width: 1280, height: 800 }, mobile: { width: 390, height: 844 } };
const NAVIGATION_TIMEOUT_MS = 30_000;
const STEP_TIMEOUT_MS = 10_000;
const SETTLE_MS = 2_000;
const MAX_OUTLINE_CHARS = 6_000;
const MAX_MESSAGES = 30;

const request = JSON.parse(process.argv[2]);
const report = { console: [], failed: [], steps: [] };
const done = () => {
  console.log(JSON.stringify(report));
  process.exit(0);
};

let chromium;
try {
  ({ chromium } = await import(PLAYWRIGHT));
} catch {
  report.unavailable = true;
  done();
}

// openshell routes egress through a proxy named in the environment; local servers skip it
const proxy = process.env.HTTPS_PROXY || process.env.https_proxy || process.env.HTTP_PROXY || process.env.http_proxy || process.env.ALL_PROXY;
let browser;
try {
  browser = await chromium.launch({
    // the sandbox can't grant chromium's own sandbox the privileges it needs, and is the boundary
    // anyway; its seccomp filter also crashes the zygote chromium forks pages from
    args: ["--no-sandbox", "--no-zygote", "--disable-dev-shm-usage", "--disable-gpu"],
    proxy: proxy ? { server: proxy, bypass: "localhost,127.0.0.1,::1,[::1]" } : undefined,
  });
} catch (error) {
  report.error = `the browser didn't start: ${error.message.split("\n")[0]}`;
  done();
}
const mobile = request.viewport === "mobile";
const context = await browser.newContext({
  viewport: VIEWPORTS[mobile ? "mobile" : "desktop"],
  isMobile: mobile,
  hasTouch: mobile,
  // the proxy re-signs tls with a per-sandbox ca chromium doesn't know; the proxy checks upstream
  ignoreHTTPSErrors: true,
});
const page = await context.newPage();
const note = (list, text) => list.length < MAX_MESSAGES && list.push(text.slice(0, 500));
page.on("console", (message) => {
  if (message.type() === "error" || message.type() === "warning") note(report.console, `${message.type()}: ${message.text()}`);
});
page.on("pageerror", (error) => note(report.console, `uncaught: ${error.message}`));
page.on("requestfailed", (failed) => note(report.failed, `${failed.method()} ${failed.url()}: ${failed.failure()?.errorText ?? "failed"}`));
page.on("response", (response) => {
  if (response.status() >= 400) note(report.failed, `${response.request().method()} ${response.url()}: ${response.status()}`);
});

// `button "Sign in"` finds by role and name, as the outline lists them; css when it looks like css;
// otherwise visible text
function locate(target) {
  const role = /^([a-z]+) "(.*)"$/.exec(target);
  if (role) return page.getByRole(role[1], { name: role[2] }).first();
  if (/^(css=|[#.[]|\w+\[)/.test(target)) return page.locator(target.replace(/^css=/, "")).first();
  return page.getByText(target).first();
}

async function run(step) {
  const target = step.target ?? "";
  switch (step.action) {
    case "click":
      return locate(target).click({ timeout: STEP_TIMEOUT_MS });
    case "type":
      return locate(target).fill(step.text ?? "", { timeout: STEP_TIMEOUT_MS });
    case "press":
      return page.keyboard.press(step.text ?? target);
    case "wait_for_text":
      return page.getByText(step.text ?? target).first().waitFor({ timeout: STEP_TIMEOUT_MS });
    case "scroll":
      return page.mouse.wheel(0, Number(step.text) || 800);
    case "wait":
      return page.waitForTimeout(Math.min(Number(step.text) || 1000, STEP_TIMEOUT_MS));
    default:
      throw new Error(`unknown action ${step.action}`);
  }
}

try {
  await page.goto(request.url, { waitUntil: "load", timeout: NAVIGATION_TIMEOUT_MS });
  await page.waitForLoadState("networkidle", { timeout: SETTLE_MS }).catch(() => {});
  for (const step of request.steps ?? []) {
    try {
      await run(step);
      await page.waitForLoadState("networkidle", { timeout: SETTLE_MS }).catch(() => {});
      report.steps.push(`${step.action} ${step.target ?? step.text ?? ""}: ok`);
    } catch (error) {
      // the rest depend on this one, so the page is reported as it stands
      report.steps.push(`${step.action} ${step.target ?? step.text ?? ""}: failed, ${error.message.split("\n")[0]}`);
      break;
    }
  }
} catch (error) {
  report.error = error.message.split("\n")[0];
}

try {
  report.url = page.url();
  report.title = await page.title();
  const outline = await page.locator("body").ariaSnapshot({ timeout: STEP_TIMEOUT_MS });
  report.outline = outline.length > MAX_OUTLINE_CHARS ? `${outline.slice(0, MAX_OUTLINE_CHARS)}\n…` : outline;
  await mkdir(request.directory, { recursive: true });
  report.screenshot = `${request.directory}/${request.id}.jpg`;
  await page.screenshot({ path: report.screenshot, type: "jpeg", quality: 75, fullPage: false });
} catch (error) {
  report.error ??= error.message.split("\n")[0];
}
await browser.close();
done();
