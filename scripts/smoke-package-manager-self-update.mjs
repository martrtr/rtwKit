#!/usr/bin/env node

import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn, spawnSync } from "node:child_process";

const REGISTRY_URL =
  process.env.RINTAWA_REGISTRY_URL ?? "https://martrtr.github.io/rtwKit/index.json";
const RINTAWA_BIN = process.env.RINTAWA_BIN;
const EXPECTED_PM_VERSION = process.env.RINTAWA_EXPECTED_PM_VERSION ?? null;
const REGISTRY_READY_TIMEOUT_MS = Number(
  process.env.RINTAWA_REGISTRY_READY_TIMEOUT_MS ?? "120000",
);
const REGISTRY_RETRY_MS = Number(process.env.RINTAWA_REGISTRY_RETRY_MS ?? "2000");
const PORT = Number(process.env.RINTAWA_SMOKE_PORT ?? "44719");
const HOST = `127.0.0.1:${PORT}`;
const WS_URL = `ws://${HOST}/__rintawa/ws`;
const PM_ID = "rintawa.package-manager";

const UI_CAPABILITIES = [
  "rintawa.ui.text@1",
  "rintawa.ui.markdown@1",
  "rintawa.ui.button@1",
  "rintawa.ui.icon@1",
  "rintawa.ui.image@1",
  "rintawa.ui.input.checkbox@1",
  "rintawa.ui.input.select@1",
  "rintawa.ui.input.text@1",
  "rintawa.ui.input.text-area@1",
  "rintawa.ui.layout.split@1",
  "rintawa.ui.layout.row@1",
  "rintawa.ui.layout.column@1",
  "rintawa.ui.list@1",
  "rintawa.ui.data-grid@1",
];

if (!RINTAWA_BIN) {
  throw new Error("RINTAWA_BIN must point to the Core rintawa executable");
}

