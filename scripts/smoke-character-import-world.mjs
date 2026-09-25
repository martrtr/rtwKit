import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn, spawnSync } from "node:child_process";

const RINTAWA_BIN = process.env.RINTAWA_BIN;
const WEB_RUNTIME_RTW = process.env.RINTAWA_WEB_RUNTIME_RTW;
const WEB_UI_RTW = process.env.RINTAWA_WEB_UI_RTW;
const CHARACTER_RTW = process.env.RINTAWA_CHARACTER_RTW;
const HOST = process.env.RINTAWA_SMOKE_HOST ?? "127.0.0.1:44719";
const WS_URL = `ws://${HOST}/__rintawa/ws`;
const CHARACTER_ID = "rintawa.character-library";
const IMPORT_ACTION = "rintawa.character-library.import-tavern-v2";
const INSTANTIATE_ACTION = "rintawa.character-library.instantiate";
const CHARACTER_CONTENT = "rintawa.character-template@1";

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

for (const [name, value] of [
  ["RINTAWA_BIN", RINTAWA_BIN],
  ["RINTAWA_WEB_RUNTIME_RTW", WEB_RUNTIME_RTW],
  ["RINTAWA_WEB_UI_RTW", WEB_UI_RTW],
  ["RINTAWA_CHARACTER_RTW", CHARACTER_RTW],
]) {
  if (!value) throw new Error(`${name} is required`);
}

function run(args) {
  const result = spawnSync(RINTAWA_BIN, args, {
    encoding: "utf8",
    env: process.env,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `rintawa ${args.join(" ")} failed (${result.status})\n${result.stdout}${result.stderr}`,
    );
  }
  return result.stdout;
}

function install(home, artifact) {
  const output = run(["--home", home, "install", artifact]);
  const values = Object.fromEntries(
    output
      .split("\n")
      .map((line) => line.match(/^([^=]+?) = (.+)$/))
      .filter(Boolean)
      .map((match) => [match[1].trim(), match[2].trim()]),
  );
  if (!values.subject || !values.instance) {
    throw new Error(`install output did not expose subject/instance\n${output}`);
  }
  return values;
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

function characterSurface(state, ownerInstance) {
  return state.surfaces.find(
    (surface) =>
      surface.owner.instance_id === ownerInstance &&
      surface.snapshot.surface_id === "rintawa.character-library.main",
  );
}

function nodeByButtonAction(surface, action) {
  return surface?.snapshot.nodes.find(
    (node) =>
      node.kind.type === "button" &&
      node.kind.data.action === action &&
      node.kind.data.is_enabled,
  );
}

function importTextArea(surface) {
  return surface?.snapshot.nodes.find(
    (node) =>
      node.kind.type === "text-area" &&
      node.kind.data.submit_action === IMPORT_ACTION &&
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
      // Host is still starting.
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
  child.stdout.on("data", (chunk) => process.stdout.write(chunk));
  child.stderr.on("data", (chunk) => process.stderr.write(chunk));
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
    let resolved = false;

    const fail = (error) => {
      for (const waiter of waiters) waiter.reject(error);
      waiters.clear();
      if (!resolved) reject(error);
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
        clearTimeout(waiter.timer);
        waiter.resolve(message);
      }
      if (!resolved) {
        resolved = true;
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
                timer: null,
              };
              waiter.timer = setTimeout(() => {
                if (!waiters.delete(waiter)) return;
                waitReject(new Error("timed out waiting for Portable UI state"));
              }, timeoutMs);
              waiters.add(waiter);
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
      }
    });

    socket.addEventListener("error", () => {
      fail(new Error("WebSocket transport failed"));
    });
  });
}

function parseWorldList(output) {
  return output
    .trim()
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => /^[0-9a-f-]{36} \d+$/.test(line))
    .map((line) => {
      const [id, position] = line.split(/\s+/);
      return { id, position: Number(position) };
    });
}

async function waitForCommittedWorld(home, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const worlds = parseWorldList(run(["--home", home, "world-list"]));
    const committed = worlds.find((world) => world.position >= 1);
    if (committed) return committed;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("timed out waiting for Character World commit");
}

