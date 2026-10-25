use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{OnceLock, RwLock};

use chrono::Utc;
use chrono_tz::Asia::Shanghai;

/// 通知日志文件路径（来自 UCI 的 log_file）。为空时只写 syslog。
static LOG_PATH: OnceLock<RwLock<String>> = OnceLock::new();

fn path_store() -> &'static RwLock<String> {
    LOG_PATH.get_or_init(|| RwLock::new(String::new()))
}

pub fn init(path: &str) {
    if let Ok(mut guard) = path_store().write() {
        *guard = path.trim().to_string();
    }
}

fn file_path() -> String {
    path_store()
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// 记录一条通知事件：同时写 syslog（stdout 由 procd 收集）和通知日志文件，
/// 后者对应 LuCI 的“通知日志”页面。
pub fn event(kind: &str, message: &str) {
    let now = Utc::now().with_timezone(&Shanghai);
    let timestamp = now.format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] [{}] {}", timestamp, kind, message);

    crate::log_info!("{}", line);

    let path = file_path();
    if path.is_empty() {
        return;
    }

    rotate_if_needed(&path);

    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => {
            if let Err(err) = writeln!(file, "{}", line) {
                crate::log_warn!("写入通知日志失败 ({}): {}", path, err);
            }
        }
        Err(err) => {
            crate::log_warn!("打开通知日志失败 ({}): {}", path, err);
        }
    }
}

/// 日志超过 1MB 时轮转为 .1，避免 /var/log 被写满。
fn rotate_if_needed(path: &str) {
    const MAX_SIZE: u64 = 1024 * 1024;

    let too_big = std::fs::metadata(path)
        .map(|meta| meta.len() > MAX_SIZE)
        .unwrap_or(false);

    if too_big {
        let backup = format!("{}.1", path);
        let _ = std::fs::rename(path, backup);
    }
}

/// 把常见的 URC 翻译成通知日志条目。
///
/// 只处理“能明确说明发生了什么”的上报，其它 URC 仍然只通过 WebSocket 转发，
/// 避免把信号强度之类的周期性上报刷进日志。
pub fn describe_urc(line: &str) -> Option<(&'static str, String)> {
    let trimmed = line.trim();
    let upper = trimmed.to_ascii_uppercase();

    if upper.starts_with("+CMTI:") {
        // +CMTI: "SM",3
        let storage = trimmed.split('"').nth(1).unwrap_or("SM");
        let index = trimmed.rsplit(',').next().unwrap_or("?").trim();
        return Some(("短信", format!("收到新短信（存储 {}，索引 {}）", storage, index)));
    }

    if upper.starts_with("+CMT:") || upper.starts_with("+CDS:") {
        return Some(("短信", "收到新短信".to_string()));
    }

    if upper.starts_with("+CLIP:") {
        // +CLIP: "13800138000",129,"",0
        let number = trimmed.split('"').nth(1).unwrap_or("未知号码");
        return Some(("来电", format!("来电：{}", number)));
    }

    if upper.starts_with("NO CARRIER") || upper.starts_with("NO ANSWER") || upper.starts_with("BUSY") {
        return Some(("来电", "通话结束".to_string()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cmti() {
        let (kind, message) = describe_urc("+CMTI: \"SM\",3").unwrap();
        assert_eq!(kind, "短信");
        assert!(message.contains("SM"));
        assert!(message.contains('3'));
    }

    #[test]
    fn parses_clip() {
        let (kind, message) = describe_urc("+CLIP: \"13800138000\",129,\"\",0").unwrap();
        assert_eq!(kind, "来电");
        assert!(message.contains("13800138000"));
    }

    #[test]
    fn ignores_signal_urc() {
        assert!(describe_urc("^HCSQ: \"NR\",68,201,30").is_none());
        assert!(describe_urc("+CREG: 2,1").is_none());
    }
}
