# Ignite for VS Code

Syntax highlighting for `.ignite` files and the diagnostics of `ignite check` as you type,
through [ignite-lsp](https://github.com/ignite-lang/ignite-lsp).

## Binaries

The extension starts `ignite-lsp`, which runs the `ignite` compiler. Each is found in this order:

| binary | setting | then | then |
|---|---|---|---|
| `ignite-lsp` | `ignite.lsp.path` | `ignite-lsp` on `PATH` | the latest release of `ignite-lang/ignite-lsp` |
| `ignite` | `ignite.path` | `ignite` on `PATH` | the latest release of `ignite-lang/ignite` |

A release binary is downloaded once into the extension's storage; the releases carry Linux
x86_64 binaries only. `Ignite: Restart Language Server` restarts the server; the output channel
`Ignite` shows which binaries it runs (`ignite.trace.server` adds the messages).

## Build

```sh
npm ci
npx @vscode/vsce package --skip-license
code --install-extension ignite-0.1.0.vsix
```
