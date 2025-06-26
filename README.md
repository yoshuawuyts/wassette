# weld-mcp-server

**Dynamically load and run WebAssembly MCP tools in secure sandboxes.** Weld transforms your AI agent workflows from risky native tool execution to secure, isolated environments where you control exactly what tools can access.

<!-- insert demo gif here -->

It's an open-source MCP server that works as a CLI tool with Claude Desktop, Cursor, VSCode, and other MCP-compatible agents.

- 🔧 **Dynamic Loading**: Load WebAssembly components on-demand from OCI registries, URLs, or local files - no restart required.
- 🔒 **Secure Sandboxes**: Each tool runs in isolated WebAssembly environments with capability-based policies controlling file/network access.
- 🎯 **Runtime Introspection**: Automatically discover tool capabilities and exported functions without manual configuration.
- 🧩 **Composable Tools**: Mix and match components from different sources in real-time - build workflows dynamically.
- 🚀 **Developer-Friendly**: Write functions that compile to WASM components, not entire servers - focus on logic, not infrastructure.
- ⚡ **Hot-Swap Tools**: Load, unload, and replace components without downtime - perfect for experimentation and development.

🦺 This project is in early development and actively evolving. Expect rapid iteration, breaking changes, and responsiveness to feedback. Please submit issues or reach out with questions!

## Install

### All Platforms (Shell Script)

```bash
curl -fsSL https://raw.githubusercontent.com/semcp/weld-mcp-server/main/install.sh | bash
```

This will detect your platform and install the latest `weld-mcp-server` binary to your `$PATH`.

## Integrate with MCP Clients

### [VSCode](https://code.visualstudio.com/docs/copilot/chat/mcp-servers) / [GitHub Copilot](https://docs.github.com/en/copilot/customizing-copilot/extending-copilot-chat-with-mcp) / [Cursor](https://docs.cursor.com/context/model-context-protocol)

Add this to your VSCode or Cursor settings:

```json
"mcp": {
  "servers": {
    "weld": {
      "type": "sse",
      "url": "http://127.0.0.1:9001/sse"
    }
  }
}
```

## Quick Start

1. **Start the weld server:**

   ```bash
   weld-mcp-server serve --http --policy-file policy.yaml
   ```

2. **Dynamically load tools:**

   **From OCI Registry:**
   <!-- update to point to weld pkgs -->

   ```
   Load the filesystem tools from oci://ghcr.io/duffney/filesystem:latest
   ```

   **From Local File:**

   ```
   Load component from file:///path/to/my-tools.wasm
   ```

3. **Use the newly loaded tools immediately:**

   ```
   Use the read-file tool to get the contents of the Justfile at the root of this repo
   ```

   The tools are now available in your AI client's tool list - no restart required! Weld automatically detects what functions each component exports and makes them available as MCP tools.

**Built-in Tools for Dynamic Loading:**

- `load-component` - Load WebAssembly components from any source
- `unload-component` - Remove components from the runtime

## Examples

| Example                                    | Description                                            |
| ------------------------------------------ | ------------------------------------------------------ |
| [fetch-rs](examples/fetch-rs/)             | HTTP client for making web requests                    |
| [filesystem-rs](examples/filesystem-rs/)   | File system operations (read, write, list directories) |
| [eval-py](examples/eval-py/)               | Python code execution sandbox                          |
| [get-weather-js](examples/get-weather-js/) | Weather API client for fetching weather data           |
| [time-server-js](examples/time-server-js/) | Simple time server component                           |
| [gomodule-go](examples/gomodule-go/)       | Go module information tool                             |

See the `examples/` directory for more components you can build and load dynamically.

## How it works

Unlike traditional MCP servers that require pre-installed tools, `weld` **dynamically loads WebAssembly components at runtime** - letting your agents discover, load, and use new tools on-demand from OCI registries or local files. Each component runs in an isolated WebAssembly sandbox with no direct host access, and security is enforced through YAML policy files that define exactly what each component can access - filesystem paths, network endpoints, and environment variables. This deny-by-default approach provides container-level security with WebAssembly performance, giving you complete observability and control over tool composition.

## Contributing

We welcome contributions! Check our [issues](https://github.com/semcp/weld-mcp-server/issues) for open tasks or create a new issue to discuss your ideas. Submit pull requests to the `main` branch - we'll review them promptly.
