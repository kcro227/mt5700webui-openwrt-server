// use async_trait::async_trait;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast};
use tokio::time::{sleep, timeout};

use crate::config::Config;
use crate::at::connection::{ATConnection, NetworkATConn, SerialATConn, TomModemATConn};
//  connection::ATConnection;

pub struct ATClient {
    pub conn: Arc<Mutex<Box<dyn ATConnection>>>,
    pub urc_tx: broadcast::Sender<String>,
    // pub config: Arc<Config>,
}

impl ATClient {
    pub fn new(config: &Arc<Config>) -> Result<Self, Box<dyn Error>> {
        let at_config = &config.at_config;

        let conn: Box<dyn ATConnection> = if at_config.conn_type == "NETWORK" {
            Box::new(NetworkATConn::new(at_config.network.clone()))
        } else {
            if at_config.serial.method == "TOM_MODEM" {
                Box::new(TomModemATConn::new(
                    at_config.serial.port.clone(),
                    at_config.serial.timeout,
                    at_config.serial.feature.clone(),
                ))
            } else {
                Box::new(SerialATConn::new(at_config.serial.clone()))
            }
        };

        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            urc_tx: tx,
            // config,
        })
    }

    /// 发送 AT 命令并等待响应
    pub async fn send_command(
        &self,
        mut command: String,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut conn = self.conn.lock().await;
        let original_cmd = command.trim().to_string();
        if !command.ends_with("\r\n") {
            command = command.trim_end().to_string();
            command.push_str("\r\n");
        }

        // 1. 清理旧残留，防止 ping 干扰指令结果
        while let Ok(d) = timeout(Duration::from_millis(10), conn.receive())
            .await
            .unwrap_or(Ok(vec![]))
        {
            if d.is_empty() {
                break;
            }
        }

        println!("[DEBUG] ==> TX: {:?}", command);
        conn.send(command.as_bytes()).await?;

        let mut raw_response = String::new();
        let start = std::time::Instant::now();

        // 2. 超时设为 1000ms
        while start.elapsed() < Duration::from_millis(1000) {
            if let Ok(data) = conn.receive().await {
                if !data.is_empty() {
                    raw_response.push_str(&String::from_utf8_lossy(&data));
                    // 如果看到 OK 或 ERROR，说明指令响应结束
                    if raw_response.contains("OK\r\n") || raw_response.contains("ERROR") {
                        break;
                    }
                }
            }
            sleep(Duration::from_millis(10)).await;
        }

        let mut cleaned = raw_response.replace("ping", "").trim().to_string();
        if cleaned.trim_start().starts_with(&original_cmd) {
            if let Some(pos) = cleaned.find('\n') {
                cleaned = cleaned[(pos + 1)..].to_string();
            }
        }

        let result = cleaned.trim().to_string();
        println!("[DEBUG] <== RX: {:?}", result);

        // 如果结果包含 ERROR，返回 Err 分支
        if result.contains("ERROR") {
            return Err("ERROR".into());
        }

        if result.is_empty() && start.elapsed() >= Duration::from_millis(1000) {
            return Err("TIMEOUT".into());
        }

        Ok(result)
    }

    /// 初始化模块（ATE0, CNMI, CMGF, CLIP）
    pub async fn init_module(&self) {
        let _ = self.send_command("ATE0".into()).await;
        let _ = self.send_command("AT+CNMI=2,1,0,2,0".into()).await;
        let _ = self.send_command("AT+CMGF=0".into()).await;
        let _ = self.send_command("AT+CLIP=1".into()).await;
    }
}