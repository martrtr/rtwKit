import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn, spawnSync } from "node:child_process";

const RINTAWA_BIN = process.env.RINTAWA_BIN;
const WEB_RUNTIME_RTW = process.env.RINTAWA_WEB_RUNTIME_RTW;
const WEB_UI_RTW = process.env.RINTAWA_WEB_UI_RTW;
const WORLD_MANAGER_RTW = process.env.RINTAWA_WORLD_MANAGER_RTW;
const PACKAGE_MANAGER_RTW = process.env.RINTAWA_PACKAGE_MANAGER_RTW;
const CHARACTER_RTW = process.env.RINTAWA_CHARACTER_RTW;
const CHAT_RTW = process.env.RINTAWA_CHAT_RTW;
const HOST = process.env.RINTAWA_SMOKE_HOST ?? "127.0.0.1:44719";
const WS_URL = `ws://${HOST}/__rintawa/ws`;
const WORLD_MANAGER_ID = "rintawa.world-manager";
const PACKAGE_MANAGER_ID = "rintawa.package-manager";
const CHARACTER_ID = "rintawa.character-library";
const CHAT_ID = "rintawa.chat";
const WORLD_REFRESH_ACTION = "rintawa.world-manager.refresh";
const WORLD_TOGGLE_ACTION = "rintawa.world-manager.toggle-active";
const WORLD_IMPORT_ACTION = "rintawa.world-manager.import-resource";
const CHARACTER_REFRESH_ACTION = "rintawa.character-library.refresh";
const CHARACTER_SELECT_WORLD_ACTION = "rintawa.character-library.select-world";
const CHARACTER_ADD_TO_WORLD_ACTION = "rintawa.character-library.add-to-world";
const PACKAGE_SCOPE_ACTION = "management.scope";
const CHAT_SEND_ACTION = "rintawa.chat.send";

const UI_CAPABILITIES = [
  "rintawa.ui.text@1",
  "rintawa.ui.markdown@1",
  "rintawa.ui.button@1",
  "rintawa.ui.icon@1",
  "rintawa.ui.image@1",
  "rintawa.ui.asset-image@1",
  "rintawa.ui.input.checkbox@1",
  "rintawa.ui.input.select@1",
  "rintawa.ui.input.text@1",
  "rintawa.ui.input.text-area@1",
  "rintawa.ui.input.asset@1",
  "rintawa.ui.input.resource@1",
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
  ["RINTAWA_WORLD_MANAGER_RTW", WORLD_MANAGER_RTW],
  ["RINTAWA_PACKAGE_MANAGER_RTW", PACKAGE_MANAGER_RTW],
  ["RINTAWA_CHARACTER_RTW", CHARACTER_RTW],
  ["RINTAWA_CHAT_RTW", CHAT_RTW],
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

function install(home, artifact, disabled = false) {
  const args = ["--home", home, "install", artifact];
  if (disabled) args.push("--disabled");
  const output = run(args);
  const values = Object.fromEntries(
    output
      .split("\n")
      .map((line) => line.match(/^([^=]+?) = (.+)$/))
      .filter(Boolean)
      .map((match) => [match[1].trim(), match[2].trim()]),
  );
  if (!values.subject || !values.instance) {
    throw new Error(
      `install output did not expose subject/instance\n${output}`,
    );
  }
  return values;
}

function grant(home, instance, component, permission) {
  run(["--home", home, "grant-runtime", instance, component, permission]);
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

function worldManagerSurface(state, ownerInstance) {
  return state.surfaces.find(
    (surface) =>
      surface.owner.instance_id === ownerInstance &&
      surface.snapshot.surface_id === "rintawa.world-manager.main",
  );
}

function nodeByAction(surface, action) {
  return surface?.snapshot.nodes.find((node) => {
    if (node.kind.type === "button") return node.kind.data.action === action;
    if (node.kind.type === "select")
      return node.kind.data.change_action === action;
    if (node.kind.type === "resource-picker")
      return node.kind.data.change_action === action;
    return false;
  });
}

function chatSurface(state) {
  return state.surfaces.find(
    (surface) => surface.snapshot.surface_id === "rintawa.chat.main",
  );
}

function enabledChatComposer(surface) {
  return surface?.snapshot.nodes.find(
    (node) =>
      node.kind.type === "text-area" &&
      node.kind.data.submit_action === CHAT_SEND_ACTION &&
      node.kind.data.is_enabled,
  );
}

function packageManagerSurface(state, ownerInstance) {
  return state.surfaces.find(
    (surface) =>
      surface.owner.instance_id === ownerInstance &&
      surface.snapshot.surface_id === "package-manager.main",
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
      throw new Error(
        `rintawa exited before Web UI became ready: ${process.exitCode}`,
      );
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
                const summary = currentState?.surfaces?.map((surface) => ({
                  owner: surface.owner.instance_id,
                  id: surface.snapshot.surface_id,
                  revision: surface.snapshot.revision,
                  nodes: surface.snapshot.nodes.length,
                  text: textContent(surface).slice(0, 800),
                }));
                waitReject(
                  new Error(
                    `timed out waiting for Portable UI state; current surfaces=${JSON.stringify(summary)}`,
                  ),
                );
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

async function waitForCommittedWorld(
  home,
  expectedWorldId,
  minimumPosition = 1,
  timeoutMs = 30_000,
) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const worlds = parseWorldList(run(["--home", home, "world-list"]));
    const committed = worlds.find(
      (world) =>
        world.id === expectedWorldId && world.position >= minimumPosition,
    );
    if (committed) return committed;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    `timed out waiting for Character World ${expectedWorldId} position ${minimumPosition}`,
  );
}

function parseCreatedWorld(output) {
  const worldId = output.match(/^world = ([0-9a-f-]{36})$/m)?.[1];
  if (!worldId)
    throw new Error(`world-create did not expose a World id
${output}`);
  return worldId;
}

async function waitForWorldActive(
  ui,
  worldManagerInstance,
  timeoutMs = 30_000,
) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const surface = worldManagerSurface(ui.latest(), worldManagerInstance);
    const toggle = nodeByAction(surface, WORLD_TOGGLE_ACTION);
    if (toggle?.kind.type === "button" && toggle.kind.data.label === "Stop")
      return surface;
    const refresh = nodeByButtonAction(surface, WORLD_REFRESH_ACTION);
    if (refresh)
      ui.sendAction(surface, refresh, WORLD_REFRESH_ACTION, { type: "none" });
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    "timed out waiting for World Manager to observe active World",
  );
}

