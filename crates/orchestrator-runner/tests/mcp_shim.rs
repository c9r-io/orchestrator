//! Offline conformance test for the stdio-to-daemon MCP transport shim.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

#[test]
fn stdio_shim_forwards_json_rpc_with_run_token() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept callback");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).expect("read callback request");
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            let text = String::from_utf8_lossy(&request);
            let Some(header_end) = text.find("\r\n\r\n") else {
                continue;
            };
            let content_length = text[..header_end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .expect("content length");
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        let text = String::from_utf8(request).expect("HTTP request");
        assert!(text.contains("authorization: Bearer run-token-118"));
        assert!(text.contains("\"method\":\"tools/list\""));
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write callback response");
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_orch-mcp-tools"))
        .env("ORCH_MCP_CALLBACK_URL", format!("http://{address}/mcp"))
        .env("ORCH_MCP_CALLBACK_TOKEN", "run-token-118")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn shim");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n")
        .expect("write request");
    let output = child.wait_with_output().expect("wait for shim");
    server.join().expect("callback server");
    assert!(output.status.success());
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("shim JSON response");
    assert_eq!(response["result"]["tools"], serde_json::json!([]));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("run-token-118"));
}
