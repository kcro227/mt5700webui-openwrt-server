use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast};
use tokio::time::sleep;

use crate::at::connection::{ATConnection, NetworkATConn, SerialATConn, TomModemATConn};
use crate::config::Config;

/// 单条 AT 命令的最长等待时间。
/// 前端 AtService 的 commandTimeout 也是 1000ms，这里略小一点，
/// 保证服务端先于前端超时返回，前端能拿到明确的错误而不是“命令执行超时”。
const COMMAND_TIMEOUT: Duration = Duration::from_millis(900);
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
const DRAIN_INTERVAL: Duration = Duration::from_millis(10);

fn normalize_command(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.ends_with("\r\n") {
        trimmed.to_string()
    } else {
        format!("{}\r\n", trimmed.trim_end())
    }
}

/// 去掉模组回显和心跳残留。
///
/// 旧实现使用 `replace("ping", "")`，会把短信正文里的 ping（例如 "shipping"）
/// 一并删掉；这里只丢弃“整行等于 ping”的行，并且只删除第一行的命令回显。
fn sanitize_response(response: &str, original_cmd: &str) -> String {
    let mut lines: Vec<&str> = response
        .lines()
        .filter(|line| !line.trim().eq_ignore_ascii_case("ping"))
        .collect();

    if let Some(first) = lines.first() {
        let first = first.trim();
        if !original_cmd.is_empty()
            && (first.eq_ignore_ascii_case(original_cmd) || first.starts_with(original_cmd))
        {
            lines.remove(0);
        }
    }

    lines.join("\n").trim().to_string()
}

/// 只有“最后一条非空行”是结果码时才算命令结束。
/// 不能对整段响应做 `contains("OK")`：短信 PDU/正文里出现 OK、或者 URC 里带 OK，
/// 都会让读取提前结束，把半截数据交给前端。
fn has_terminal_marker(response: &str) -> bool {
    for line in response.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        return line.eq_ignore_ascii_case("OK")
            || line.eq_ignore_ascii_case("ERROR")
            || line.starts_with("+CME ERROR")
            || line.starts_with("+CMS ERROR");
    }
    false
}

/// 提取最终错误码（只认最后一行的结果码）。
fn response_error(response: &str) -> Option<String> {
    for line in response.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("ERROR") {
            return Some("ERROR".to_string());
        }
        if line.starts_with("+CME ERROR") || line.starts_with("+CMS ERROR") {
            return Some(line.to_string());
        }
        return None;
    }
    None
}

async fn drain_stale_data(conn: &mut Box<dyn ATConnection>) {
    let deadline = Instant::now() + Duration::from_millis(50);
    while Instant::now() < deadline {
        match conn.receive().await {
            Ok(data) if data.is_empty() => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
}

pub struct ATClient {
    pub config: Arc<Config>,
    pub conn: Arc<Mutex<Box<dyn ATConnection>>>,
    pub urc_tx: broadcast::Sender<String>,
}

impl ATClient {
    pub fn new(config: Arc<Config>) -> Result<Self, Box<dyn Error>> {
        let at_config = &config.at_config;

        let conn: Box<dyn ATConnection> = if at_config.conn_type == "NETWORK" {
            Box::new(NetworkATConn::new(config.clone()))
        } else if at_config.serial.method == "TOM_MODEM" {
            Box::new(TomModemATConn::new(config.clone()))
        } else {
            Box::new(SerialATConn::new(config.clone()))
        };

        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            config,
            conn: Arc::new(Mutex::new(conn)),
            urc_tx: tx,
        })
    }

    /// 发送 AT 命令并等待响应
    pub async fn send_command(
        &self,
        command: String,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut conn = self.conn.lock().await;
        let original_cmd = command.trim().to_string();
        let command = normalize_command(&command);

        if command.is_empty() {
            return Err("EMPTY COMMAND".into());
        }

        drain_stale_data(&mut conn).await;

        crate::log_debug!("[DEBUG] ==> TX: {:?}", command);
        conn.send(command.as_bytes()).await?;

        let mut raw_response = String::new();
        let start = Instant::now();

        while start.elapsed() < COMMAND_TIMEOUT {
            match conn.receive().await {
                Ok(data) if !data.is_empty() => {
                    raw_response.push_str(&String::from_utf8_lossy(&data));
                    if has_terminal_marker(&raw_response) {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
            sleep(DRAIN_INTERVAL).await;
        }

        let result = sanitize_response(&raw_response, &original_cmd);
        crate::log_debug!("[DEBUG] <== RX: {:?}", result);

        if let Some(err) = response_error(&raw_response) {
            return Err(err.into());
        }

        if result.is_empty() || raw_response.trim().is_empty() {
            return Err("TIMEOUT".into());
        }

        Ok(result)
    }

    /// 心跳探测：发送标准空指令 AT，期望 OK。
    ///
    /// 不要用 "ping" 探测：模组会把它当成未知命令，回 ERROR 或者干脆不回，
    /// 残留的响应会被下一条真正的命令读走，造成“命令随机失败”。
    pub async fn probe(&self) -> bool {
        let mut conn = self.conn.lock().await;
        if !conn.is_connected() {
            return false;
        }

        drain_stale_data(&mut conn).await;

        if conn.send(b"AT\r\n").await.is_err() {
            return false;
        }

        let mut raw = String::new();
        let start = Instant::now();
        while start.elapsed() < PROBE_TIMEOUT {
            match conn.receive().await {
                Ok(data) if !data.is_empty() => {
                    raw.push_str(&String::from_utf8_lossy(&data));
                    if has_terminal_marker(&raw) {
                        return !raw.trim().is_empty();
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
            sleep(DRAIN_INTERVAL).await;
        }

        crate::log_debug!("[AT] 心跳无响应，残留数据: {:?}", raw.trim());
        false
    }

    /// 初始化模块（ATE0, CNMI, CMGF, CLIP）
    pub async fn init_module(&self) {
        let _ = self.send_command("ATE0".into()).await;
        let _ = self.send_command("AT+CNMI=2,1,0,2,0".into()).await;
        let _ = self.send_command("AT+CMGF=0".into()).await;
        let _ = self.send_command("AT+CLIP=1".into()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ping_inside_text() {
        let resp = "+CMGL: 0,0,,20\r\nshipping soon\r\nOK";
        assert!(sanitize_response(resp, "AT+CMGL=4").contains("shipping soon"));
    }

    #[test]
    fn strips_echo_and_ping_echo_lines() {
        let resp = "AT+CMGF?\r\nping\r\n+CMGF: 0\r\nOK";
        assert_eq!(sanitize_response(resp, "AT+CMGF?"), "+CMGF: 0\nOK");
    }

    #[test]
    fn terminal_marker_must_be_last_line() {
        assert!(has_terminal_marker("+CMGF: 0\r\nOK\r\n"));
        assert!(!has_terminal_marker("+CMGL: 1,0,,10\r\nOK inside text\r\n"));
    }

    #[test]
    fn error_only_when_last_line() {
        assert!(response_error("+CMGF: 0\r\nOK").is_none());
        assert_eq!(response_error("+CMGF: 0\r\nERROR").as_deref(), Some("ERROR"));
    }
}
