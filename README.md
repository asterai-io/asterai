<br />
<p align="center">
  <a href="https://asterai.io">
    <img src="images/logo.png" alt="Asterai Logo" width="100">
  </a>
</p>

<h3 align="center"><b>Asterai</b></h3>
<p align="center">
    <b>The AI Agent Tool Marketplace.</b><br />
    Discover and deploy tools for your AI agents, written in any language.
</p>

<div align="center">

[![License](https://img.shields.io/github/license/asterai-io/asterai?color=blue)](https://github.com/asterai-io/asterai/blob/master/LICENSE)
[![Discord](https://img.shields.io/discord/1260408236578832475?label=discord&color=7289da)](https://asterai.io/discord)
[![GitHub stars](https://img.shields.io/github/stars/asterai-io/asterai)](https://github.com/asterai-io/asterai)
[![X Follow](https://img.shields.io/twitter/follow/asterai_io)](https://x.com/asterai_io)

</div>

<h4 align="center">
  <a href="https://asterai.io" target="_blank">Website</a> ·
  <a href="https://docs.asterai.io" target="_blank">Documentation</a> ·
  <a href="https://asterai.io/discord" target="_blank">Discord</a>
</h4>

<br />

## ✨ Overview

Asterai is an open-source platform and runtime for creating, sharing, and executing portable, sandboxed WebAssembly (WASI) components. Built around wasmtime, it provides a neutral compute substrate for the agentic era.

- **Language Interoperability**: Write tools in any language (Rust, Go, Python, JS, C/C++) — components written in different languages work together seamlessly via type-safe WIT interfaces
- **Sandboxed Execution**: AI agents can run untrusted code safely with WASI security guarantees
- **True Portability**: Deploy anywhere (local, cloud, edge) with no dependency hell — same behavior everywhere
- **Instant Deployment**: Tools just work without DevOps or container configuration

## 🚀 Installation

**npm** (recommended):
```bash
npm install -g @asterai/cli
```

**Cargo** (Rust users):
```bash
cargo install asterai
```

**Binary releases**: Download from [GitHub Releases](https://github.com/asterai-io/asterai/releases)

## 📦 Structure

| Directory               | Description                 |
|-------------------------|-----------------------------|
| [cli][1]                | The asterai CLI utility     |
| [asterai][2]            | The asterai runtime library |
| [component-examples][3] | Example components          |

## 🔧 CLI Command Reference

The CLI operates in two modes: **local** (offline, working with files in
`~/.local/bin/asterai/artifacts`) and **remote** (online, interacting with the Asterai
registry API).

By default, most commands operate locally. To operate on the remote registry instead, pass
the `--remote` or `-r` flag. This enables a git-like workflow where you modify locally and
sync when ready.

| Command                | Local | Remote | `--remote` | Notes                                    |
|------------------------|:-----:|:------:|:----------:|------------------------------------------|
| **Auth**               |       |        |            |                                          |
| `auth login`           |   ✓   |        |            | Stores API key to local file             |
| `auth logout`          |   ✓   |        |            | Removes local API key file               |
| `auth status`          |   ✓   |   ✓    |            | Checks local file, validates against API |
| **Environment**        |       |        |            |                                          |
| `env init`             |   ✓   |        |            | Creates local environment                |
| `env inspect`          |   ✓   |        |            | Reads environment config                 |
| `env add-component`    |   ✓   |        |            | Adds component to environment            |
| `env remove-component` |   ✓   |        |            | Removes component from environment       |
| `env set-var`          |   ✓   |        |            | Sets environment variable                |
| `env list`             |   ✓   |   ✓    |            | Shows local and remote environments      |
| `env run`              |   ✓   |   ✓    |            | Checks local first, pulls if not found   |
| `env pull`             |       |   ✓    |            | Fetches from registry to local           |
| `env push`             |   ✓   |   ✓    |            | Pushes local to registry                 |
| `env delete`           |   ✓   |   ✓    |     ✓      | Deletes environment (-r for registry)    |
| **Component**          |       |        |            |                                          |
| `component init`       |   ✓   |        |            | Creates local component project          |
| `component list`       |   ✓   |        |            | Lists local components                   |
| `component pkg`        |   ✓   |        |            | Packages WIT locally                     |
| `component pull`       |       |   ✓    |            | Fetches from registry to local           |
| `component push`       |   ✓   |   ✓    |            | Pushes local to registry                 |
| `component delete`     |   ✓   |        |            | Deletes local component                  |

**Legend:**
- **Local**: Command reads/writes local files
- **Remote**: Command interacts with the Asterai API
- **`--remote`**: Pass `-r` or `--remote` to operate on the registry instead of locally

## 📚 Documentation

The asterai documentation is available at [docs.asterai.io](https://docs.asterai.io)

[1]: ./cli
[2]: ./runtime
[3]: ./component-examples
