# LSP meta model

`metaModel.json` is the machine-readable meta model of the Language Server Protocol 3.17
(`"metaData": {"version": "3.17.0"}`), vendored unmodified from the
[microsoft/vscode-languageserver-node](https://github.com/microsoft/vscode-languageserver-node)
repository.

| file | from | tag | commit | bytes | sha256 |
|---|---|---|---|---|---|
| `metaModel.json` | <https://github.com/microsoft/vscode-languageserver-node/blob/release/protocol/3.17.5/protocol/metaModel.json> | `release/protocol/3.17.5`, the last 3.17 protocol release | `4f782ceac1b4444d335a32561bda0ded305c401e` | 395,040 | `ba382feb60d4bab4249073c0508b19c67ba2b32c53d70505f452bb49a6f39997` |

It is vendored so that `tools/ignite-lsp-types` can generate the LSP message types of
`src/lsp_types.ignite` offline, and so that the drift test of that crate can
regenerate the module and compare it in the gate (`task lsp-types` rewrites it). The types are
generated, never written by hand (`design:ignite-lsp`, "Decision 2026-11-05: generated types").
The module's header records this file's sha256.

## Licence

The file is part of vscode-languageserver-node, licensed under the MIT License. Its notice,
copied from the repository's `License.txt` at the same tag, is in [`LICENSE.txt`](LICENSE.txt):

> Copyright (c) Microsoft Corporation
>
> All rights reserved.
>
> MIT License
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software
> and associated documentation files (the "Software"), to deal in the Software without
> restriction, including without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
> Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED *AS IS*, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
> BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
