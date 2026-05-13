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

let client: LanguageClient | undefined;
let log: OutputChannel | undefined;
let autoInstallAttempted = false;

export async function activate(context: ExtensionContext): Promise<void> {
  log = window.createOutputChannel("Cinccino");
  context.subscriptions.push(log);
  log.appendLine(`[${new Date().toISOString()}] activate() entered`);

  context.subscriptions.push(
    commands.registerCommand("cinccino.restart", async () => {
      log?.appendLine(`[${new Date().toISOString()}] restart requested`);
      await stopServer();
      await startServer(context);
    }),
    commands.registerCommand("cinccino.installServer", async () => {
      // Manual entry point — always attempts install regardless of prompt-suppression state.
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
  const rawServerPath = config.get<string>("serverPath") ?? "cinccino-lsp";
  const libraryPaths = config.get<string[]>("libraryPaths") ?? [];

  log?.appendLine(`config.serverPath = ${rawServerPath}`);
  log?.appendLine(`PATH             = ${process.env.PATH ?? "(unset)"}`);
  log?.appendLine(`libraryPaths     = ${JSON.stringify(libraryPaths)}`);

  const ready = await ensureServerAvailable(rawServerPath);
  if (!ready) {
    log?.appendLine("server not available; skipping LSP start");
    return;
  }

  const serverPath = resolveServerPath(rawServerPath);
  log?.appendLine(`resolved binary  = ${serverPath}`);

  const serverOptions: ServerOptions = {
    run: {
      command: serverPath,
      transport: TransportKind.stdio,
      options: { env: process.env },
    },
    debug: {
      command: serverPath,
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
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log?.appendLine(`ERROR starting LSP: ${msg}`);
    window.showErrorMessage(`Failed to start cinccino-lsp at ${serverPath}: ${msg}`);
  }

  context.subscriptions.push({ dispose: () => void client?.stop() });
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
 * Confirm the binary referenced by `rawPath` is locatable. For absolute or
 * separator-bearing paths we stat the file directly; for bare names we ask
 * the shell via `which`. If missing, attempt cargo install silently (one
 * attempt per session) — matching the install-on-first-use UX of
 * rust-analyzer, gopls, etc.
 *
 * Two exceptions to the silent-install path:
 *   1. The user configured an *absolute* `cinccino.serverPath`. We respect
 *      their explicit choice and surface an error rather than installing
 *      somewhere they didn't ask for.
 *   2. The `cargo` binary itself is missing. We can't bootstrap a Rust
 *      toolchain without sudo / shell-rc edits, so we point at rustup.rs
 *      and leave the install to the user.
 */
async function ensureServerAvailable(rawPath: string): Promise<boolean> {
  if (await binaryResolves(rawPath)) return true;

  if (path.isAbsolute(rawPath)) {
    log?.appendLine(`absolute serverPath "${rawPath}" does not exist; not auto-installing`);
    window.showErrorMessage(
      `cinccino.serverPath points at "${rawPath}", which doesn't exist. ` +
        `Either fix the setting or clear it to fall back to the default.`,
    );
    return false;
  }

  if (autoInstallAttempted) {
    log?.appendLine("binary missing and auto-install already attempted this session");
    return false;
  }
  autoInstallAttempted = true;

  log?.appendLine("cinccino-lsp not found on PATH; attempting auto-install via cargo");
  const ok = await runCargoInstall();
  return ok && (await binaryResolves(rawPath));
}

async function binaryResolves(rawPath: string): Promise<boolean> {
  if (path.isAbsolute(rawPath)) {
    return fs.existsSync(rawPath);
  }
  if (rawPath.includes(path.sep) || rawPath.includes("/")) {
    const root = workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!root) return false;
    return fs.existsSync(path.resolve(root, rawPath));
  }
  return (await which(rawPath)) !== null;
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

function resolveServerPath(raw: string): string {
  if (path.isAbsolute(raw)) {
    return raw;
  }
  if (raw.includes(path.sep) || raw.includes("/")) {
    const root = workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (root) {
      return path.resolve(root, raw);
    }
  }
  return raw;
}
