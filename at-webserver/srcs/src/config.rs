use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::process::Command;

// 默认配置常量
pub const DEFAULT_CONFIG_JSON: &str = r#"{
    "AT_CONFIG": {
        "TYPE": "NETWORK",
        "NETWORK": { "HOST": "192.168.8.1", "PORT": 20249, "TIMEOUT": 30 },
        "SERIAL": { 
            "PORT": "COM6", 
            "BAUDRATE": 115200, 
            "TIMEOUT": 30,
            "METHOD": "TOM_MODEM",
            "FEATURE": "UBUS"
        }
    },
    "WEBSOCKET_CONFIG": {
        "IPV4": { "HOST": "0.0.0.0", "PORT": 8765 },
        "IPV6": { "HOST": "::", "PORT": 8765 },
        "AUTH_KEY": ""
    },
    "NOTIFICATION_CONFIG": {
        "WECHAT_WEBHOOK": "",
        "LOG_FILE": "",
        "NOTIFICATION_TYPES": {
            "SMS": true,
            "CALL": true,
            "MEMORY_FULL": true,
            "SIGNAL": true
        }
    },
    "SCHEDULE_AIRPLANE_CONFIG": {
        "ENABLED": false,
        "ACTION_TIME": "8:00"
    }
}"#;

// ========== 配置结构体定义 ==========

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(rename = "AT_CONFIG")]
    pub at_config: AtConfig,
    #[serde(rename = "WEBSOCKET_CONFIG")]
    pub websocket_config: WsConfig,
    #[serde(rename = "NOTIFICATION_CONFIG")]
    pub notification_config: NotificationConfig,
    #[serde(rename = "SCHEDULE_AIRPLANE_CONFIG")]
    pub auto_airplane: AutoAirPlane,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AtConfig {
    #[serde(rename = "TYPE")]
    pub conn_type: String,
    #[serde(rename = "NETWORK")]
    pub network: NetworkConfig,
    #[serde(rename = "SERIAL")]
    pub serial: SerialConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkConfig {
    #[serde(rename = "HOST")]
    pub host: String,
    #[serde(rename = "PORT")]
    pub port: u16,
    #[serde(rename = "TIMEOUT")]
    pub timeout: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SerialConfig {
    #[serde(rename = "PORT")]
    pub port: String,
    #[serde(rename = "BAUDRATE")]
    pub baudrate: u32,
    #[serde(rename = "TIMEOUT")]
    pub timeout: u64,
    #[serde(rename = "METHOD")]
    pub method: String,
    #[serde(rename = "FEATURE")]
    pub feature: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WsConfig {
    #[serde(rename = "IPV4")]
    pub ipv4: WsEndpoint,
    #[serde(rename = "IPV6")]
    pub ipv6: WsEndpoint,
    #[serde(rename = "AUTH_KEY")]
    pub auth_key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WsEndpoint {
    #[serde(rename = "HOST")]
    pub host: String,
    #[serde(rename = "PORT")]
    pub port: u16,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotificationConfig {
    #[serde(rename = "WECHAT_WEBHOOK")]
    pub wechat_webhook: String,
    #[serde(rename = "LOG_FILE")]
    pub log_file: String,
    #[serde(rename = "NOTIFICATION_TYPES")]
    pub notification_types: NotificationTypes,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotificationTypes {
    #[serde(rename = "SMS")]
    pub sms: bool,
    #[serde(rename = "CALL")]
    pub call: bool,
    #[serde(rename = "MEMORY_FULL")]
    pub memory_full: bool,
    #[serde(rename = "SIGNAL")]
    pub signal: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AutoAirPlane {
    #[serde(rename = "ENABLED")]
    pub enabled: bool,
    #[serde(rename = "ACTION_TIME")]
    pub action_time: String,
}

// ========== 从 UCI 加载配置 ==========

pub fn load_config_from_uci() -> Result<Config, Box<dyn Error>> {
    println!("开始从 UCI 加载配置...");

    // 执行 uci 命令
    let output = Command::new("uci")
        .args(&["show", "at-webserver"])
        .output()?;

    if !output.status.success() {
        println!("读取 UCI 配置失败，使用默认配置");
        return serde_json::from_str(DEFAULT_CONFIG_JSON)
            .map_err(|e| format!("解析默认配置失败: {}", e).into());
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut uci_data = HashMap::new();

    // 解析 UCI 输出
    for line in output_str.trim().lines() {
        if line.contains('=') {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                let key = parts[0];
                let value = parts[1].trim_matches(|c| c == '\'' || c == '"');

                // 移除前缀 'at-webserver.config.'
                if key.starts_with("at-webserver.config.") {
                    let short_key = key.replace("at-webserver.config.", "");
                    uci_data.insert(short_key, value.to_string());
                }
            }
        }
    }

    // 从默认配置开始
    println!("使用默认配置初始化...");
    let mut config: Config = serde_json::from_str(DEFAULT_CONFIG_JSON)?;

    println!("开始从 UCI 加载配置...");
    // 读取连接类型
    let conn_type = uci_data
        .get("connection_type")
        .map(|s| s.as_str())
        .unwrap_or("NETWORK");
    config.at_config.conn_type = conn_type.to_string();
    println!("配置加载: 连接类型 = {}", conn_type);

    // 读取网络配置
    if conn_type == "NETWORK" {
        let host = uci_data
            .get("network_host")
            .map(|s| s.as_str())
            .unwrap_or("192.168.8.1");
        let port = uci_data
            .get("network_port")
            .map(|s| s.parse().unwrap_or(20249))
            .unwrap_or(20249);
        let timeout = uci_data
            .get("network_timeout")
            .map(|s| s.parse().unwrap_or(10))
            .unwrap_or(10);

        config.at_config.network.host = host.to_string();
        config.at_config.network.port = port;
        config.at_config.network.timeout = timeout;
        println!("配置加载: 网络连接 {}:{} (超时: {}秒)", host, port, timeout);
    } else {
        // 读取串口配置
        let mut port = uci_data
            .get("serial_port")
            .map(|s| s.as_str())
            .unwrap_or("/dev/ttyUSB0")
            .to_string();

        // 如果选择了自定义路径，读取自定义值
        if port == "custom" {
            port = uci_data
                .get("serial_port_custom")
                .map(|s| s.as_str())
                .unwrap_or("/dev/ttyUSB0")
                .to_string();
        }

        let baudrate = uci_data
            .get("serial_baudrate")
            .map(|s| s.parse().unwrap_or(115200))
            .unwrap_or(115200);
        let timeout = uci_data
            .get("serial_timeout")
            .map(|s| s.parse().unwrap_or(10))
            .unwrap_or(10);

        config.at_config.serial.port = port.clone();
        config.at_config.serial.baudrate = baudrate;
        config.at_config.serial.timeout = timeout;

        // 读取串口方法和功能
        let method = uci_data
            .get("serial_method")
            .map(|s| s.as_str())
            .unwrap_or("TOM_MODEM");
        let feature = uci_data
            .get("serial_feature")
            .map(|s| s.as_str())
            .unwrap_or("UBUS");

        config.at_config.serial.method = method.to_string();
        config.at_config.serial.feature = feature.to_string();

        println!(
            "配置加载: 串口连接 {} @ {} bps (超时: {}秒)",
            port, baudrate, timeout
        );
        println!("配置加载: 串口方法 = {}, 功能 = {}", method, feature);
    }

    // 读取 WebSocket 端口
    let ws_port = uci_data
        .get("websocket_port")
        .map(|s| s.parse().unwrap_or(8765))
        .unwrap_or(8765);
    config.websocket_config.ipv4.port = ws_port;
    config.websocket_config.ipv6.port = ws_port;

    // 读取是否允许外网访问（仅用于打印提示）
    let _allow_wan = uci_data
        .get("websocket_allow_wan")
        .map(|s| s == "1")
        .unwrap_or(false);

    // WebSocket 始终监听所有网卡
    config.websocket_config.ipv4.host = "0.0.0.0".to_string();
    config.websocket_config.ipv6.host = "::".to_string();

    // 读取连接密钥
    let auth_key = uci_data
        .get("websocket_auth_key")
        .map(|s| s.as_str())
        .unwrap_or("");
    config.websocket_config.auth_key = auth_key.to_string();

    // 读取通知配置
    if let Some(wechat_webhook) = uci_data.get("wechat_webhook") {
        config.notification_config.wechat_webhook = wechat_webhook.clone();
        println!("配置加载: 企业微信推送已启用");
    }

    if let Some(log_file) = uci_data.get("log_file") {
        config.notification_config.log_file = log_file.clone();
        println!("配置加载: 日志文件 = {}", log_file);
    }

    // 读取通知类型开关
    if let Some(notify_sms) = uci_data.get("notify_sms") {
        config.notification_config.notification_types.sms = notify_sms == "1";
    }
    if let Some(notify_call) = uci_data.get("notify_call") {
        config.notification_config.notification_types.call = notify_call == "1";
    }
    if let Some(notify_memory_full) = uci_data.get("notify_memory_full") {
        config.notification_config.notification_types.memory_full = notify_memory_full == "1";
    }
    if let Some(notify_signal) = uci_data.get("notify_signal") {
        config.notification_config.notification_types.signal = notify_signal == "1";
    }

    // 读取自动开关飞行模式
    if let Some(auto_airplane) = uci_data.get("schedule_auto_airplane_enable") {
        let enabled = auto_airplane == "1";
        let action_time = uci_data
            .get("schedule_airplane_time")
            .map(|s: &String| s.as_str())
            .unwrap_or("8:00")
            .to_string();

        println!(
            "配置加载: 自动开关飞行模式 = {} (时间: {})",
            if enabled { "启用" } else { "禁用" },
            action_time
        );

        config.auto_airplane.enabled = enabled;
        config.auto_airplane.action_time = action_time;
    }

    println!("✓ UCI 配置加载完成");
    Ok(config)
}