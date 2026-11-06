# ignite-lsp

A language server for [Ignite](https://github.com/ignite-lang/ignite), written in Ignite. It speaks
LSP 3.17 over stdin and stdout and publishes the diagnostics of `ignite check` for each open
buffer, as you type, with positions in UTF-8 or UTF-16.

| path | what |
|---|---|
| `src/server.ignite` | the server |
| `src/lsp_types.ignite` | the LSP message types, generated from `docs/lsp/metaModel.json` |
| `tools/ignite-lsp-types/` | the generator of those types |
| `tests/` | the end-to-end test over stdio |

## Install

Needs Rust (stable), [go-task](https://taskfile.dev) and clang.

1. Install the released Ignite compiler, v0.8.0, into `.ignite/`:

   ```sh
   task install-ignite
   ```

   This runs `cargo install --git https://github.com/ignite-lang/ignite --tag v0.8.0 --locked ignite
   --root .ignite`. To use an `ignite` on your `PATH` instead, install it without `--root`.

2. Build the server:

   ```sh
   task build
   ```

   The binary is `target/ignite-lsp/bin/ignite-lsp`. Put it on your `PATH`, together with an
   `ignite`: the server runs `ignite check` for every buffer (`ignite-lsp --ignite /path/to/ignite`
   names another compiler).

## Helix

[ignite-helix](https://github.com/ignite-lang/ignite-helix) has the grammar, the queries and the
`languages.toml` entry that starts `ignite-lsp`; follow its README.

## Development

See [`AGENTS.md`](AGENTS.md).
