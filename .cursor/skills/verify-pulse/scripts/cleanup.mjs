#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { ARTIFACTS_DIR, RUN_DIR, clearRunDir, isPidAlive, killPidTree, readState } from "./lib.mjs";

const chromeStatePath = path.join(RUN_DIR, "chrome.json");
if (existsSync(chromeStatePath)) {
  try {
    const chrome = JSON.parse(readFileSync(chromeStatePath, "utf8"));
    if (chrome.pid && isPidAlive(chrome.pid)) {
      killPidTree(chrome.pid);
      console.log(`stopped chrome pid=${chrome.pid}`);
    }
  } catch {
    // ignore a half-written chrome.json
  }
}

const state = readState();
if (!state) {
  console.log("nothing to clean");
  console.log(`artifacts=${ARTIFACTS_DIR} (untouched)`);
  process.exit(0);
}

if (isPidAlive(state.pid)) {
  killPidTree(state.pid);
  const started = Date.now();
  while (isPidAlive(state.pid) && Date.now() - started < 5000) {
    await new Promise((r) => setTimeout(r, 100));
  }
  if (isPidAlive(state.pid)) {
    console.error(`pid ${state.pid} still alive after taskkill — not deleting .run so a later cleanup can retry`);
    process.exit(1);
  }
  console.log(`stopped pid=${state.pid} port=${state.port}`);
} else {
  console.log(`pid ${state.pid} already gone`);
}

clearRunDir();
console.log("removed .run/");
console.log(`artifacts=${ARTIFACTS_DIR} exists=${existsSync(ARTIFACTS_DIR)} (never deleted)`);
