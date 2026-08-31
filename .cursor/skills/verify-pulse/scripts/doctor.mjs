#!/usr/bin/env node
import {
  fetchText,
  findRepo,
  htmlLooksLikePulseVite,
  isPidAlive,
  readState,
} from "./lib.mjs";

function fail(reason) {
  console.error(`unhealthy: ${reason}`);
  process.exit(1);
}

const state = readState();
if (!state) {
  fail("no .run/state.json — this checkout has no verification instance. Run node .cursor/skills/verify-pulse/scripts/launch.mjs");
}

if (state.port === 1420) {
  fail("state.port is 1420 (tauri/vite default). Do not drive it. Cleanup and relaunch on 1422.");
}

if (!isPidAlive(state.pid)) {
  fail(`pid ${state.pid} is dead. Cleanup leftover state and launch again. Do not attach to some other listener on ${state.port}.`);
}

let page;
try {
  page = await fetchText(state.url, 2000);
} catch (err) {
  fail(`GET ${state.url} failed (${err instanceof Error ? err.message : err})`);
}

if (!page.ok) fail(`GET ${state.url} returned ${page.status}`);
if (!htmlLooksLikePulseVite(page.body)) {
  fail(`GET ${state.url} is not this repo's Vite index (expected id=root and /src/main.tsx)`);
}

const { pkg } = findRepo();
if (state.packageVersion && state.packageVersion !== pkg.version) {
  fail(`running package ${state.packageVersion} but checkout is ${pkg.version}`);
}

const title = (page.body.match(/<title>([^<]*)<\/title>/i) || [])[1] || "";

console.log("ok");
console.log(`url=${state.url}`);
console.log(`pid=${state.pid}`);
console.log(`port=${state.port}`);
console.log(`package=${pkg.version}`);
console.log(`title=${title}`);
console.log("html=pulse-vite");
