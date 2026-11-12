//! MCP and onboarding smoke tests: the real binary, real stdio, no models.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sonolytica"))
}

#[test]
fn mcp_initializes_lists_tools_and_calls_one() {
    let mut child = binary()
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    for request in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_models","arguments":{}}}"#,
    ] {
        writeln!(stdin, "{request}").expect("write request");
    }
    drop(stdin);

    let stdout = child.stdout.take().expect("stdout");
    let mut lines = BufReader::new(stdout).lines();
    let init: serde_json::Value =
        serde_json::from_str(&lines.next().expect("init line").unwrap()).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "sonolytica");
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");

    let tools: serde_json::Value =
        serde_json::from_str(&lines.next().expect("tools line").unwrap()).unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"analyze_media"));
    assert!(names.contains(&"query_ask"));
    assert!(names.contains(&"query_captions"));
    assert!(names.contains(&"query_verify"));

    let call: serde_json::Value =
        serde_json::from_str(&lines.next().expect("call line").unwrap()).unwrap();
    assert_eq!(call["result"]["isError"], false);
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("model directory"), "{text}");
    let _ = child.wait();
}

#[test]
fn setup_without_network_or_skill_prints_the_next_steps() {
    let output = binary()
        .args(["setup", "--no-models", "--no-skill"])
        .output()
        .expect("run setup");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("SONOLYTICA setup"));
    assert!(text.contains("sonolytica query brief"));
}

#[test]
fn completions_are_generated() {
    let output = binary()
        .args(["completions", "bash"])
        .output()
        .expect("run completions");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("sonolytica"),
        "{}",
        &text[..text.len().min(200)]
    );
    assert!(
        text.contains("complete"),
        "{}",
        &text[..text.len().min(200)]
    );
}
