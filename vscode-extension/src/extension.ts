import * as cp from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import {
  commands,
  env,
  ExtensionContext,
  OutputChannel,
  ProgressLocation,
  Uri,
  window,
  workspace,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

const SERVER_REPO = "https://github.com/mystiz-miso/cinccino.git";
const DEFAULT_SERVER_PATH = "cinccino-lsp";

// Minimum cinccino-lsp version this extension requires. Bump this in
// lockstep with `cinccino/Cargo.toml` whenever the extension starts
// depending on a server-side fix or feature — the activation-time version
// check below will then prompt users on older binaries to upgrade.
const MIN_SERVER_VERSION = "0.2.0";

type ServerSource = "override" | "bundled" | "path" | "cargo";
type ResolvedServer = { path: string; source: ServerSource };

let client: LanguageClient | undefined;
let log: OutputChannel | undefined;
let extensionRoot: string | undefined;
let autoInstallAttempted = false;

export async function activate(context: ExtensionContext): Promise<void> {
  log = window.createOutputChannel("Cinccino");
  context.subscriptions.push(log);
  extensionRoot = context.extensionPath;
  log.appendLine(`[${new Date().toISOString()}] activate() entered`);

  context.subscriptions.push(
    commands.registerCommand("cinccino.restart", async () => {
      log?.appendLine(`[${new Date().toISOString()}] restart requested`);
      await stopServer();
      await startServer(context);
    }),
    commands.registerCommand("cinccino.installServer", async () => {
      // Manual entry point — always attempts cargo install. Useful on the
      // generic .vsix (no bundled binary) or when you want to upgrade the
      // server independently.
      const ok = await runCargoInstall();
      if (ok) {
        await stopServer();
        await startServer(context);
      }
    }),
  );

  await startServer(context);
}

export function deactivate(): Thenable<void> | undefined {
  return stopServer();
}

async function startServer(context: ExtensionContext): Promise<void> {
  const config = workspace.getConfiguration("cinccino");
  const rawServerPath = config.get<string>("serverPath") ?? DEFAULT_SERVER_PATH;
  const libraryPaths = config.get<string[]>("libraryPaths") ?? [];

  log?.appendLine(`config.serverPath = ${rawServerPath}`);
  log?.appendLine(`PATH             = ${process.env.PATH ?? "(unset)"}`);
  log?.appendLine(`libraryPaths     = ${JSON.stringify(libraryPaths)}`);

  const resolved = await resolveServer(rawServerPath);
  if (!resolved) {
    log?.appendLine("server not available; skipping LSP start");
    return;
  }
  log?.appendLine(`resolved binary  = ${resolved.path} (source: ${resolved.source})`);

  const serverOptions: ServerOptions = {
    run: {
      command: resolved.path,
      transport: TransportKind.stdio,
      options: { env: process.env },
    },
    debug: {
      command: resolved.path,
      transport: TransportKind.stdio,
      options: { env: { ...process.env, RUST_LOG: "cinccino=debug" } },
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "circom" },
      { scheme: "untitled", language: "circom" },
    ],
    synchronize: {
      configurationSection: "cinccino",
      fileEvents: workspace.createFileSystemWatcher("**/*.circom"),
    },
    initializationOptions: { libraryPaths },
    outputChannel: log,
  };

  client = new LanguageClient(
    "cinccino",
    "Cinccino (Circom)",
    serverOptions,
    clientOptions,
  );

  try {
    await client.start();
    log?.appendLine(`[${new Date().toISOString()}] LSP started successfully`);
    await checkServerVersion(resolved, context);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log?.appendLine(`ERROR starting LSP: ${msg}`);
    window.showErrorMessage(`Failed to start cinccino-lsp at ${resolved.path}: ${msg}`);
  }

  context.subscriptions.push({ dispose: () => void client?.stop() });
}

/**
 * Compare two `MAJOR.MINOR.PATCH` strings. Returns negative if `a < b`,
 * 0 if equal, positive if `a > b`. Non-numeric or pre-release suffixes
 * are ignored — sufficient for our gating use case.
 */
function compareSemver(a: string, b: string): number {
  const parse = (s: string) =>
    s.split(".").slice(0, 3).map((p) => parseInt(p, 10) || 0);
  const [a0, a1, a2] = parse(a);
  const [b0, b1, b2] = parse(b);
  return a0 - b0 || a1 - b1 || a2 - b2;
}

/**
 * If the running cinccino-lsp predates `MIN_SERVER_VERSION`, react based
 * on where the binary came from:
 *   - cargo / path: offer to run cargo install (we manage that path).
 *   - bundled:       warn and suggest reinstalling the extension.
 *   - override:      warn only — the user pinned this binary themselves.
 */
