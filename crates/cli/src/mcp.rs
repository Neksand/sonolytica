//! Minimal MCP server over stdio.
//!
//! MCP is the interface for clients that cannot run a shell (Claude Desktop,
//! Cursor, IDE agents). The server speaks newline-delimited JSON-RPC 2.0 and
//! exposes the same operations as `sonolytica query`; it shells out to this
//! same binary so there is only one implementation of every query.
//!
//! Protocol: MCP `initialize`, `notifications/initialized`, `tools/list`,
//! `tools/call`, `ping`. We echo the client's protocol version, which is the
//! portable choice for a tools-only server.

use std::io::{BufRead, Write};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::{Value, json};

const PROTOCOL_FALLBACK: &str = "2025-06-18";

pub fn run() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.context("could not read MCP input")?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("mcp: invalid JSON: {error}");
                continue;
            }
        };
        if let Some(response) = handle(&request) {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle(request: &Value) -> Option<Value> {
    let method = request.get("method")?.as_str()?;
    let id = request.get("id").cloned();
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    // Notifications have no id and never get a response.
    id.as_ref()?;

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_FALLBACK),
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "sonolytica",
                "version": env!("CARGO_PKG_VERSION"),
            }
        })),
        "ping" => Ok(json!({})),
        "resources/list" => Ok(json!({ "resources": [] })),
        "tools/list" => Ok(json!({ "tools": tool_specs() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str);
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            match name {
                Some(name) => Ok(call_tool(name, &arguments)),
                None => Err((-32602, "tools/call needs a tool name".to_string())),
            }
        }
        _ => Err((-32601, format!("unknown method `{method}`"))),
    };

    match result {
        Ok(result) => Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
        Err((code, message)) => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        })),
    }
}

fn call_tool(name: &str, arguments: &Value) -> Value {
    match run_tool(name, arguments) {
        Ok(text) => json!({
            "content": [{ "type": "text", "text": text }],
            "isError": false
        }),
        Err(error) => json!({
            "content": [{ "type": "text", "text": error }],
            "isError": true
        }),
    }
}

fn run_tool(name: &str, arguments: &Value) -> std::result::Result<String, String> {
    match name {
        "analyze_media" => {
            let media = required_str(arguments, "media")?;
            let out = required_str(arguments, "out")?;
            let mut args = vec![
                "analyze".to_string(),
                media.to_string(),
                "--out".to_string(),
                out.to_string(),
            ];
            if let Some(profile) = optional_str(arguments, "profile") {
                args.push("--profile".into());
                args.push(profile.to_string());
            }
            if let Some(language) = optional_str(arguments, "language") {
                args.push("--language".into());
                args.push(language.to_string());
            }
            if let Some(model) = optional_str(arguments, "model") {
                args.push("--model".into());
                args.push(model.to_string());
            }
            for (key, flag) in [
                ("ocr", "--ocr"),
                ("faces", "--faces"),
                ("objects", "--objects"),
                ("pose", "--pose"),
                ("caption", "--caption"),
                ("align", "--align"),
                ("no_frames", "--no-frames"),
            ] {
                if optional_bool(arguments, key) {
                    args.push(flag.into());
                }
            }
            if let Some(max) = optional_u64(arguments, "max_frames") {
                args.push("--max-frames".into());
                args.push(max.to_string());
            }
            if let Some(stride) = optional_f64(arguments, "frame_stride") {
                args.push("--frame-stride".into());
                args.push(stride.to_string());
            }
            run_cli(&args)?;
            Ok(format!("analyzed {media} -> {out}; next: query_brief"))
        }
        "query_brief" => query(arguments, "brief", &[]),
        "query_ask" => {
            let question = required_str(arguments, "question")?;
            let mut extra = vec![question.to_string()];
            if let Some(budget) = optional_u64(arguments, "budget") {
                extra.push("--budget".into());
                extra.push(budget.to_string());
            }
            if let Some(max) = optional_u64(arguments, "max") {
                extra.push("--max".into());
                extra.push(max.to_string());
            }
            query(arguments, "ask", &extra)
        }
        "query_timeline" => {
            let mut extra = Vec::new();
            push_opt(&mut extra, "--from", optional_str(arguments, "from"));
            push_opt(&mut extra, "--to", optional_str(arguments, "to"));
            push_opt(&mut extra, "--type", optional_str(arguments, "type"));
            query(arguments, "timeline", &extra)
        }
        "query_search" => {
            let needle = required_str(arguments, "needle")?;
            query(arguments, "search", &[needle.to_string()])
        }
        "query_transcript" => {
            let mut extra = Vec::new();
            push_opt(&mut extra, "--from", optional_str(arguments, "from"));
            push_opt(&mut extra, "--to", optional_str(arguments, "to"));
            query(arguments, "transcript", &extra)
        }
        "query_words" => {
            let segment = arguments
                .get("segment")
                .and_then(Value::as_u64)
                .ok_or("query_words needs `segment`")?;
            query(arguments, "words", &[segment.to_string()])
        }
        "query_speakers" => query(arguments, "speakers", &[]),
        "query_captions" => {
            let mut extra = Vec::new();
            push_opt(
                &mut extra,
                "--max",
                optional_u64(arguments, "max")
                    .map(|v| v.to_string())
                    .as_deref(),
            );
            query(arguments, "captions", &extra)
        }
        "query_poses" => {
            let mut extra = Vec::new();
            push_opt(
                &mut extra,
                "--max",
                optional_u64(arguments, "max")
                    .map(|v| v.to_string())
                    .as_deref(),
            );
            query(arguments, "poses", &extra)
        }
        "query_runs" => query(arguments, "runs", &[]),
        "query_verify" => {
            let document = required_str(arguments, "document")?;
            let claims = required_str(arguments, "claims")?;
            let path = std::env::temp_dir()
                .join(format!("sonolytica-mcp-claims-{}.json", std::process::id()));
            std::fs::write(&path, claims).map_err(|error| error.to_string())?;
            let result = run_cli(&[
                "query".into(),
                "verify".into(),
                document.into(),
                "--claims".into(),
                path.to_string_lossy().into_owned(),
                "--json".into(),
            ]);
            let _ = std::fs::remove_file(&path);
            result
        }
        "list_models" => run_cli(&["models".into(), "list".into()]),
        "doctor" => run_cli(&["doctor".into()]),
        _ => Err(format!("unknown tool `{name}`")),
    }
}