const root = await mkdtemp(join(tmpdir(), "rintawa-character-smoke-"));
const home = join(root, "home");
let host = null;
let ui = null;

try {
  install(home, WEB_RUNTIME_RTW);
  const webUi = install(home, WEB_UI_RTW);
  const worldManager = install(home, WORLD_MANAGER_RTW);
  const packageManager = install(home, PACKAGE_MANAGER_RTW);
  const character = install(home, CHARACTER_RTW);
  const chat = install(home, CHAT_RTW, true);

  if (worldManager.subject !== WORLD_MANAGER_ID) {
    throw new Error(
      `unexpected World Manager package id: ${worldManager.subject}`,
    );
  }
  if (packageManager.subject !== PACKAGE_MANAGER_ID) {
    throw new Error(
      `unexpected Package Manager package id: ${packageManager.subject}`,
    );
  }
  if (character.subject !== CHARACTER_ID) {
    throw new Error(`unexpected Character package id: ${character.subject}`);
  }
  if (chat.subject !== CHAT_ID) {
    throw new Error(`unexpected Chat package id: ${chat.subject}`);
  }

  for (const permission of [
    "asset-import",
    "background-task",
    "loopback-listen",
    "user-resource-import",
  ]) {
    grant(home, webUi.instance, "web", permission);
  }
  for (const permission of [
    "background-task",
    "world-session-read",
    "world-session-write",
  ]) {
    grant(home, worldManager.instance, "runtime", permission);
  }
  for (const permission of [
    "background-task",
    "http-fetch",
    "artifact-import",
    "user-resource-read",
    "composition-read",
    "composition-write",
    "runtime-policy-read",
    "runtime-policy-write",
    "world-session-read",
  ]) {
    grant(home, packageManager.instance, "runtime", permission);
  }
  for (const permission of [
    "background-task",
    "world-command-submit",
    "world-projection-read",
  ]) {
    grant(home, chat.instance, "runtime", permission);
  }

  for (const permission of [
    "background-task",
    "user-content-read",
    "user-content-write",
    "user-resource-read",
    "asset-import",
    "world-session-read",
    "world-session-write",
    "world-command-submit",
    "composition-read",
  ]) {
    grant(home, character.instance, "runtime", permission);
  }

  run(["--home", home, "world-default", CHARACTER_ID]);
  run(["--home", home, "world-default", CHAT_ID]);
  const worldId = parseCreatedWorld(run(["--home", home, "world-create"]));

  host = startHost(home);
  await waitForHttp(host);
  ui = await connectUi();

  let state = await ui.waitFor((message) => {
    const surface = worldManagerSurface(message, worldManager.instance);
    return Boolean(nodeByAction(surface, WORLD_IMPORT_ACTION));
  });
  let worldSurface = worldManagerSurface(state, worldManager.instance);
  const importPicker = nodeByAction(worldSurface, WORLD_IMPORT_ACTION);
  if (importPicker?.kind.type !== "resource-picker") {
    throw new Error(
      "World Manager did not expose Character world creator picker",
    );
  }
  const directCard = Buffer.from(
    JSON.stringify({
      spec: "chara_card_v3",
      spec_version: "3.0",
      data: {
        name: "Direct V3 Alice",
        description: "Created from the World Manager file picker",
        personality: "Curious",
        scenario: "A clean import smoke test",
        first_mes: "Hello from V3.",
        mes_example: "",
        creator_notes: "",
        system_prompt: "",
        post_history_instructions: "",
        alternate_greetings: [],
        tags: ["smoke"],
        creator: "rtwKit smoke",
        character_version: "1",
        extensions: {},
      },
    }),
  );
  const importedResourceResponse = await fetch(
    `http://${HOST}/__rintawa/import-resource`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Rintawa-Resource-Name": "direct-v3-alice.json",
      },
      body: directCard,
    },
  );
  if (!importedResourceResponse.ok) {
    throw new Error(
      `resource ingress failed: HTTP ${importedResourceResponse.status}`,
    );
  }
  const resource = await importedResourceResponse.json();
  console.log("SMOKE stage: submit direct V3 World import");
  ui.sendAction(worldSurface, importPicker, WORLD_IMPORT_ACTION, {
    type: "resource",
    value: resource,
  });
  state = await ui.waitFor((message) =>
    textContent(worldManagerSurface(message, worldManager.instance)).includes(
      "Direct V3 Alice",
    ),
  );
  if (
    !textContent(worldManagerSurface(state, worldManager.instance)).includes(
      "Direct V3 Alice",
    )
  ) {
    throw new Error("direct V3 import did not create a titled World");
  }
  const importedWorlds = parseWorldList(run(["--home", home, "world-list"]));
  const directWorld = importedWorlds.find((world) => world.id !== worldId);
  if (!directWorld) {
    throw new Error("direct V3 import did not persist a distinct World");
  }
  await waitForCommittedWorld(home, directWorld.id, 2);
  const directWorldCommitted = await waitForCommittedWorld(home, directWorld.id);
  if (directWorldCommitted.position < 2) {
    const directWorldInfo = run(["--home", home, "world-info", directWorld.id]);
    throw new Error(
      `direct V3 World did not commit Character + Chat bootstrap\n${directWorldInfo}`,
    );
  }
  console.log("SMOKE stage: wait for imported World Chat bootstrap");
  await ui.waitFor((message) => {
    const chat = chatSurface(message);
    return Boolean(chat) && textContent(chat).includes("Hello from V3.");
  });
  await new Promise((resolve) => setTimeout(resolve, 1_000));
  const diagnosticChat = chatSurface(ui.latest());
  const participantToggle = nodeByAction(
    diagnosticChat,
    "rintawa.chat.toggle-participants",
  );
  console.log(
    "SMOKE Chat bootstrap diagnostic:",
    JSON.stringify({
      world: parseWorldList(run(["--home", home, "world-list"])).find(
        (candidate) => candidate.id === directWorld.id,
      ),
      participantLabel:
        participantToggle?.kind.type === "button"
          ? participantToggle.kind.data.label
          : null,
      composerEnabled: Boolean(enabledChatComposer(diagnosticChat)),
      textAreas: diagnosticChat?.snapshot.nodes
        .filter((node) => node.kind.type === "text-area")
        .map((node) => ({ id: node.id, data: node.kind.data })) ?? [],
    }),
  );
  await ui.waitFor((message) => Boolean(enabledChatComposer(chatSurface(message))));

  const worldScopeId = `world:${worldId}`;
  state = await ui.waitFor((message) => {
    const worlds = worldManagerSurface(message, worldManager.instance);
    const characters = characterSurface(message, character.instance);
    const extensions = packageManagerSurface(message, packageManager.instance);
    const scope = nodeByAction(extensions, PACKAGE_SCOPE_ACTION);
    const hasWorldScope =
      scope?.kind.type === "select" &&
      scope.kind.data.options.some((option) => option.value === worldScopeId);
    return (
      Boolean(worlds) &&
      Boolean(extensions) &&
      hasWorldScope &&
      Boolean(characters) &&
      textContent(characters).includes("Direct V3 Alice")
    );
  });
  worldSurface = worldManagerSurface(state, worldManager.instance);
  const startWorld = nodeByButtonAction(worldSurface, WORLD_TOGGLE_ACTION);
  if (!startWorld) throw new Error("World Manager Start action is unavailable");
  console.log("SMOKE stage: start existing World");
  ui.sendAction(worldSurface, startWorld, WORLD_TOGGLE_ACTION, {
    type: "none",
  });
  await waitForWorldActive(ui, worldManager.instance);

  let characters = characterSurface(ui.latest(), character.instance);
  const refreshCharacters = nodeByButtonAction(
    characters,
    CHARACTER_REFRESH_ACTION,
  );
  if (!refreshCharacters)
    throw new Error("Character refresh action is unavailable");
  console.log("SMOKE stage: refresh Manage Characters");
  ui.sendAction(characters, refreshCharacters, CHARACTER_REFRESH_ACTION, {
    type: "none",
  });

  state = await ui.waitFor((message) => {
    const candidate = characterSurface(message, character.instance);
    const add = nodeByAction(candidate, CHARACTER_ADD_TO_WORLD_ACTION);
    return Boolean(add?.kind.type === "button" && add.kind.data.is_enabled);
  });
  characters = characterSurface(state, character.instance);
  const worldSelect = nodeByAction(characters, CHARACTER_SELECT_WORLD_ACTION);
  if (
    worldSelect?.kind.type !== "select" ||
    worldSelect.kind.data.value !== worldId
  ) {
    throw new Error("Character management did not target the created World");
  }
  const addCharacter = nodeByButtonAction(
    characters,
    CHARACTER_ADD_TO_WORLD_ACTION,
  );
  if (!addCharacter)
    throw new Error("Add character to World action is unavailable");
  console.log("SMOKE stage: add reusable character to existing World");
  ui.sendAction(characters, addCharacter, CHARACTER_ADD_TO_WORLD_ACTION, {
    type: "none",
  });

  const world = await waitForCommittedWorld(home, worldId);
  if (world.id !== worldId) {
    throw new Error(
      `unexpected committed World: ${world.id}; expected ${worldId}`,
    );
  }
  const worldInfo = run(["--home", home, "world-info", world.id]);
  if (!worldInfo.includes("commit_position = 1")) {
    throw new Error(
      `Character World did not commit exactly once\n${worldInfo}`,
    );
  }
  const schemas = Number(worldInfo.match(/^schemas = (\d+)$/m)?.[1] ?? "0");
  if (schemas < 1) {
    throw new Error(`Character World did not persist schemas\n${worldInfo}`);
  }

  let extensions = packageManagerSurface(ui.latest(), packageManager.instance);
  const scope = nodeByAction(extensions, PACKAGE_SCOPE_ACTION);
  if (scope?.kind.type !== "select") {
    throw new Error("Package Manager scope selector is unavailable");
  }
  console.log("SMOKE stage: select World-specific Extensions scope");
  ui.sendAction(extensions, scope, PACKAGE_SCOPE_ACTION, {
    type: "text",
    value: worldScopeId,
  });
  await ui.waitFor((message) => {
    const candidate = packageManagerSurface(message, packageManager.instance);
    const selector = nodeByAction(candidate, PACKAGE_SCOPE_ACTION);
    return (
      selector?.kind.type === "select" &&
      selector.kind.data.value === worldScopeId &&
      textContent(candidate).includes("Only this World composition is changed")
    );
  });

  ui.socket.close();
  ui = null;
  await stopHost(host);
  host = null;

  host = startHost(home);
  await waitForHttp(host);
  const restartedUi = await connectUi();
  await restartedUi.waitFor((message) => {
    const candidate = characterSurface(message, character.instance);
    return (
      Boolean(candidate) && textContent(candidate).includes("Direct V3 Alice")
    );
  });
  restartedUi.socket.close();

  const persistedWorldInfo = run(["--home", home, "world-info", world.id]);
  if (!persistedWorldInfo.includes("commit_position = 1")) {
    throw new Error(
      `Character World commit was not stable across restart\n${persistedWorldInfo}`,
    );
  }

  await stopHost(host);
  host = null;
  console.log(
    `PASS: Tavern V3 resource -> Character + Chat bootstrap -> focused usable Chat -> ` +
      `Manage Characters add -> World ${world.id} commit -> restart through real Portable UI and Core Host.`,
  );
} finally {
  if (ui) ui.socket.close();
  await stopHost(host);
  await rm(root, { recursive: true, force: true });
}
