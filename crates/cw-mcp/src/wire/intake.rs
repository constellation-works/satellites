//! The input on its way to the protocol SDK, with one mark added.
//!
//! The SDK decodes a `tools/call` whose `arguments` is an explicit `null`
//! the same as one that leaves `arguments` out, so the handler cannot tell
//! them apart and would run the tool on an empty object. Before the SDK
//! reads a line, such an `arguments` is replaced with `false`: the SDK then
//! cannot read the request as a `tools/call`, and the handler answers it as
//! invalid parameters (`-32602`) without running a tool. Every other byte
//! passes through unchanged.

use serde_json::Value;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader};

/// The method whose `arguments` the pump inspects.
const CALL: &str = "tools/call";

/// Copy `input` to `pipe` one line at a time, marking explicit null
/// arguments, until the input ends or fails; then close the pipe, which
/// the SDK reads as the input closing.
pub(crate) async fn pump<R, W>(input: R, mut pipe: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(input);
    let mut line = Vec::new();
    loop {
        line.clear();
        // A read error ends the session as the input closing does: the SDK
        // reports both the same way.
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let marked = mark_null_arguments(&line);
        if pipe
            .write_all(marked.as_deref().unwrap_or(&line))
            .await
            .is_err()
        {
            break;
        }
    }
    // The SDK may have gone already; then nobody reads the pipe.
    let _ = pipe.shutdown().await;
}

/// The line with each `tools/call`'s explicit null `arguments` replaced,
/// or `None` when it holds none.
fn mark_null_arguments(line: &[u8]) -> Option<Vec<u8>> {
    if !contains(line, CALL.as_bytes()) || !contains(line, b"null") {
        return None;
    }
    let mut message: Value = serde_json::from_slice(line).ok()?;
    // Every request in a batch is marked, so this does not stop at the first.
    let mut marked = false;
    if let Value::Array(batch) = &mut message {
        for one in batch {
            marked |= mark(one);
        }
    } else {
        marked = mark(&mut message);
    }
    if !marked {
        return None;
    }
    let mut out = serde_json::to_vec(&message).ok()?;
    out.push(b'\n');
    Some(out)
}

/// Replace one request's explicit null `arguments`, saying whether it had
/// one.
fn mark(message: &mut Value) -> bool {
    if message.get("method").and_then(Value::as_str) != Some(CALL) {
        return false;
    }
    let Some(params) = message.get_mut("params").and_then(Value::as_object_mut) else {
        return false;
    };
    if !params.get("arguments").is_some_and(Value::is_null) {
        return false;
    }
    params.insert("arguments".to_owned(), Value::Bool(false));
    true
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
