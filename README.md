# asterai
The asterai WASM environment runtime.

This includes the asterai CLI tool and the runtime.

# Overview

| Directory               | Description                 |
|-------------------------|-----------------------------|
| [cli][1]                | The asterai CLI utility     |
| [asterai][2]            | The asterai runtime library |
| [component-examples][3] | Example components          |

# CLI Command Reference

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

# Documentation

The asterai documentation is available at [docs.asterai.io](https://docs.asterai.io)

[1]: ./cli
[2]: ./runtime
[3]: ./component-examples