async function checkServerVersion(
  resolved: ResolvedServer,
  context: ExtensionContext,
): Promise<void> {
  const reported = client?.initializeResult?.serverInfo?.version;
  if (!reported) {
    log?.appendLine("server did not report a version; skipping version check");
    return;
  }
  log?.appendLine(`server reports version ${reported}; required >= ${MIN_SERVER_VERSION}`);
  if (compareSemver(reported, MIN_SERVER_VERSION) >= 0) return;

  const summary = `cinccino-lsp ${reported} is older than the ${MIN_SERVER_VERSION} this extension requires.`;
  log?.appendLine(summary);

  if (resolved.source === "override") {
    window.showWarningMessage(
      `${summary} You set cinccino.serverPath manually, so update or replace that binary yourself.`,
    );
    return;
  }
  if (resolved.source === "bundled") {
    window.showWarningMessage(
      `${summary} Reinstall the extension to pick up the bundled binary that matches.`,
    );
    return;
  }

  const choice = await window.showWarningMessage(
    `${summary} Upgrade now via cargo install?`,
    "Upgrade",
    "Later",
  );
  if (choice !== "Upgrade") return;

  const ok = await runCargoInstall();
  if (!ok) return;
  await stopServer();
  await startServer(context);
}

async function stopServer(): Promise<void> {
  if (!client) return;
  try {
    await client.stop();
  } catch (err) {
    log?.appendLine(`error stopping client: ${err}`);
  }
  client = undefined;
}

/**
 * Resolve the LSP server binary path, walking a four-stop priority list.
 * Returns the absolute path to spawn, or undefined if nothing usable was
 * found (in which case startServer aborts gracefully).
 *
 * Priority:
 *   1. User-configured `cinccino.serverPath` if non-default. Respected
 *      verbatim — we don't fall back if it's missing, since the user
 *      explicitly pointed us somewhere.
 *   2. Pre-built binary bundled into the .vsix at
 *      `<extension>/server/<vscode-target>/cinccino-lsp[.exe]`. This is
 *      the path Marketplace users on a platform-targeted .vsix take —
 *      zero install steps for them.
 *   3. `cinccino-lsp` already on PATH. Covers users who pre-installed
 *      manually, plus the development-build .vsix that has no bundled
 *      binary.
 *   4. Auto-install via cargo. Last-resort for users on a generic .vsix
 *      with no Rust toolchain prior, with bundled binary not present.
 */
async function resolveServer(rawPath: string): Promise<ResolvedServer | undefined> {
  // (1) Explicit non-default override → respect it verbatim.
  if (rawPath !== DEFAULT_SERVER_PATH) {
    const expanded = expandUserPath(rawPath);
    if (fs.existsSync(expanded)) return { path: expanded, source: "override" };
    log?.appendLine(`configured cinccino.serverPath "${rawPath}" does not exist`);
    window.showErrorMessage(
      `cinccino.serverPath points at "${rawPath}", which doesn't exist. ` +
        `Either fix the setting or clear it to fall back to the bundled binary.`,
    );
    return undefined;
  }

  // (2) Pre-built bundled binary.
  const bundled = bundledServerPath();
  if (bundled) {
    log?.appendLine(`using bundled binary at ${bundled}`);
    return { path: bundled, source: "bundled" };
  }

  // (3) cinccino-lsp on PATH.
  const onPath = await which(rawPath);
  if (onPath) {
    log?.appendLine(`found on PATH at ${onPath}`);
    return { path: onPath, source: "path" };
  }

  // (4) Auto-install via cargo, then re-check PATH.
  if (autoInstallAttempted) {
    log?.appendLine("auto-install already attempted this session; giving up");
    return undefined;
  }
  autoInstallAttempted = true;
  log?.appendLine("cinccino-lsp not found anywhere; attempting auto-install via cargo");
  const ok = await runCargoInstall();
  if (!ok) return undefined;
  const installed = await which(rawPath);
  return installed ? { path: installed, source: "cargo" } : undefined;
}

/**
 * Map (process.platform, process.arch) to the VS Code target folder we
 * publish under and check for a bundled binary there.
 */
function bundledServerPath(): string | undefined {
  if (!extensionRoot) return undefined;
  const target = vscodeTarget();
  if (!target) {
    log?.appendLine(`no bundled binary for ${process.platform}-${process.arch}`);
    return undefined;
  }
  const exe = process.platform === "win32" ? "cinccino-lsp.exe" : "cinccino-lsp";
  const candidate = path.join(extensionRoot, "server", target, exe);
  return fs.existsSync(candidate) ? candidate : undefined;
}

function vscodeTarget(): string | undefined {
  const arch = process.arch;
  switch (process.platform) {
    case "linux":
      return arch === "arm64" ? "linux-arm64" : arch === "x64" ? "linux-x64" : undefined;
    case "darwin":
      return arch === "arm64" ? "darwin-arm64" : arch === "x64" ? "darwin-x64" : undefined;
    case "win32":
      return arch === "x64" ? "win32-x64" : undefined;
    default:
      return undefined;
  }
}

