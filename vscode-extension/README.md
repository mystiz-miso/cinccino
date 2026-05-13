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

Install the extension from the VS Code Marketplace:

```bash
code --install-extension samueltangz.cinccino-circom
```

Or search for *Cinccino — Circom Language Support* in the Extensions
view (`Cmd/Ctrl+Shift+X`).

The first time you open a `.circom` file, the extension will detect
that the `cinccino-lsp` binary is missing and install it via
`cargo install` automatically. Build progress streams into the
**Cinccino** output channel; the LSP starts as soon as the build
finishes (takes ~2 minutes the first time).

### Prerequisite: Rust toolchain

Auto-install requires `cargo` on `$PATH`. If you don't have Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

You can also pre-install the server manually and skip the prompt:

```bash
cargo install --git https://github.com/mystiz-miso/cinccino.git --bin cinccino-lsp
ln -sf "$HOME/.cargo/bin/cinccino-lsp" "$HOME/.local/bin/cinccino-lsp"
```

To re-run the auto-installer later (e.g. to pick up a new version of
the server), use the command palette: **Cinccino: Install or Update
Server**.

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