fn query(
    arguments: &Value,
    command: &str,
    extra: &[String],
) -> std::result::Result<String, String> {
    let document = required_str(arguments, "document")?;
    let mut args = vec![
        "query".to_string(),
        command.to_string(),
        document.to_string(),
    ];
    args.extend(extra.iter().cloned());
    args.push("--json".into());
    run_cli(&args)
}

fn run_cli(args: &[String]) -> std::result::Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(exe)
        .args(args)
        .output()
        .map_err(|error| format!("could not run sonolytica: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

fn required_str<'a>(arguments: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string argument `{key}`"))
}

fn optional_str<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(Value::as_str)
}

fn optional_bool(arguments: &Value, key: &str) -> bool {
    arguments.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn optional_u64(arguments: &Value, key: &str) -> Option<u64> {
    arguments.get(key).and_then(Value::as_u64)
}

fn optional_f64(arguments: &Value, key: &str) -> Option<f64> {
    arguments.get(key).and_then(Value::as_f64)
}

fn push_opt(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        args.push(flag.into());
        args.push(value.into());
    }
}

fn tool_specs() -> Vec<Value> {
    let document = json!({ "type": "string", "description": "Document produced by analyze_media" });
    let bools = [
        ("ocr", "Read on-screen text"),
        ("faces", "Detect faces (count/boxes only)"),
        ("objects", "Detect COCO objects"),
        ("pose", "Detect 17 COCO body keypoints"),
        ("caption", "Describe keyframes with Florence-2"),
        ("align", "Force-align words (English)"),
        ("no_frames", "Skip keyframe extraction"),
    ];
    let mut analyze_props = serde_json::Map::new();
    analyze_props.insert(
        "media".into(),
        json!({ "type": "string", "description": "Input audio or video path" }),
    );
    analyze_props.insert(
        "out".into(),
        json!({ "type": "string", "description": "Output document path" }),
    );
    analyze_props.insert(
        "profile".into(),
        json!({ "type": "string", "enum": ["structure", "standard", "ultra"] }),
    );
    analyze_props.insert("language".into(), json!({ "type": "string" }));
    analyze_props.insert("model".into(), json!({ "type": "string" }));
    analyze_props.insert(
        "max_frames".into(),
        json!({ "type": "integer", "minimum": 1 }),
    );
    analyze_props.insert(
        "frame_stride".into(),
        json!({ "type": "number", "minimum": 0 }),
    );
    for (key, description) in bools {
        analyze_props.insert(
            key.into(),
            json!({ "type": "boolean", "description": description }),
        );
    }

    vec![
        tool(
            "analyze_media",
            "Analyze a local media file into a typed SONOLYTICA document. Long-running; call query_brief afterwards.",
            json!({ "type": "object", "properties": analyze_props, "required": ["media", "out"], "additionalProperties": false }),
        ),
        tool(
            "query_brief",
            "One compact overview: media, runs, speech, speakers, chapters, counts. Start here.",
            doc_schema(document.clone(), &[]),
        ),
        tool(
            "query_ask",
            "Deterministic evidence pack for a question; also names absent terms. Not an answer.",
            json!({ "type": "object", "properties": {
                "document": document.clone(),
                "question": { "type": "string" },
                "budget": { "type": "integer", "minimum": 100 },
                "max": { "type": "integer", "minimum": 1 }
            }, "required": ["document", "question"], "additionalProperties": false }),
        ),
        tool(
            "query_timeline",
            "Segments in a time window, with optional kind filter.",
            json!({ "type": "object", "properties": {
                "document": document.clone(),
                "from": { "type": "string" },
                "to": { "type": "string" },
                "type": { "type": "string" }
            }, "required": ["document"], "additionalProperties": false }),
        ),
        tool(
            "query_search",
            "Case-insensitive search over speech, speaker turns, OCR and captions.",
            json!({ "type": "object", "properties": {
                "document": document.clone(),
                "needle": { "type": "string" }
            }, "required": ["document", "needle"], "additionalProperties": false }),
        ),
        tool(
            "query_transcript",
            "The whole transcript, untruncated.",
            doc_schema(document.clone(), &["from", "to"]),
        ),
        tool(
            "query_words",
            "Every word of one speech segment, with its own range.",
            json!({ "type": "object", "properties": {
                "document": document.clone(),
                "segment": { "type": "integer", "minimum": 0 }
            }, "required": ["document", "segment"], "additionalProperties": false }),
        ),
        tool(
            "query_speakers",
            "Speaking time per speaker, longest first.",
            doc_schema(document.clone(), &[]),
        ),
        tool(
            "query_captions",
            "Model-generated keyframe captions. Observations, not OCR.",
            doc_schema(document.clone(), &["max"]),
        ),
        tool(
            "query_poses",
            "Body keypoints per person per keyframe.",
            doc_schema(document.clone(), &["max"]),
        ),
        tool(
            "query_runs",
            "Stage status, provider and warnings.",
            doc_schema(document.clone(), &[]),
        ),
        tool(
            "query_verify",
            "Mechanically verify quotes and ranges; claims is a JSON document string.",
            json!({ "type": "object", "properties": {
                "document": document,
                "claims": { "type": "string" }
            }, "required": ["document", "claims"], "additionalProperties": false }),
        ),
        tool(
            "list_models",
            "Pinned models, license and local verification status.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "doctor",
            "Environment, FFmpeg, providers and build check.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn doc_schema(document: Value, optional: &[&str]) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert("document".into(), document);
    for key in optional {
        let schema = if *key == "max" {
            json!({ "type": "integer", "minimum": 1 })
        } else {
            json!({ "type": "string" })
        };
        properties.insert((*key).into(), schema);
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": ["document"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_echoes_the_protocol_and_advertises_tools() {
        let response = handle(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-06-18" }
        }))
        .unwrap();
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(response["result"]["serverInfo"]["name"], "sonolytica");
    }

    #[test]
    fn tools_list_has_the_core_tools() {
        let response = handle(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }))
        .unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(names.contains(&"analyze_media"));
        assert!(names.contains(&"query_brief"));
        assert!(names.contains(&"query_ask"));
        assert!(names.contains(&"query_captions"));
        assert!(names.contains(&"query_verify"));
    }

    #[test]
    fn unknown_method_is_a_jsonrpc_error_with_the_same_id() {
        let response = handle(&json!({
            "jsonrpc": "2.0",
            "id": "abc",
            "method": "nope"
        }))
        .unwrap();
        assert_eq!(response["id"], "abc");
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn notifications_get_no_response() {
        assert!(
            handle(&json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .is_none()
        );
    }
}
