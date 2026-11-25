# cw-mcp

A Model Context Protocol server over standard input and output, serving a program's commands
as a list of typed tools through the same entry point its command line calls. Synchronous on
the outside: the server builds its own runtime, and every tool's function is a plain
function that runs on a blocking thread. Mechanism only: it names no command, record or
error code of any project.

| Item | What it does |
|---|---|
| `Tool::new::<I>(name, description, access, f)` | A tool: its input type `I` (its schema derived with `schemars`), a read-only or mutating mark, and `f(&Runtime<S>, &Origin, I) -> Result<Value, cw_base::Error>` |
| `Access` | `ReadOnly` or `Mutating`, advertised as the tool's read-only hint |
| `ServerInfo::new(name, version, panic_code)` | What the server calls itself, and the program's code for a tool that panicked |
| `Server::new(info, runtime, tools)` | Checks the tool list once: two tools with one name, or an input with no object schema, are refused |
| `serve_stdio(info, runtime, tools)` | Builds the server and serves stdio; blocks until standard input closes |
| `payload::{structured, tool_error}` | The wire shapes, for a surface that renders the same results |
| `testing::Harness` (feature `test-support`) | An in-process client over an in-memory stream, sending raw JSON-RPC lines |

## A tool is a mapping onto the program's command

```rust,ignore
let tools = vec![
    Tool::new::<AddInput>("note_add", "Add a note.", Access::Mutating, |rt, origin, input| {
        let (result, _warning) = cw_app::exec::<Notes>(rt, origin, input.into()).settle();
        result.map(payload::note)                  // the same JSON `--json` prints
    }),
];
cw_mcp::serve_stdio(ServerInfo::new("notes", "0.1.0", "notes_internal"), runtime, tools)?;
```

- **The schema is the parse.** A tool's input schema is derived from the type its function
  receives, so the schema a client reads and the parse a call goes through cannot drift.
  The type's `Deserialize` is the only check: no JSON Schema validator runs. Do not put a
  `schemars` attribute Serde does not enforce (`range`, `length`, `pattern`) on a tool's
  input, because the schema would advertise a limit a call could break and still run. A
  value limit goes in a field type whose `Deserialize` refuses what breaks it.
- **The origin.** Each call's `Origin` names the `mcp` surface and the client's name from
  its `initialize` handshake, exactly as sent; `cw_app`'s actor rule decides what an audit
  line records of it.
- **Standard output is the protocol.** A tool's function must not print to it.

## On the wire

- **`initialize`** advertises the tools capability only.
- **`tools/list`** gives each tool's name, description, input schema (JSON Schema 2020-12,
  an object) and `annotations.readOnlyHint`, in the order the tools were given.
- **A tool that succeeded** answers with its JSON as `structuredContent`. A list is wrapped
  as `{"items": [...]}` and any other non-object value as `{"value": ...}`, because some
  clients refuse structured content that is not a record.
- **A tool that failed** answers with a normal result marked `isError: true`, whose
  `structuredContent` is:

  ```json
  {"code": "store_locked", "message": "the store is locked by another process", "kind": "busy", "retryable": true}
  ```

  `code` is the program's own; `kind` is the `cw_base::Kind` in `snake_case`; `retryable`
  is true only for `busy`.
- **A tool that panicked** answers the same way, with kind `internal` and the code given to
  `ServerInfo::new`. The server goes on serving.
- **A request the server cannot route** (an unknown tool, or arguments that do not match the
  input schema, including arguments that are not an object or are an explicit `null`) is a JSON-RPC error `-32602`
  with the request's id, and no tool runs.
- **Framing is the protocol SDK's** (`rmcp`), including where it departs from JSON-RPC 2.0:
  a line that is not complete JSON is ignored rather than answered with `-32700`, and a
  well-formed object that is not a request is answered `-32600` without an `id` rather than
  with `"id": null`. A client that writes whole JSON-RPC lines never meets either.
- **Closing standard input** ends the session: `serve_stdio` returns `Ok(())`, also when the
  client leaves before the handshake. The protocol SDK reports a failure to read the input
  the same way, so it also ends with `Ok(())`; `Error::Connection` means the handshake
  failed or the session ended abnormally.
- **Shutdown is bounded.** After the input closes, answers in flight get up to 5 s to be
  written, and a tool function still running gets 2 s more; then it is abandoned on its
  thread, its answer is never sent, and `serve_stdio` returns. A function that changes data
  should leave it whole whether or not it runs to the end.

## Tests in a project

With the `test-support` feature, `testing::Harness` serves a `Server` on a thread of its own
and holds the client end of an in-memory stream:

```rust,ignore
let mut harness = Harness::start(server)?;
harness.initialize("test-client")?;
let reply = harness.request(&json!({
    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
    "params": { "name": "note_add", "arguments": { "text": "hi" } }
}))?;
assert_eq!(reply["result"]["structuredContent"]["text"], "hi");
harness.close()??;
```

Every wait is bounded (10 s by default, `Harness::with_deadline` to change it).

## Left out

Other transports (TCP, an SSH proxy), toolsets and roles, presentation resources, prompts,
and handing a session over to a new process: each is one program's own.

## Example

`cargo run -p cw-mcp --example two_tools` serves `tally_add` and `tally_show` over one
in-memory tally, both through `cw_app::exec`.
