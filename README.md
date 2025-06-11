# weld-mcp-server

##  🚩 Purpose

### Problem Statement

A popular use scenario for MCP today is to run the server as a standalone process on a machine that talks to clients over stdio or a socket. Running these servers natively poses security risks, such as "my mcp-client calls the tool to read a file, and for some reason the tool writes to a file on my machine", "the tool opens a socket to a remote machine and sends data to it", or "I downloaded this open source mcp-server, but it has a vulnerability that allows an attacker to execute arbitrary code on my machine". This is the same problem as running untrusted code on your machine, but mcp-clients make them much easier to install on your machine and run.

### Who is the target audience?

1. *Developers* who want to focus on writing the business logic for MCP tools, instead of worrying about the infrastructure.

2. *Devops engineers* who want the tools to be able to run everywhere and have a great observability story and tools are secured by design.

3. *Users* who want to run a trusted mcp-server on their machine that is proven to securely execute untrusted tools.

4. *Platform providers* who want to provide a serverless experience for their users.

### What are the current solutions?

1. Package and distribute the server as Docker images. This is perhaps the most common way to run MCP servers securely today, because it works with existing tooling and infrastructure and requires no changes to the server code. One could argue that containers are not a secure boundary, but they are a good starting point. The harder problem is how to apply security policies to the container like "how do I know what HTTP domain is this tool calling to?". [The Docker MCP Catalog](https://docs.docker.com/ai/mcp-catalog-and-toolkit/catalog/) runs each MCP server as a container - providing isolation and portability.

2. Centralized MCP server that runs WebAssembly-based tools locally. WebAssembly (Wasm) is a great fit for tools because they are tiny, performant binaries that are sandboxed in the runtime. By default, Wasm has no access to the system resources, unless given by imported functions like WASI (WebAssembly System Interface). Registry like [mcp.run](https://mcp.run) is a good example of this approach and I very much like the idea of running tools in tiny sandboxes which incur less memory overhead than containers.



### What is the minimal viable scope that we can achieve?

1. One centralized open-source mcp-server, written in a memory safe, high performance language that embeds a WebAssembly runtime (e.g. [Wasmtime](https://github.com/bytecodealliance/wasmtime) or [hyperlight-wasm](https://github.com/hyperlight-dev/hyperlight-wasm)), acting as a minimal trusted computing base (TCB). We call it `weld`.
3. `weld` will implement allow/deny lists for file paths, network endpoints, and system calls using capability-based policy like [policy-mcp-rs](https://github.com/semcp/policy-mcp-rs).
4. Untrusted tool code will be distributed as WebAssembly OCI artifacts in OCI registries, and be loaded into the trusted layer upon signature verification. Each tool will have a discrete set of capabilities. For example, tool A needs to read `./data`; not network; tool B needs read/write to `/assets` and outbound HTTP only to `api.company.com:443`.

`weld` essentially acts as a *virtual MCP server*, with built-in observability, resource quotas, and handles infrastructure complexity automatically.

### What about the developer experience?

Developers will write MCP tools as normal functions that can be compiled to WebAssembly Components, instead of developing servers. This is a significant paradigm shift and offers a completely different experience than writing server code and I am fully aware that the majority of the code needs to be rewritten for retargeting to Wasm but I am expecting the ecosystem will continue to evolve to maturity.

If you are interested in learning more about what programming language supports WebAssembly, you can check out [this page](https://developer.fermyon.com/wasm-languages/webassembly-language-support).
