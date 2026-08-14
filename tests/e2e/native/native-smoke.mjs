import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readdir, rm, stat } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const currentFile = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(currentFile), "../../..");
const nativeDir = dirname(currentFile);
const smokeConfig = join(nativeDir, "tauri.smoke.conf.json");
const binary = join(repoRoot, "src-tauri/target/debug/vindexa");
const temporaryRoot = await mkdtemp(join(tmpdir(), "vindexa-tauri-smoke-"));
const temporaryHome = join(temporaryRoot, "home");
const temporaryTmp = join(temporaryRoot, "tmp");
const probeBinary = join(temporaryRoot, "window-probe");
const logs = [];
let app;

const isolatedEnvironment = {
  ...process.env,
  HOME: temporaryHome,
  CFFIXED_USER_HOME: temporaryHome,
  TMPDIR: `${temporaryTmp}/`,
  XDG_CACHE_HOME: join(temporaryHome, ".cache"),
  XDG_CONFIG_HOME: join(temporaryHome, ".config"),
  XDG_DATA_HOME: join(temporaryHome, ".local/share"),
  CARGO_HOME: process.env.CARGO_HOME ?? join(homedir(), ".cargo"),
  RUSTUP_HOME: process.env.RUSTUP_HOME ?? join(homedir(), ".rustup"),
};

try {
  await Promise.all([
    mkdir(temporaryHome, { recursive: true }),
    mkdir(temporaryTmp, { recursive: true }),
  ]);

  await run("xcrun", ["swiftc", join(nativeDir, "window-probe.swift"), "-o", probeBinary]);
  const windowsBefore = await queryWindows();

  await run(
    "pnpm",
    ["exec", "tauri", "build", "--debug", "--no-bundle", "--config", smokeConfig, "--ci"],
    { env: isolatedEnvironment, timeout: 240_000 },
  );

  await stat(binary);
  app = spawn(binary, [], {
    cwd: repoRoot,
    env: isolatedEnvironment,
    stdio: ["ignore", "pipe", "pipe"],
  });
  app.stdout.on("data", (chunk) => collectLog("stdout", chunk));
  app.stderr.on("data", (chunk) => collectLog("stderr", chunk));

  const window = await waitFor(
    async () => {
      assertRunning();
      const matches = await queryWindows(app.pid);
      return matches.find(
        (candidate) =>
          candidate.title === "Vindexa E2E Smoke" &&
          candidate.width >= 900 &&
          candidate.height >= 640,
      );
    },
    25_000,
    "la ventana nativa WKWebView",
  );

  const database = await waitFor(
    async () => {
      assertRunning();
      const candidates = await findNamedFiles(temporaryHome, "vindexa.sqlite3");
      return candidates.length === 1 ? candidates[0] : undefined;
    },
    15_000,
    "la base SQLite aislada",
  );

  const relativeDatabase = relative(temporaryHome, database);
  if (!relativeDatabase.includes("io.vindexa.desktop.e2e")) {
    throw new Error(`La base E2E escapó del identificador aislado: ${relativeDatabase}`);
  }

  const windowsAfter = await queryWindows();
  const priorWindowIds = new Set(windowsBefore.map((candidate) => candidate.id));
  const credentialWindows = windowsAfter.filter(
    (candidate) =>
      !priorWindowIds.has(candidate.id) &&
      /SecurityAgent|CoreServicesUIAgent/i.test(candidate.ownerName),
  );
  if (credentialWindows.length > 0) {
    throw new Error(
      `El arranque abrió una ventana de credenciales: ${JSON.stringify(credentialWindows)}`,
    );
  }

  console.log(
    JSON.stringify(
      {
        result: "PASS",
        process: { pid: app.pid, binary: relative(repoRoot, binary) },
        window,
        identifier: "io.vindexa.desktop.e2e",
        database: relativeDatabase,
        isolationRoot: basename(temporaryRoot),
        keychainPromptDetected: false,
      },
      null,
      2,
    ),
  );
} finally {
  await stopApp();
  await rm(temporaryRoot, { recursive: true, force: true });
}

function collectLog(stream, chunk) {
  logs.push(`[${stream}] ${String(chunk)}`);
  if (logs.length > 80) logs.shift();
}

function assertRunning() {
  if (!app || app.exitCode === null) return;
  throw new Error(
    `Vindexa terminó antes de completar el smoke (exit ${app.exitCode}).\n${logs.join("")}`,
  );
}

async function queryWindows(pid) {
  const args = pid ? [String(pid)] : [];
  const output = await run(probeBinary, args, { capture: true, timeout: 10_000 });
  return JSON.parse(output);
}

async function findNamedFiles(directory, name) {
  const matches = [];
  const entries = await readdir(directory, { withFileTypes: true });
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) matches.push(...(await findNamedFiles(path, name)));
    else if (entry.isFile() && entry.name === name) matches.push(path);
  }
  return matches;
}

async function waitFor(probe, timeout, description) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const result = await probe();
    if (result !== undefined) return result;
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 250));
  }
  throw new Error(`Tiempo agotado esperando ${description}.\n${logs.join("")}`);
}

async function stopApp() {
  if (!app || app.exitCode !== null) return;
  app.kill("SIGTERM");
  const stopped = await Promise.race([
    new Promise((resolveStopped) => app.once("exit", () => resolveStopped(true))),
    new Promise((resolveStopped) => setTimeout(() => resolveStopped(false), 4_000)),
  ]);
  if (!stopped && app.exitCode === null) app.kill("SIGKILL");
}

function run(command, args, options = {}) {
  const { capture = false, env = process.env, timeout = 120_000 } = options;
  return new Promise((resolveRun, rejectRun) => {
    const child = spawn(command, args, {
      cwd: repoRoot,
      env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      rejectRun(new Error(`${command} excedió ${timeout} ms.`));
    }, timeout);
    child.stdout.on("data", (chunk) => {
      stdout += String(chunk);
      if (!capture) process.stdout.write(chunk);
    });
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
      if (!capture) process.stderr.write(chunk);
    });
    child.once("error", (error) => {
      clearTimeout(timer);
      rejectRun(error);
    });
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      if (code === 0) resolveRun(stdout);
      else rejectRun(new Error(`${command} terminó con ${code ?? signal}.\n${stderr}`));
    });
  });
}
