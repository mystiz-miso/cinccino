# Cinccino — Circom Language Support for VS Code

VS Code extension that wires the editor to the
[cinccino](https://github.com/litwick/cinccino) LSP server, giving Circom
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
git clone https://github.com/litwick/cinccino.git
cd cinccino
cargo install --path . --bin cinccino-lsp
```

The binary lands in `~/.cargo/bin/cinccino-lsp`. Make sure that directory
is on `$PATH`.

### 2. Install the extension

While development builds the extension is not published to the VS Code
Marketplace. Install it from source:

```bash
cd cinccino/vscode-extension
npm install
npm run compile
# Package into a .vsix
npx @vscode/vsce package --out cinccino-circom.vsix
# Install in VS Code
code --install-extension cinccino-circom.vsix
```

Or, for iterative development, open the `vscode-extension/` folder in
VS Code and press `F5` to launch an Extension Development Host.

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
[github.com/litwick/cinccino](https://github.com/litwick/cinccino); the
extension lives under `cinccino/vscode-extension/`.