function expandUserPath(raw: string): string {
  // Expand a leading `~` for user-set absolute paths like ~/.cargo/bin/cinccino-lsp.
  if (raw.startsWith("~")) return path.join(os.homedir(), raw.slice(1));
  if (path.isAbsolute(raw)) return raw;
  // Relative path with a separator → resolve against workspace root for
  // backwards compatibility with the old behaviour.
  if (raw.includes(path.sep) || raw.includes("/")) {
    const root = workspace.workspaceFolders?.[0]?.uri.fsPath;
    return root ? path.resolve(root, raw) : raw;
  }
  return raw;
}

/**
 * Cross-platform `which` that returns the resolved path or null. Uses
 * `where` on Windows, `which` elsewhere; bypasses the shell so PATH
 * resolution matches what `child_process.spawn` actually uses.
 */
function which(cmd: string): Promise<string | null> {
  return new Promise((resolve) => {
    const lookup = process.platform === "win32" ? "where" : "which";
    const proc = cp.spawn(lookup, [cmd]);
    let out = "";
    proc.stdout.on("data", (chunk) => (out += chunk.toString()));
    proc.on("close", (code) => {
      if (code !== 0) return resolve(null);
      const first = out.split(/\r?\n/).find((s) => s.trim().length > 0);
      resolve(first?.trim() ?? null);
    });
    proc.on("error", () => resolve(null));
  });
}

/**
 * Run `cargo install --git … --bin cinccino-lsp` with progress UI and
 * live output streamed to the Cinccino channel. On success, symlink the
 * resulting binary into `~/.local/bin/` (Linux/macOS) so the default
 * `cinccino.serverPath: "cinccino-lsp"` lookup works without the user
 * having to source `~/.cargo/env` in the extension host's environment.
 */
async function runCargoInstall(): Promise<boolean> {
  const cargo = await which("cargo");
  if (!cargo) {
    const choice = await window.showErrorMessage(
      "Rust toolchain not found. Install Rust from https://rustup.rs, then run \"Cinccino: Install Server\".",
      "Open rustup.rs",
    );
    if (choice === "Open rustup.rs") {
      void env.openExternal(Uri.parse("https://rustup.rs"));
    }
    return false;
  }

  log?.appendLine(`Found cargo at ${cargo}`);
  log?.appendLine(`Running: cargo install --git ${SERVER_REPO} --bin cinccino-lsp`);
  log?.show(true);

  return window.withProgress(
    {
      location: ProgressLocation.Notification,
      title: "Installing cinccino-lsp",
      cancellable: false,
    },
    async (progress) => {
      progress.report({ message: "compiling (this may take a few minutes)…" });
      const code = await new Promise<number>((resolve) => {
        const proc = cp.spawn(
          cargo,
          ["install", "--git", SERVER_REPO, "--bin", "cinccino-lsp"],
          { env: process.env },
        );
        proc.stdout.on("data", (d) => log?.append(d.toString()));
        proc.stderr.on("data", (d) => log?.append(d.toString()));
        proc.on("close", (rc) => resolve(rc ?? -1));
        proc.on("error", (err) => {
          log?.appendLine(`spawn error: ${err.message}`);
          resolve(-1);
        });
      });

      if (code !== 0) {
        window.showErrorMessage(
          `cargo install failed (exit ${code}). See the Cinccino output channel for details.`,
        );
        return false;
      }

      if (process.platform !== "win32") {
        await trySymlinkIntoLocalBin();
      }

      window.showInformationMessage("cinccino-lsp installed.");
      return true;
    },
  );
}

/**
 * cargo installs into `~/.cargo/bin/`. That directory is on the
 * interactive shell's PATH (via `~/.cargo/env` sourced from .bashrc),
 * but VS Code's extension host doesn't run a login shell — so the
 * symlink into `~/.local/bin/` (which *is* on the host PATH on most
 * Linux setups) is what makes the default `serverPath: "cinccino-lsp"`
 * actually work. Best-effort; we log and ignore failures.
 */
async function trySymlinkIntoLocalBin(): Promise<void> {
  const home = os.homedir();
  const src = path.join(home, ".cargo", "bin", "cinccino-lsp");
  const dstDir = path.join(home, ".local", "bin");
  const dst = path.join(dstDir, "cinccino-lsp");
  try {
    if (!fs.existsSync(src)) {
      log?.appendLine(`Skipping symlink: ${src} does not exist`);
      return;
    }
    fs.mkdirSync(dstDir, { recursive: true });
    try {
      fs.unlinkSync(dst);
    } catch {
      // dst didn't exist; that's fine.
    }
    fs.symlinkSync(src, dst);
    log?.appendLine(`Symlinked ${src} → ${dst}`);
  } catch (err) {
    log?.appendLine(`symlink failed (non-fatal): ${err}`);
  }
}

