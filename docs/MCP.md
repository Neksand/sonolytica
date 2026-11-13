# MCP server

`sonolytica mcp` is a tools-only Model Context Protocol server over **stdio**.
It is for clients that cannot run a shell: Claude Desktop, Cursor, IDE agents
and any other MCP client.

The server is deliberately thin: every tool runs the same `sonolytica` binary
with the same arguments and returns its JSON. There is no second query
implementation and no model loaded in the server. Analysis and queries use the
same local model store and cache.

## Configure a client

Any MCP stdio client needs only the command and one argument:

```json
{
  "mcpServers": {
    "sonolytica": {
      "command": "sonolytica",
      "args": ["mcp"]
    }
  }
}
```

Claude Desktop uses `claude_desktop_config.json`; Cursor and other clients use
their own MCP settings file with the same `command` / `args` shape. If the
binary is not on the client's `PATH`, use its absolute path.

On Windows, use `sonolytica.exe` (or the absolute path with escaped
backslashes):

```json
{
  "mcpServers": {
    "sonolytica": {
      "command": "C:\\Users\\you\\AppData\\Local\\sonolytica\\bin\\sonolytica.exe",
      "args": ["mcp"]
    }
  }
}
```

Test it without a client:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' |
  sonolytica mcp
```

The server reads one JSON-RPC object per line and writes one response per
line. Notifications (`notifications/initialized`) get no response. It echoes
the client's protocol version, advertises only the `tools` capability, and
returns an empty `resources/list`.

## Tools

| Tool | Arguments | What it returns |
|---|---|---|
| `analyze_media` | `media`, `out`, optional `profile`, `language`, `model`, `ocr`, `faces`, `objects`, `pose`, `caption`, `align`, `no_frames`, `max_frames`, `frame_stride` | Runs analysis; the document path and a next-step hint |
| `query_brief` | `document` | Compact overview: media, runs, speech, speakers, chapters, counts |
| `query_ask` | `document`, `question`, optional `budget`, `max` | Ranked evidence plus absent terms; retrieval, not an answer |
| `query_timeline` | `document`, optional `from`, `to`, `type` | Segments in a window |
| `query_search` | `document`, `needle` | Exact matches with ranges and speakers |
| `query_transcript` | `document`, optional `from`, `to` | Full transcript |
| `query_words` | `document`, `segment` | Every word of one segment |
| `query_speakers` | `document` | Speaking time per speaker |
| `query_captions` | `document`, optional `max` | Model-generated keyframe captions |
| `query_poses` | `document`, optional `max` | Body keypoints per person per keyframe |
| `query_runs` | `document` | Stage status and warnings |
| `query_verify` | `document`, `claims` (JSON string) | Mechanical quote/range verification |
| `list_models` | — | Pinned models, licenses, verification status |
| `doctor` | — | FFmpeg, providers, build check |

`tools/call` returns `content: [{type:"text", text: "<CLI output>"}]`. A failed
CLI call sets `isError: true` and includes stderr; the MCP client should surface
that instead of inventing a result.

## Example

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call",
 "params":{"name":"query_brief","arguments":{"document":"/tmp/meeting.json"}}}
```

A typical shell-less agent flow is `analyze_media` → `query_brief` →
`query_ask` or `query_search` → `query_words` / `query_verify`.

## Limits

- **Tools only.** No MCP prompts, resources or sampling yet; `resources/list`
  is empty. `/tmp` documents can be passed as ordinary paths.
- **Blocking calls.** `analyze_media` waits for the full analysis. Clients with
  a short tool timeout should start it in the background or pre-analyze.
- **Local and private.** The server never uploads media or documents and never
  calls a cloud model.
- **No shell interpolation.** Arguments are passed to the binary directly.
- **Streamable HTTP and the official `rmcp` SDK are not implemented yet.**
  This is a small stdio server that speaks the tools subset of MCP; it is
  enough for Claude Desktop, Cursor and IDE agents.
