import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const SKILL_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const RUN_DIR = path.join(SKILL_DIR, ".run");
export const STATE_PATH = path.join(RUN_DIR, "state.json");
export const ARTIFACTS_DIR = path.join(SKILL_DIR, "artifacts");
export const DEFAULT_PORT = 1422;

export function findRepo() {
  let dir = SKILL_DIR;
  for (let i = 0; i < 8; i++) {
    const pkgPath = path.join(dir, "package.json");
    if (existsSync(pkgPath)) {
      const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
      if (pkg.name === "pulse") return { root: dir, pkg };
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error("pulse repo root not found (walked up from the skill looking for package.json name pulse)");
}

export function readState() {
  if (!existsSync(STATE_PATH)) return null;
  return JSON.parse(readFileSync(STATE_PATH, "utf8"));
}

export function writeState(state) {
  mkdirSync(RUN_DIR, { recursive: true });
  writeFileSync(STATE_PATH, `${JSON.stringify(state, null, 2)}\n`);
}

export function clearRunDir() {
  rmSync(RUN_DIR, { recursive: true, force: true });
}

export function isPidAlive(pid) {
  if (!pid) return false;
  try {
    if (process.platform === "win32") {
      const out = execFileSync("tasklist", ["/FI", `PID eq ${pid}`, "/NH"], {
        encoding: "utf8",
        windowsHide: true,
      });
      return out.includes(String(pid));
    }
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export function killPidTree(pid) {
  if (!pid || !isPidAlive(pid)) return;
  if (process.platform === "win32") {
    try {
      execFileSync("taskkill", ["/PID", String(pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
    } catch {
      // already gone
    }
    return;
  }
  try {
    process.kill(-pid, "SIGTERM");
  } catch {
    try {
      process.kill(pid, "SIGTERM");
    } catch {
      // already gone
    }
  }
}

export function parsePort(argv = process.argv.slice(2)) {
  const i = argv.indexOf("--port");
  if (i !== -1) {
    const n = Number(argv[i + 1]);
    if (!Number.isInteger(n) || n < 1 || n > 65535) {
      throw new Error(`invalid --port ${argv[i + 1]}`);
    }
    return n;
  }
  return DEFAULT_PORT;
}

export function urlFor(port) {
  // Vite's default host is `localhost` (often ::1 on Windows). 127.0.0.1 is
  // a different socket and will hang while the mock is healthy.
  return `http://localhost:${port}/`;
}

export async function fetchText(url, timeoutMs = 2000) {
  const ac = new AbortController();
  const t = setTimeout(() => ac.abort(), timeoutMs);
  try {
    const res = await fetch(url, { signal: ac.signal });
    const body = await res.text();
    return { ok: res.ok, status: res.status, body };
  } finally {
    clearTimeout(t);
  }
}

export function htmlLooksLikePulseVite(body) {
  return (
    typeof body === "string" &&
    body.includes('id="root"') &&
    body.includes("/src/main.tsx")
  );
}
