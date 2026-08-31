#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, openSync } from "node:fs";
import { createConnection } from "node:net";
import path from "node:path";
import {
  RUN_DIR,
  fetchText,
  findRepo,
  htmlLooksLikePulseVite,
  isPidAlive,
  killPidTree,
  parsePort,
  readState,
  urlFor,
  writeState,
} from "./lib.mjs";

const READY_MS = 45_000;
const POLL_MS = 250;

function portAccepts(host, port) {
  return new Promise((resolve) => {
    const sock = createConnection({ host, port }, () => {
      sock.end();
      resolve(true);
    });
    sock.setTimeout(400, () => {
      sock.destroy();
      resolve(false);
    });
    sock.on("error", () => resolve(false));
  });
}

async function portInUse(port) {
  return (await portAccepts("localhost", port)) || (await portAccepts("127.0.0.1", port));
}

async function waitReady(port) {
  const started = Date.now();
  const url = urlFor(port);
  while (Date.now() - started < READY_MS) {
    if (await portInUse(port)) {
      try {
        const page = await fetchText(url, 1500);
        if (page.ok && htmlLooksLikePulseVite(page.body)) return;
      } catch {
        // still booting
      }
    }
    await new Promise((r) => setTimeout(r, POLL_MS));
  }
  throw new Error(`vite did not become ready on ${url} within ${READY_MS}ms — see ${path.join(RUN_DIR, "vite.log")}`);
}

const port = parsePort();
const { root, pkg } = findRepo();
const existing = readState();

if (existing && isPidAlive(existing.pid)) {
  console.error(
    `already running pid=${existing.pid} url=${existing.url} — refuse to double-drive. Run node .cursor/skills/verify-pulse/scripts/cleanup.mjs first.`,
  );
  process.exit(1);
}

if (port === 1420) {
  console.error(
    "refusing port 1420 — that is Vite's and `tauri dev`'s default. Use 1422 (the verification default) or another free port.",
  );
  process.exit(1);
}

if (await portInUse(port)) {
  console.error(
    `port ${port} is already in use. Pick another --port, or stop the listener you started. Never steal a port from tauri dev or the user's Pulse.`,
  );
  process.exit(1);
}

if (!existsSync(path.join(root, "node_modules", "vite"))) {
  console.error(`vite is not installed in ${root} — run npm install from the repo root, then launch again.`);
  process.exit(1);
}

mkdirSync(RUN_DIR, { recursive: true });
const logPath = path.join(RUN_DIR, "vite.log");
const logFd = openSync(logPath, "a");
// Same entry `npm run dev` uses (package.json → vite). Spawn the binary
// through this node so Windows does not need cmd.exe / npm.cmd, which drop
// inherited log fds when detached.
const viteJs = path.join(root, "node_modules", "vite", "bin", "vite.js");
const child = spawn(process.execPath, [viteJs], {
  cwd: root,
  env: { ...process.env, PORT: String(port) },
  detached: true,
  stdio: ["ignore", logFd, logFd],
  windowsHide: true,
});
if (!child.pid) {
  console.error("failed to spawn npm run dev");
  process.exit(1);
}

const url = urlFor(port);
writeState({
  pid: child.pid,
  port,
  url,
  startedAt: new Date().toISOString(),
  repo: root,
  packageVersion: pkg.version,
  log: logPath,
});
child.unref();

try {
  await waitReady(port);
} catch (err) {
  killPidTree(child.pid);
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
}

console.log(`ready ${url}`);
console.log(`pid=${child.pid}`);
console.log(`port=${port}`);
console.log(`package=${pkg.version}`);
