#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync, readFileSync } from "node:fs";
import path from "node:path";
import { connectCdp } from "./cdp.mjs";
import {
  ARTIFACTS_DIR,
  RUN_DIR,
  fetchText,
  isPidAlive,
  killPidTree,
  readState,
} from "./lib.mjs";

const CHROME_PATHS = [
  process.env.VERIFY_PULSE_CHROME,
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
  "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
].filter(Boolean);

const CHROME_JSON = path.join(RUN_DIR, "chrome.json");
const CHROME_PORT = 9333;

function chromeBin() {
  for (const p of CHROME_PATHS) {
    if (p && existsSync(p)) return p;
  }
  throw new Error(
    "Chrome or Edge not found. Set VERIFY_PULSE_CHROME to the browser exe. cursor-ide-browser is the other harness.",
  );
}

function readChrome() {
  if (!existsSync(CHROME_JSON)) return null;
  return JSON.parse(readFileSync(CHROME_JSON, "utf8"));
}

function writeChrome(state) {
  mkdirSync(RUN_DIR, { recursive: true });
  writeFileSync(CHROME_JSON, `${JSON.stringify(state, null, 2)}\n`);
}

async function waitDevtools(port) {
  const started = Date.now();
  while (Date.now() - started < 20_000) {
    try {
      const page = await fetchText(`http://127.0.0.1:${port}/json/version`, 800);
      if (page.ok) return JSON.parse(page.body);
    } catch {
      // booting
    }
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(`chrome DevTools on 127.0.0.1:${port} did not answer`);
}

async function pageWs(port) {
  const list = await fetchText(`http://127.0.0.1:${port}/json/list`, 1500);
  if (!list.ok) throw new Error(`chrome /json/list ${list.status}`);
  const pages = JSON.parse(list.body);
  const page = pages.find((p) => p.type === "page") || pages[0];
  if (!page?.webSocketDebuggerUrl) throw new Error("chrome has no page target");
  return page.webSocketDebuggerUrl;
}

async function withPage(fn) {
  const chrome = readChrome();
  if (!chrome || !isPidAlive(chrome.pid)) {
    throw new Error("no chrome session — run node .cursor/skills/verify-pulse/scripts/chrome-session.mjs start");
  }
  const ws = await pageWs(chrome.port);
  const cdp = await connectCdp(ws);
  try {
    return await fn(cdp);
  } finally {
    cdp.close();
  }
}

async function start() {
  const pulse = readState();
  if (!pulse || !isPidAlive(pulse.pid)) {
    throw new Error("verification Vite is not up — run launch.mjs first");
  }
  const existing = readChrome();
  if (existing && isPidAlive(existing.pid)) {
    throw new Error(`chrome already running pid=${existing.pid} — stop it first`);
  }
  const bin = chromeBin();
  const profile = path.join(RUN_DIR, "chrome-profile");
  mkdirSync(profile, { recursive: true });
  const child = spawn(
    bin,
    [
      `--remote-debugging-port=${CHROME_PORT}`,
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "--window-size=1280,800",
      "--headless=new",
      pulse.url,
    ],
    { detached: true, stdio: "ignore", windowsHide: true },
  );
  if (!child.pid) throw new Error("failed to spawn chrome");
  child.unref();
  writeChrome({ pid: child.pid, port: CHROME_PORT, bin, startedAt: new Date().toISOString() });
  await waitDevtools(CHROME_PORT);
  const started = Date.now();
  let ready = false;
  while (Date.now() - started < 8000) {
    ready = await evaluate(
      `document.body && (document.body.innerText.includes("Savior") || document.body.innerText.includes("Nothing playing") || document.body.innerText.includes("Play something"))`,
    );
    if (ready) break;
    await new Promise((r) => setTimeout(r, 150));
  }
  if (!ready) throw new Error("chrome opened but Pulse UI did not hydrate (no Savior / Nothing playing / Play something)");
  console.log(`chrome ready pid=${child.pid} port=${CHROME_PORT}`);
}

function stop() {
  const chrome = readChrome();
  if (!chrome) {
    console.log("no chrome session");
    return;
  }
  if (isPidAlive(chrome.pid)) {
    killPidTree(chrome.pid);
    console.log(`stopped chrome pid=${chrome.pid}`);
  } else {
    console.log(`chrome pid ${chrome.pid} already gone`);
  }
  if (existsSync(CHROME_JSON)) {
    writeChrome({ ...chrome, pid: 0, stoppedAt: new Date().toISOString() });
  }
}

async function goto(url) {
  await withPage(async (cdp) => {
    await cdp.send("Page.enable");
    await cdp.send("Page.navigate", { url });
  });
  const started = Date.now();
  while (Date.now() - started < 8000) {
    const ready = await evaluate(
      `document.body && (document.body.innerText.includes("Savior") || document.body.innerText.includes("Nothing playing") || document.body.innerText.includes("Play something"))`,
    );
    if (ready) break;
    await new Promise((r) => setTimeout(r, 150));
  }
  console.log(`goto ${url}`);
}

async function evaluate(js) {
  return withPage(async (cdp) => {
    const r = await cdp.send("Runtime.evaluate", {
      expression: js,
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.exceptionDetails) {
      throw new Error(r.exceptionDetails.text || "evaluate threw");
    }
    return r.result?.value;
  });
}

async function reveal() {
  const hot = await evaluate(`
    (() => {
      const slider = document.querySelector('[aria-label="Track position"]');
      const root =
        slider?.closest("[class*='group']") ||
        document.querySelector("[class*='group/widget']") ||
        document.body;
      root.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientX: 20, clientY: 20 }));
      return root.hasAttribute("data-hot");
    })()
  `);
  console.log(`reveal data-hot=${hot}`);
}

async function clickName(name) {
  const found = await evaluate(`
    (() => {
      const nodes = document.querySelectorAll("button, [role='button']");
      for (const n of nodes) {
        const label = n.getAttribute("aria-label") || n.getAttribute("title") || n.textContent.trim();
        if (label === ${JSON.stringify(name)}) {
          n.click();
          return label;
        }
      }
      return null;
    })()
  `);
  if (!found) throw new Error(`no button named ${name}`);
  await new Promise((r) => setTimeout(r, 250));
  console.log(`clicked ${found}`);
}

async function snapshot(outPath) {
  const tree = await evaluate(`
    (() => {
      const lines = [];
      const walk = (el, depth) => {
        if (!el || el.nodeType !== 1) return;
        const label = el.getAttribute("aria-label") || el.getAttribute("role") || "";
        const text = (el.innerText || "").split("\\n").map((s) => s.trim()).filter(Boolean)[0] || "";
        if (el.getAttribute("aria-label") || el.getAttribute("role") === "slider" || el.tagName === "BUTTON") {
          lines.push(
            \`\${"  ".repeat(depth)}\${el.tagName.toLowerCase()}\${el.getAttribute("role") ? \` role=\${el.getAttribute("role")}\` : ""}\${el.getAttribute("aria-label") ? \` name=\${JSON.stringify(el.getAttribute("aria-label"))}\` : ""}\${el.getAttribute("aria-disabled") ? " disabled" : ""}\${text && !el.getAttribute("aria-label") ? \` text=\${JSON.stringify(text)}\` : ""}\`,
          );
        }
        for (const c of el.children) walk(c, depth + 1);
      };
      const title = document.title;
      const body = (document.body.innerText || "").replace(/\\s+/g, " ").trim();
      walk(document.body, 0);
      return { title, url: location.href, body, aria: lines };
    })()
  `);
  const dest = path.isAbsolute(outPath) ? outPath : path.join(process.cwd(), outPath);
  mkdirSync(path.dirname(dest), { recursive: true });
  const text = [
    `url=${tree.url}`,
    `title=${tree.title}`,
    `body=${tree.body}`,
    "aria:",
    ...(tree.aria || []),
    "",
  ].join("\n");
  writeFileSync(dest, text);
  console.log(`wrote ${dest}`);
}

async function screenshot(outPath) {
  const dest = path.isAbsolute(outPath) ? outPath : path.join(process.cwd(), outPath);
  mkdirSync(path.dirname(dest), { recursive: true });
  await withPage(async (cdp) => {
    const r = await cdp.send("Page.captureScreenshot", { format: "png" });
    writeFileSync(dest, Buffer.from(r.data, "base64"));
  });
  console.log(`wrote ${dest}`);
}

async function resetStorage() {
  await evaluate(`
    localStorage.setItem("pulse.mode", "card");
    localStorage.removeItem("pulse.expandedView");
    location.reload();
  `);
  await new Promise((r) => setTimeout(r, 500));
  console.log("reset pulse.mode=card and reloaded");
}

const [cmd, ...rest] = process.argv.slice(2);

try {
  switch (cmd) {
    case "start":
      await start();
      break;
    case "stop":
      stop();
      break;
    case "goto":
      if (!rest[0]) throw new Error("usage: chrome-session.mjs goto <url>");
      await goto(rest[0]);
      break;
    case "reveal":
      await reveal();
      break;
    case "click":
      if (!rest[0]) throw new Error("usage: chrome-session.mjs click <aria-label>");
      await clickName(rest.join(" "));
      break;
    case "snapshot":
      await snapshot(rest[0] || path.join(ARTIFACTS_DIR, "snapshot.aria.txt"));
      break;
    case "screenshot":
      await screenshot(rest[0] || path.join(ARTIFACTS_DIR, "snapshot.png"));
      break;
    case "reset":
      await resetStorage();
      break;
    case "eval":
      console.log(await evaluate(rest.join(" ")));
      break;
    default:
      console.error(
        "usage: chrome-session.mjs start|stop|goto|reveal|click|snapshot|screenshot|reset|eval",
      );
      process.exit(1);
  }
} catch (err) {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
}