function run(args, options = {}) {
  const result = spawnSync(RINTAWA_BIN, args, {
    encoding: "utf8",
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(
      `${RINTAWA_BIN} ${args.join(" ")} failed\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
    );
  }
  return result.stdout;
}

async function download(url, path) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) {
    throw new Error(`download failed: ${response.status} ${url}`);
  }
  await writeFile(path, new Uint8Array(await response.arrayBuffer()));
}

function packageById(index, id) {
  const pkg = index.packages.find((candidate) => candidate.id === id);
  if (!pkg) throw new Error(`registry package not found: ${id}`);
  return pkg;
}

async function fetchReadyRegistry() {
  const deadline = Date.now() + REGISTRY_READY_TIMEOUT_MS;
  let observedVersion = null;
  let lastError = null;

  while (Date.now() < deadline) {
    try {
      const url = new URL(REGISTRY_URL);
      url.searchParams.set("smoke", String(Date.now()));
      const response = await fetch(url, { cache: "no-store" });
      if (!response.ok) {
        throw new Error(`registry fetch failed: ${response.status}`);
      }
      const registry = await response.json();
      observedVersion = packageById(registry, PM_ID).latest;
      if (!EXPECTED_PM_VERSION || observedVersion === EXPECTED_PM_VERSION) {
        return registry;
      }
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, REGISTRY_RETRY_MS));
  }

  const detail = lastError ? `; last error: ${lastError.message}` : "";
  throw new Error(
    `registry did not expose expected Package Manager ${EXPECTED_PM_VERSION}; latest observed: ${observedVersion}${detail}`,
  );
}

function releaseByVersion(pkg, version) {
  const release = pkg.versions.find((candidate) => candidate.version === version);
  if (!release) throw new Error(`release ${pkg.id} ${version} not found`);
  return release;
}

function latestRelease(pkg) {
  return releaseByVersion(pkg, pkg.latest);
}

function previousRelease(pkg) {
  const latestIndex = pkg.versions.findIndex(
    (release) => release.version === pkg.latest,
  );
  if (latestIndex < 0) {
    throw new Error(`registry latest release is missing for ${pkg.id}: ${pkg.latest}`);
  }
  return pkg.versions[latestIndex + 1] ?? null;
}

function grant(home, instance, component, permission) {
  run([
    "--home",
    home,
    "grant-runtime",
    instance,
    component,
    permission,
  ]);
}

function presentationSurface(state) {
  return state.surfaces.find((surface) => surface.owner.instance_id === PM_ID);
}

function nodeByAction(surface, action) {
  return surface?.snapshot.nodes.find(
    (node) =>
      node.kind.type === "button" &&
      node.kind.data.action === action &&
      node.kind.data.is_enabled,
  );
}

function textContent(surface) {
  return (
    surface?.snapshot.nodes
      .flatMap((node) => {
        if (node.kind.type === "text") return [node.kind.data.text];
        if (node.kind.type === "markdown") return [node.kind.data.source];
        return [];
      })
      .join("\n") ?? ""
  );
}

async function waitForHttp(process, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (process.exitCode !== null) {
      throw new Error(`rintawa exited before Web UI became ready: ${process.exitCode}`);
    }
    try {
      const response = await fetch(`http://${HOST}/`);
      if (response.ok) return;
    } catch {
      // Startup is still in progress.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("timed out waiting for Web UI");
}

function startHost(home) {
  const child = spawn(RINTAWA_BIN, ["--home", home, "run"], {
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? "info" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk) => {
    process.stdout.write(chunk);
  });
  child.stderr.on("data", (chunk) => {
    process.stderr.write(chunk);
  });
  return child;
}

async function stopHost(child) {
  if (!child || child.exitCode !== null) return;
  child.kill("SIGINT");
  await new Promise((resolve) => child.once("exit", resolve));
}

function connectUi() {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(WS_URL);
    let currentState = null;
    const waiters = new Set();

    const fail = (error) => {
      for (const waiter of waiters) waiter.reject(error);
      waiters.clear();
      reject(error);
    };

    socket.addEventListener(
      "open",
      () => {
        socket.send(
          JSON.stringify({
            type: "hello",
            protocol_major: 1,
            portable_ui_protocol_major: 1,
            capabilities: UI_CAPABILITIES,
          }),
        );
      },
      { once: true },
    );

    socket.addEventListener("message", (event) => {
      const message = JSON.parse(String(event.data));
      if (message.type === "error") {
        fail(new Error(`${message.code}: ${message.message}`));
        return;
      }
      if (message.type !== "state") return;
      currentState = message;
      for (const waiter of [...waiters]) {
        if (!waiter.predicate(message)) continue;
        waiters.delete(waiter);
        waiter.resolve(message);
      }
      resolve({
        socket,
        latest: () => currentState,
        waitFor(predicate, timeoutMs = 30_000) {
          if (currentState && predicate(currentState)) {
            return Promise.resolve(currentState);
          }
          return new Promise((waitResolve, waitReject) => {
            const waiter = {
              predicate,
              resolve: waitResolve,
              reject: waitReject,
            };
            waiters.add(waiter);
            const timer = setTimeout(() => {
              if (!waiters.delete(waiter)) return;
              waitReject(new Error("timed out waiting for Portable UI state"));
            }, timeoutMs);
            const originalResolve = waiter.resolve;
            waiter.resolve = (value) => {
              clearTimeout(timer);
              originalResolve(value);
            };
          });
        },
        sendAction(surface, node, action, payload) {
          socket.send(
            JSON.stringify({
              type: "action",
              protocol_major: 1,
              event: {
                owner_instance_id: surface.owner.instance_id,
                surface_id: surface.snapshot.surface_id,
                node_id: node.id,
                action_id: action,
                surface_revision: surface.snapshot.revision,
                payload,
              },
            }),
          );
        },
      });
    });

    socket.addEventListener("error", () => {
      fail(new Error("WebSocket transport failed"));
    });
  });
}

const root = await mkdtemp(join(tmpdir(), "rintawa-package-manager-smoke-"));
const home = join(root, "home");
const artifacts = join(root, "artifacts");
await mkdir(home, { recursive: true });
await mkdir(artifacts, { recursive: true });

let host = null;
try {
  const registry = await fetchReadyRegistry();

  const packageManager = packageById(registry, PM_ID);
  const previousPackageManager = previousRelease(packageManager);
  if (!previousPackageManager) {
    console.log("SKIP: Package Manager has no previous release to exercise self-update.");
    await rm(root, { recursive: true, force: true });
    process.exit(0);
  }
  const latestPackageManager = latestRelease(packageManager);
  const webRuntime = latestRelease(packageById(registry, "rintawa.web-runtime"));
  const webUi = latestRelease(packageById(registry, "rintawa.web-ui"));

  const selected = [
    ["web-runtime.rtw", webRuntime],
    ["web-ui.rtw", webUi],
    ["package-manager.rtw", previousPackageManager],
  ];
  for (const [name, release] of selected) {
    const path = join(artifacts, name);
    await download(release.artifact.url, path);
    run(["--home", home, "install", path]);
  }

  for (const permission of ["background-task", "loopback-listen"]) {
    grant(home, "rintawa.web-ui", "web", permission);
  }
  for (const permission of [
    "background-task",
    "http-fetch",
    "artifact-import",
    "composition-read",
    "composition-write",
    "runtime-policy-read",
    "runtime-policy-write",
  ]) {
    grant(home, PM_ID, "runtime", permission);
  }

  host = startHost(home);
  await waitForHttp(host);
  const ui = await connectUi();
  let state = await ui.waitFor((message) => {
    const surface = presentationSurface(message);
    return Boolean(surface) && textContent(surface).includes("Loaded ");
  });
  let surface = presentationSurface(state);

  const grid = surface.snapshot.nodes.find(
    (node) =>
      node.kind.type === "data-grid" &&
      node.kind.data.row_action === "installed.row-select",
  );
  if (!grid || grid.kind.type !== "data-grid") {
    throw new Error("Package Manager installed grid is unavailable");
  }
  ui.sendAction(surface, grid, grid.kind.data.row_action, {
    type: "text",
    value: PM_ID,
  });

  state = await ui.waitFor((message) => {
    const candidate = presentationSurface(message);
    return Boolean(nodeByAction(candidate, "package.update"));
  });
  surface = presentationSurface(state);
  const update = nodeByAction(surface, "package.update");
  ui.sendAction(surface, update, "package.update", { type: "none" });

  state = await ui.waitFor((message) => {
    const candidate = presentationSurface(message);
    return Boolean(nodeByAction(candidate, "install.confirm"));
  });
  surface = presentationSurface(state);
  const confirm = nodeByAction(surface, "install.confirm");
  ui.sendAction(surface, confirm, "install.confirm", { type: "none" });

  const expectedStatus = `Package Manager ${latestPackageManager.version}`;
  await ui.waitFor((message) => {
    const candidate = presentationSurface(message);
    return textContent(candidate).includes(expectedStatus);
  });
  ui.socket.close();

  await stopHost(host);
  host = null;

  const listing = run(["--home", home, "list"]);
  const expectedDigest = latestPackageManager.artifact.sha256;
  if (
    !listing.includes(
      `${PM_ID} ${latestPackageManager.version}`,
    ) ||
    !listing.includes(expectedDigest)
  ) {
    throw new Error(
      `self-update did not select exact latest Package Manager artifact\n${listing}`,
    );
  }

  host = startHost(home);
  await waitForHttp(host);
  const restartedUi = await connectUi();
  await restartedUi.waitFor((message) => {
    const surface = presentationSurface(message);
    return Boolean(surface) && textContent(surface).includes("Loaded ");
  });
  restartedUi.socket.close();
  await stopHost(host);
  host = null;

  console.log(
    `PASS: Package Manager ${previousPackageManager.version} -> ${latestPackageManager.version} through real WebSocket UI, exact persisted digest, and restart.`,
  );
} finally {
  await stopHost(host);
  await rm(root, { recursive: true, force: true });
}
