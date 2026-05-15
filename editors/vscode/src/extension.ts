// Kryos VS Code extension — launches `kryos lsp` as the language server.
//
// Build: `npx tsc -p .` (from the extension root)
//
// Configuration:
//   `kryos.serverPath` — path to the kryos binary (default: "kryos" on PATH)
//   `kryos.serverArgs` — extra args to pass to the server (default: ["lsp"])
//   `kryos.trace.server` — LSP trace level: "off" | "messages" | "verbose"

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("kryos");
  const serverPath = config.get<string>("serverPath", "kryos");
  const serverArgs = config.get<string[]>("serverArgs", ["lsp"]);

  const serverOptions: ServerOptions = {
    run: {
      command: serverPath,
      args: serverArgs,
      transport: TransportKind.stdio,
    },
    debug: {
      command: serverPath,
      args: serverArgs,
      transport: TransportKind.stdio,
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "kryos" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.kry"),
    },
  };

  client = new LanguageClient(
    "kryos",
    "Kryos Language Server",
    serverOptions,
    clientOptions,
  );

  client.start();
  context.subscriptions.push({
    dispose: () => {
      if (client) {
        client.stop();
      }
    },
  });
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
