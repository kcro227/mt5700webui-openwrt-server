use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::at::ATClient;

/// WebSocket 连接处理器
pub async fn handle_connection(
    stream: TcpStream,
    addr: std::net::SocketAddr,
    client: Arc<ATClient>,
    auth_key: String,
) -> Option<()> {
    let ws_stream = accept_async(stream).await.ok()?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let mut urc_rx = client.urc_tx.subscribe();

    println!("[WebSocket] 新连接: {}", addr);

    // 如果配置了认证密钥，需要先进行认证
    if !auth_key.is_empty() {
        let auth_result = timeout(Duration::from_secs(10), async {
            if let Some(Ok(Message::Text(auth_msg))) = ws_rx.next().await {
                let auth_data: Result<serde_json::Value, _> = serde_json::from_str(&auth_msg);
                if let Ok(auth_data) = auth_data {
                    if let Some(client_key) = auth_data.get("auth_key") {
                        if client_key.as_str() == Some(&auth_key) {
                            return true;
                        }
                    }
                }
            }
            false
        })
        .await
        .unwrap_or(false);

        if !auth_result {
            println!("[WebSocket] 认证失败: {}", addr);
            let _ = ws_tx
                .send(Message::Text(
                    json!({
                        "error": "Authentication failed",
                        "message": "密钥验证失败"
                    })
                    .to_string(),
                ))
                .await;
            return None;
        }

        // 认证成功
        let _ = ws_tx
            .send(Message::Text(
                json!({
                    "success": true,
                    "message": "认证成功"
                })
                .to_string(),
            ))
            .await;
        println!("[WebSocket] 认证成功: {}", addr);
    }

    loop {
        tokio::select! {
            urc_res = urc_rx.recv() => {
                if let Ok(msg) = urc_res {
                    let payload = json!({ "type": "raw_data", "data": msg });
                    if let Ok(json_str) = serde_json::to_string(&payload) {
                        if let Err(_) = ws_tx.send(Message::Text(json_str)).await { break; }
                    }
                }
            }
            msg = ws_rx.next() => {
                if let Some(Ok(Message::Text(cmd))) = msg {
                    let res = match client.send_command(cmd).await {
                        Ok(r) => json!({ "success": true, "data": r, "error": null }),
                        Err(e) => json!({ "success": false, "data": null, "error": e.to_string() }),
                    };
                    let _ = ws_tx.send(Message::Text(serde_json::to_string(&res).unwrap())).await;
                } else { break; }
            }
        }
    }
    println!("[WebSocket] 连接断开: {}", addr);
    Some(())
}