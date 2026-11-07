# ignite-lsp

A language server for [Ignite](https://github.com/ignite-lang/ignite), written in Ignite. It speaks
LSP 3.17 over stdin and stdout, with positions in UTF-8 or UTF-16:

- diagnostics: those of `ignite check` for each open buffer, as you type; changes that arrive
  while a check runs are checked once, at their newest version; while a syntax error hides the
  type errors, the last ones stay visible, marked `(before the syntax error)`;
- hover: the type of the name, field, call or literal under the cursor, or a declaration's
  signature;
- go to definition: locals, parameters, fns, types, fields, variants and constants declared in
  the same file;
- document outline: types with their fields, enums with their variants, fns, constants, tests,
  systems with their members and protocols with their roles.

Hover, definition and outline come from `ignite emit --typed-json`, run once per buffer
version. An unsaved buffer that imports sibling modules has none until it is saved (`emit` has
no `--stdin-for`).

| path | what |
|---|---|
| `src/server.ignite` | the server |
| `src/typed.ignite` | the index behind hover, definition and the outline |
| `src/lsp_types.ignite` | the LSP message types, generated from `docs/lsp/metaModel.json` |
| `tools/ignite-lsp-types/` | the generator of those types |
| `tests/` | the end-to-end test over stdio |
| `editors/vscode/` | the VS Code extension: language `ignite`, TextMate grammar, client of `ignite-lsp` |

## Install

Each [release](https://github.com/ignite-lang/ignite-lsp/releases) carries two assets:
`ignite-lsp-VERSION-x86_64-linux`, the server for Linux x86_64 (statically linked, musl), and
`ignite-VERSION.vsix`, the VS Code extension. The server runs the `ignite` compiler for every
buffer; its releases carry `ignite-VERSION-x86_64-linux.tar.gz` from v0.13.0 on.

1. The compiler: unpack the archive of an [ignite release](https://github.com/ignite-lang/ignite/releases)
   into a prefix whose `bin` is on your `PATH`, here `~/.local`:

   ```sh
   curl -LO https://github.com/ignite-lang/ignite/releases/download/v0.13.0/ignite-0.13.0-x86_64-linux.tar.gz
   tar -xzf ignite-0.13.0-x86_64-linux.tar.gz
   cp ignite-0.13.0-x86_64-linux/bin/ignite ~/.local/bin/
   mkdir -p ~/.local/share/ignite
   cp -R ignite-0.13.0-x86_64-linux/share/ignite/std ~/.local/share/ignite/
   ```

   The driver reads its standard library from `../share/ignite/std` beside its `bin`.

2. The server:

   ```sh
   curl -L -o ~/.local/bin/ignite-lsp https://github.com/ignite-lang/ignite-lsp/releases/download/v0.2.0/ignite-lsp-0.2.0-x86_64-linux
   chmod +x ~/.local/bin/ignite-lsp
   ```

   `ignite-lsp --ignite /path/to/ignite` names a compiler that is not on `PATH`.

3. VS Code: download `ignite-0.2.0.vsix` from the release, then

   ```sh
   code --install-extension ignite-0.2.0.vsix
   ```

   and run "Developer: Reload Window". The extension (`ignite-lang.ignite`) finds `ignite-lsp`
   and `ignite` through the settings `ignite.lsp.path` and `ignite.path`, then on `PATH`, and
   otherwise downloads both from the latest releases, so steps 1 and 2 are optional for it.
   Its source is `editors/vscode/`.

### From source

Needs Rust (stable), [go-task](https://taskfile.dev) and clang.

1. Install the released Ignite compiler, v0.13.0, into `.ignite/`: `task install-ignite` (runs
   `cargo install --git https://github.com/ignite-lang/ignite --tag v0.13.0 --locked ignite
   --root .ignite`; to use an `ignite` on your `PATH` instead, install it without `--root`).
2. Build the server: `task build`. The binary is `target/ignite-lsp/bin/ignite-lsp`.
3. Package the extension: `task vsix` writes `editors/vscode/ignite-VERSION.vsix` (needs node
   and npm).

## Helix

[ignite-helix](https://github.com/ignite-lang/ignite-helix) has the grammar, the queries and the
`languages.toml` entry that starts `ignite-lsp`; follow its README.

## Development

See [`AGENTS.md`](AGENTS.md).
