# Cinccino — Circom Language Support for VS Code

VS Code extension that wires the editor to the
[cinccino](https://github.com/mystiz-miso/cinccino) LSP server, giving Circom
developers:

- Syntax highlighting (TextMate grammar)
- Diagnostics (parse errors, type errors, constraint checks, unsafe `<--`)
- Hover info for templates, functions, signals, and components
- Go-to-definition
- Find references
- Completion (keywords, in-scope symbols, pragma versions, dot-access)
- Document symbols (outline view)
- Signature help inside template / function calls

## Installation

### 1. Install the server binary

Build cinccino from source and place `cinccino-lsp` on your `$PATH`:

```bash
cargo install --git https://github.com/mystiz-miso/cinccino.git --bin cinccino-lsp
```

(Or clone and `cargo install --path .` if you'd prefer a checkout.)

The binary lands in `~/.cargo/bin/cinccino-lsp`. Make sure that directory
is on `$PATH`.

### 2. Install the extension

From the VS Code Marketplace:

```bash
code --install-extension samueltangz.cinccino-circom
```

Or search for *Cinccino — Circom Language Support* in the Extensions
view (`Cmd/Ctrl+Shift+X`).

#### Building from source

For iterative development, open the `vscode-extension/` folder in
VS Code and press `F5` to launch an Extension Development Host. To
build a `.vsix` manually:

```bash
cd vscode-extension
npm install
npm run compile
npx @vscode/vsce package --no-dependencies --out cinccino-circom.vsix
code --install-extension cinccino-circom.vsix
```

## Configuration

All settings live under the `cinccino.*` namespace:

| Setting                 | Default          | Description                                                        |
|-------------------------|------------------|--------------------------------------------------------------------|
| `cinccino.serverPath`   | `cinccino-lsp`   | Path to the `cinccino-lsp` binary.                                 |
| `cinccino.libraryPaths` | `[]`             | Extra directories to search for `include "..."` targets (circomlib). |
| `cinccino.trace.server` | `off`            | LSP trace verbosity: `off`, `messages`, or `verbose`.              |

Example `settings.json`:

```json
{
  "cinccino.serverPath": "/usr/local/bin/cinccino-lsp",
  "cinccino.libraryPaths": ["/home/me/circomlib/circuits"]
}
```

## Contributing

Bug reports and PRs welcome. The repo is at
[github.com/mystiz-miso/cinccino](https://github.com/mystiz-miso/cinccino); the
extension lives under `vscode-extension/`.
