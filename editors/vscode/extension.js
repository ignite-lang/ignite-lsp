"use strict";

// The Ignite extension: starts ignite-lsp over stdio for `ignite` documents. Each binary is
// found in this order: its setting (`ignite.lsp.path`, `ignite.path`), the name on PATH, then
// the Linux x86_64 asset of the latest GitHub release, downloaded once into the extension's
// global storage.

const fs = require("fs");
const https = require("https");
const path = require("path");
const { execFile } = require("child_process");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

const SERVER = { name: "ignite-lsp", setting: "lsp.path", repo: "ignite-lang/ignite-lsp" };
const COMPILER = { name: "ignite", setting: "path", repo: "ignite-lang/ignite" };

let client;
let output;

async function activate(context) {
  output = vscode.window.createOutputChannel("Ignite");
  context.subscriptions.push(output);
  context.subscriptions.push(
    vscode.commands.registerCommand("ignite.restartServer", async () => {
      await stop();
      await start(context);
    })
  );
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(async (event) => {
      if (event.affectsConfiguration("ignite.lsp.path") || event.affectsConfiguration("ignite.path")) {
        await stop();
        await start(context);
      }
    })
  );
  await start(context);
}

async function deactivate() {
  await stop();
}

async function start(context) {
  let server;
  let compiler;
  try {
    server = await resolve(context, SERVER);
    compiler = await resolve(context, COMPILER);
  } catch (error) {
    output.appendLine(String(error && error.stack ? error.stack : error));
    vscode.window.showErrorMessage(`Ignite: ${error.message || error}`);
    return;
  }
  output.appendLine(`ignite-lsp: ${server}`);
  output.appendLine(`ignite: ${compiler}`);
  const executable = { command: server, args: ["--ignite", compiler], transport: TransportKind.stdio };
  client = new LanguageClient(
    "ignite",
    "Ignite",
    { run: executable, debug: executable },
    { documentSelector: [{ language: "ignite" }], outputChannel: output }
  );
  await client.start();
}

async function stop() {
  if (client) {
    const running = client;
    client = undefined;
    await running.stop().catch(() => {});
  }
}

// The configured path, else the name on PATH, else the downloaded release asset.
async function resolve(context, tool) {
  const configured = vscode.workspace.getConfiguration("ignite").get(tool.setting, "").trim();
  if (configured) {
    const full = expandHome(configured);
    if (!isExecutable(full)) {
      throw new Error(`ignite.${tool.setting} is ${configured}, which is not an executable file`);
    }
    return full;
  }
  const onPath = which(tool.name);
  if (onPath) {
    return onPath;
  }
  return download(context, tool);
}

function expandHome(file) {
  return file.startsWith("~/") ? path.join(require("os").homedir(), file.slice(2)) : file;
}

function isExecutable(file) {
  try {
    fs.accessSync(file, fs.constants.X_OK);
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function which(name) {
  for (const dir of (process.env.PATH || "").split(path.delimiter)) {
    if (dir && isExecutable(path.join(dir, name))) {
      return path.join(dir, name);
    }
  }
  return undefined;
}

// The release asset for Linux x86_64: `ignite-lsp-<version>-x86_64-linux` (the binary) or
// `ignite-<version>-x86_64-linux.tar.gz` (bin/ignite and share/ignite/std), unpacked under
// globalStorage/<repo>/<tag>/.
async function download(context, tool) {
  if (process.platform !== "linux" || process.arch !== "x64") {
    throw new Error(
      `${tool.name} is not on PATH and the releases carry Linux x86_64 binaries only: set ignite.${tool.setting}`
    );
  }
  const root = path.join(context.globalStorageUri.fsPath, tool.repo.replace("/", "-"));
  const release = await getJson(`https://api.github.com/repos/${tool.repo}/releases/latest`).catch(
    (error) => {
      const cached = newestCached(root, tool);
      if (cached) {
        output.appendLine(`latest release of ${tool.repo} not reachable (${error.message}), using ${cached}`);
        return { cached };
      }
      throw new Error(`${tool.name} is not on PATH and the latest release could not be read: ${error.message}`);
    }
  );
  if (release.cached) {
    return release.cached;
  }
  const dir = path.join(root, release.tag_name);
  const binary = installedBinary(dir, tool);
  if (isExecutable(binary)) {
    return binary;
  }
  const asset = (release.assets || []).find((a) => assetMatches(tool, a.name));
  if (!asset) {
    throw new Error(`${tool.repo} ${release.tag_name} has no Linux x86_64 asset of ${tool.name}`);
  }
  return vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: `Ignite: downloading ${asset.name}` },
    async () => {
      fs.mkdirSync(dir, { recursive: true });
      const file = path.join(dir, asset.name);
      await getFile(asset.browser_download_url, file);
      if (asset.name.endsWith(".tar.gz")) {
        await run(which("tar") || "/usr/bin/tar", ["-xzf", file, "-C", dir, "--strip-components=1"]);
        fs.unlinkSync(file);
      } else {
        fs.renameSync(file, binary);
      }
      fs.chmodSync(binary, 0o755);
      output.appendLine(`downloaded ${asset.browser_download_url} to ${binary}`);
      return binary;
    }
  );
}

function assetMatches(tool, name) {
  return tool === COMPILER
    ? /^ignite-.*x86_64-linux\.tar\.gz$/.test(name)
    : /^ignite-lsp-.*x86_64-linux$/.test(name);
}

function installedBinary(dir, tool) {
  return tool === COMPILER ? path.join(dir, "bin", "ignite") : path.join(dir, "ignite-lsp");
}

function newestCached(root, tool) {
  let tags;
  try {
    tags = fs.readdirSync(root).sort().reverse();
  } catch {
    return undefined;
  }
  return tags.map((tag) => installedBinary(path.join(root, tag), tool)).find(isExecutable);
}

function run(command, args) {
  return new Promise((resolvePromise, reject) =>
    execFile(command, args, (error, _stdout, stderr) =>
      error ? reject(new Error(`${command} ${args.join(" ")}: ${stderr || error.message}`)) : resolvePromise()
    )
  );
}

function request(url, onResponse, reject, redirects = 0) {
  https
    .get(url, { headers: { "User-Agent": "ignite-vscode", Accept: "application/octet-stream, application/json" } }, (response) => {
      const status = response.statusCode || 0;
      if (status >= 300 && status < 400 && response.headers.location && redirects < 5) {
        response.resume();
        request(new URL(response.headers.location, url).toString(), onResponse, reject, redirects + 1);
      } else if (status !== 200) {
        response.resume();
        reject(new Error(`GET ${url}: HTTP ${status}`));
      } else {
        onResponse(response);
      }
    })
    .on("error", reject);
}

function getJson(url) {
  return new Promise((resolvePromise, reject) =>
    request(
      url,
      (response) => {
        let body = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => (body += chunk));
        response.on("end", () => {
          try {
            resolvePromise(JSON.parse(body));
          } catch (error) {
            reject(error);
          }
        });
      },
      reject
    )
  );
}

function getFile(url, file) {
  return new Promise((resolvePromise, reject) =>
    request(
      url,
      (response) => {
        const sink = fs.createWriteStream(file);
        response.pipe(sink);
        sink.on("finish", () => sink.close(resolvePromise));
        sink.on("error", reject);
      },
      reject
    )
  );
}

module.exports = { activate, deactivate };
