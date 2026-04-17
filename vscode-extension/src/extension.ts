import * as path from "path";
import { ExtensionContext, window, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

/**
 * Activation entry point. Launches the cinccino-lsp binary and wires
 * VS Code's language client to it over stdio.
 */
export function activate(context: ExtensionContext): void {
  const config = workspace.getConfiguration("cinccino");
  const serverPath = resolveServerPath(config.get<string>("serverPath") ?? "cinccino-lsp");
  const libraryPaths = config.get<string[]>("libraryPaths") ?? [];

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
    documentSelector: [{ scheme: "file", language: "circom" }],
    synchronize: {
      configurationSection: "cinccino",
      fileEvents: workspace.createFileSystemWatcher("**/*.circom"),
    },
    initializationOptions: {
      libraryPaths,
    },
  };

  client = new LanguageClient(
    "cinccino",
    "Cinccino (Circom)",
    serverOptions,
    clientOptions,
  );

  client.start().catch((err) => {
    window.showErrorMessage(
      `Failed to start cinccino-lsp at ${serverPath}: ${err instanceof Error ? err.message : String(err)}`,
    );
  });

  context.subscriptions.push({
    dispose: () => {
      void client?.stop();
    },
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}

/**
 * Resolve the server binary path. Three cases:
 *   1. Absolute path ("/usr/local/bin/cinccino-lsp") — return verbatim.
 *   2. Path with a separator ("./cinccino-lsp", "bin/cinccino-lsp") —
 *      resolve against the first workspace folder so it survives the
 *      stdio child_process.spawn CWD.
 *   3. Bare name ("cinccino-lsp") — return as-is so PATH lookup kicks in.
 */
function resolveServerPath(raw: string): string {
  if (path.isAbsolute(raw)) {
    return raw;
  }
  if (raw.includes(path.sep) || raw.includes("/")) {
    const workspace = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (workspace) {
      return path.resolve(workspace, raw);
    }
  }
  return raw;
}
