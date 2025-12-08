# asterai
The asterai WASM environment runtime.

This includes the asterai CLI tool and the runtime.

# Overview

| Directory            | Description             |
|----------------------|-------------------------| 
| [cli][1]             | The asterai CLI utility |
| [runtime][2]         | The asterai runtime     |
| [plugin-examples][3] | Example plugins         |

# Documentation

The asterai documentation is available at [docs.asterai.io](https://docs.asterai.io)

## Plugin secrets and environment variables

An asterai App can configure secrets and environment variables
to be passed via the hook input.
Plugins are responsible for not leaking the secrets to untrusted
plugins or to the end user.

## Host interfaces

Host interfaces provide methods to contact external APIs and servers
through the application host environment.
asterai provides host interfaces for HTTP, WebSocket, LLM, and Vector DB.

[1]: ./cli
[2]: ./runtime
[3]: ./plugin-examples
