//! M2 acceptance (without a GPU): cancelling a chat closes the HTTP stream,
//! which is what makes `llama-server` stop generating; a server that dies
//! mid-answer is reported as an interruption, with the partial text kept.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use lokalsi_engines::llama::{ChatError, ChatMessage, ChatParams, stream_chat_at};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Serves one SSE response: `tokens` deltas every 50 ms, then either `[DONE]`
/// or (with `die_after`) an abrupt close. Sets `closed` when the client hangs up.
async fn fake_server(tokens: usize, die_after: Option<usize>) -> (String, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let closed = Arc::new(AtomicBool::new(false));
    let flag = closed.clone();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await.unwrap();
        let head = String::from_utf8_lossy(&buf[..n]);
        assert!(
            head.contains("authorization: Bearer secret")
                || head.contains("Authorization: Bearer secret"),
            "{head}"
        );
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        for i in 0..tokens {
            if die_after == Some(i) {
                return; // drop the socket: engine crashed
            }
            let event =
                format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"t{i} \"}}}}]}}\n\n");
            if socket.write_all(event.as_bytes()).await.is_err() {
                flag.store(true, Ordering::SeqCst);
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let end = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"timings\":{\"predicted_n\":3,\"predicted_per_second\":60.0}}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":120,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n";
        let _ = socket.write_all(end.as_bytes()).await;
    });
    (base, closed)
}

fn history() -> Vec<ChatMessage> {
    vec![ChatMessage::text("user", "Cześć")]
}

#[tokio::test]
async fn streams_a_full_answer() {
    let (base, _) = fake_server(3, None).await;
    let client = reqwest::Client::new();
    let mut seen = String::new();
    let out = stream_chat_at(
        &client,
        &base,
        "secret",
        &history(),
        &[],
        &ChatParams::default(),
        &CancellationToken::new(),
        &mut |c, _| seen.push_str(c),
    )
    .await
    .unwrap();
    assert_eq!(out.content, "t0 t1 t2 ");
    assert_eq!(seen, out.content);
    assert_eq!(out.tokens, Some(3));
    assert_eq!(out.tokens_per_second, Some(60.0));
    assert_eq!(
        out.prompt_tokens,
        Some(120),
        "usage chunk after the last delta"
    );
    assert!(!out.cancelled);
}

#[tokio::test]
async fn cancel_closes_the_stream() {
    let (base, closed) = fake_server(200, None).await;
    let client = reqwest::Client::new();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let mut count = 0;
    let out = stream_chat_at(
        &client,
        &base,
        "secret",
        &history(),
        &[],
        &ChatParams::default(),
        &cancel,
        &mut |_, _| {
            count += 1;
            if count == 2 {
                trigger.cancel();
            }
        },
    )
    .await
    .unwrap();
    assert!(out.cancelled);
    assert!(
        out.content.starts_with("t0 t1"),
        "partial text kept: {}",
        out.content
    );
    // The server notices the closed connection on its next write.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        closed.load(Ordering::SeqCst),
        "generation must stop when the user cancels"
    );
}

#[tokio::test]
async fn engine_death_is_an_interruption() {
    let (base, _) = fake_server(10, Some(2)).await;
    let client = reqwest::Client::new();
    let mut seen = String::new();
    let err = stream_chat_at(
        &client,
        &base,
        "secret",
        &history(),
        &[],
        &ChatParams::default(),
        &CancellationToken::new(),
        &mut |c, _| seen.push_str(c),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ChatError::Interrupted), "{err:?}");
    assert_eq!(
        seen, "t0 t1 ",
        "partial output was delivered before the crash"
    );
}