const tavernFixture = JSON.stringify({
  spec: "chara_card_v2",
  spec_version: "2.0",
  data: {
    name: "Smoke Alice",
    description: "Persistent Character smoke fixture",
    personality: "Curious",
    scenario: "A platform integration test",
    first_mes: "Hello from persisted state.",
    mes_example: "<START>\nSmoke Alice: Hello.",
    creator_notes: "rtwKit smoke test",
    system_prompt: "Stay in character.",
    post_history_instructions: "Keep replies concise.",
    alternate_greetings: ["Hi."],
    tags: ["smoke"],
    creator: "rtwKit",
    character_version: "1",
    extensions: {},
  },
});

const root = await mkdtemp(join(tmpdir(), "rintawa-character-smoke-"));
const home = join(root, "home");
let host = null;
let ui = null;

try {
  const webRuntime = install(home, WEB_RUNTIME_RTW);
  const webUi = install(home, WEB_UI_RTW);
  const character = install(home, CHARACTER_RTW);

  if (character.subject !== CHARACTER_ID) {
    throw new Error(`unexpected Character package id: ${character.subject}`);
  }

  for (const permission of ["background-task", "loopback-listen"]) {
    grant(home, webUi.instance, "web", permission);
  }
  for (const permission of [
    "background-task",
    "user-content-read",
    "user-content-write",
    "world-session-write",
    "world-command-submit",
    "composition-read",
  ]) {
    grant(home, character.instance, "runtime", permission);
  }

  run(["--home", home, "world-default", CHARACTER_ID]);

  host = startHost(home);
  await waitForHttp(host);
  ui = await connectUi();

  let state = await ui.waitFor((message) => {
    const surface = characterSurface(message, character.instance);
    return Boolean(importTextArea(surface));
  });
  let surface = characterSurface(state, character.instance);
  let importNode = importTextArea(surface);
  ui.sendAction(surface, importNode, IMPORT_ACTION, {
    type: "text",
    value: tavernFixture,
  });

  state = await ui.waitFor((message) => {
    const candidate = characterSurface(message, character.instance);
    return textContent(candidate).includes("Imported Smoke Alice.");
  });
  surface = characterSurface(state, character.instance);
  if (!textContent(surface).includes("Smoke Alice")) {
    throw new Error("import succeeded but Character catalog did not render Smoke Alice");
  }

  const contentListing = run(["--home", home, "content-list"]);
  if (!contentListing.includes(CHARACTER_CONTENT)) {
    throw new Error(`CharacterTemplate was not persisted\n${contentListing}`);
  }

  const createWorld = nodeByButtonAction(surface, INSTANTIATE_ACTION);
  if (!createWorld) throw new Error("Character Create World action is unavailable");
  ui.sendAction(surface, createWorld, INSTANTIATE_ACTION, { type: "none" });

  const world = await waitForCommittedWorld(home);
  const worldInfo = run(["--home", home, "world-info", world.id]);
  if (!worldInfo.includes("commit_position = 1")) {
    throw new Error(`Character World did not commit exactly once\n${worldInfo}`);
  }
  const schemas = Number(worldInfo.match(/^schemas = (\d+)$/m)?.[1] ?? "0");
  if (schemas < 1) {
    throw new Error(`Character World did not persist schemas\n${worldInfo}`);
  }

  ui.socket.close();
  ui = null;
  await stopHost(host);
  host = null;

  host = startHost(home);
  await waitForHttp(host);
  const restartedUi = await connectUi();
  await restartedUi.waitFor((message) => {
    const candidate = characterSurface(message, character.instance);
    return Boolean(candidate) && textContent(candidate).includes("Smoke Alice");
  });
  restartedUi.socket.close();

  const persistedWorldInfo = run(["--home", home, "world-info", world.id]);
  if (!persistedWorldInfo.includes("commit_position = 1")) {
    throw new Error(`Character World commit was not stable across restart\n${persistedWorldInfo}`);
  }

  await stopHost(host);
  host = null;
  console.log(
    `PASS: Tavern JSON -> persistent CharacterTemplate -> World ${world.id} commit ` +
      "-> restart through real Portable UI and Core Host.",
  );
} finally {
  if (ui) ui.socket.close();
  await stopHost(host);
  await rm(root, { recursive: true, force: true });
}
