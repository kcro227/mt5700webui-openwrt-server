use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

mod config;
mod logger;
mod at;
mod airplane;
mod websocket;
mod net_utils;
mod netmon;
mod notify;

use config::{load_config_from_uci, Config, DEFAULT_CONFIG_JSON};
use at::ATClient;
use airplane::AutoAirPlaneMode;
use net_utils::{create_dual_stack_listener, create_ipv4_listener};

fn is_urc_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    // 心跳回显 / 自定义文本不是 URC
    if trimmed.eq_ignore_ascii_case("ping")
        || trimmed.eq_ignore_ascii_case("pong")
        || trimmed
            .as_bytes()
            .windows(4)
            .any(|window| window.eq_ignore_ascii_case(b"ping"))
    {
        return false;
    }

    let upper = trimmed.to_ascii_uppercase();
    trimmed.starts_with('^')
        || trimmed.starts_with('+')
        || upper.starts_with("RING")
        || upper.starts_with("NO CARRIER")
        || upper.starts_with("NO ANSWER")
        || upper.starts_with("BUSY")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 从UCI加载配置
    let config = match load_config_from_uci() {
        Ok(config) => config,
        Err(e) => {
            crate::log_error!("从UCI加载配置失败: {}, 使用默认配置", e);
            serde_json::from_str(DEFAULT_CONFIG_JSON)?
        }
    };

    let config = Arc::new(config);
    logger::init(
        config.logging.enabled,
        &config.logging.level,
    );
    notify::init(&config.notification_config.log_file);

    // 打印配置信息
    print_config_summary(&config);

    // 创建AT客户端
    // let at_client = Arc::new(ATClient::new(config.clone())?);
    let at_client = Arc::new(ATClient::new(config.clone())?);


    // 创建自动重启飞行模式监控
    let auto_flight_mode = AutoAirPlaneMode::new(at_client.clone());
    if auto_flight_mode.is_enbale() {
        auto_flight_mode.monitor_loop().await;
    }

    // 心跳任务：发送标准空指令 AT，而不是 "ping"。
    // 模组会把 ping 当作未知命令，回 ERROR 或者完全不回，残留响应会污染下一条命令。
    let c_heartbeat = at_client.clone();
    tokio::spawn(async move {
        let mut heartbeat_timer = interval(Duration::from_secs(30));
        let mut failures = 0u32;
        loop {
            heartbeat_timer.tick().await;
            if c_heartbeat.probe().await {
                failures = 0;
                continue;
            }

            failures += 1;
            if failures >= 3 {
                crate::log_warn!("[AT] 连续 {} 次心跳失败，标记连接断开并等待重连", failures);
                c_heartbeat.conn.lock().await.disconnect();
                failures = 0;
            }
        }
    });

    // URC 捕获任务
    let c_monitor = at_client.clone();
    tokio::spawn(async move {
        let mut last_reconnect_attempt = std::time::Instant::now();
        let mut was_connected = false;
        loop {
            let mut has_data = false;
            {
                let mut conn = c_monitor.conn.lock().await;
                if !conn.is_connected() {
                    if was_connected {
                        crate::log_warn!("[AT] 连接已断开，正在重连...");
                        was_connected = false;
                    }
                    if last_reconnect_attempt.elapsed() >= Duration::from_secs(2) {
                        last_reconnect_attempt = std::time::Instant::now();
                        if let Ok(_) = conn.connect().await {
                            crate::log_info!("[AT] 连接已恢复");
                            was_connected = true;
                            drop(conn);
                            let c_init = c_monitor.clone();
                            tokio::spawn(async move { c_init.init_module().await });
                        }
                    }
                } else {
                    match conn.receive().await {
                        Ok(data) => {
                            if !data.is_empty() {
                                has_data = true;
                                let text = String::from_utf8_lossy(&data).to_string();
                                for line in text.lines() {
                                    if is_urc_line(line) {
                                        crate::log_debug!("[URC DETECTED] <== {:?}", line);
                                        if let Some((kind, message)) = crate::notify::describe_urc(line) {
                                            crate::notify::event(kind, &message);
                                        }
                                        let _ = c_monitor.urc_tx.send(line.to_string());
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            if was_connected {
                                crate::log_warn!("[AT] 连接已断开，正在重连...");
                                was_connected = false;
                            }
                        }
                    }
                }
            }
            if !has_data {
                // Avoid a hot polling loop when the modem has no URC data.
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    });

    // 获取WebSocket配置
    let ws_v6_host = &config.websocket_config.ipv6.host;
    let ws_v6_port = config.websocket_config.ipv6.port;

    // 尝试绑定IPv6双栈监听器
    crate::log_info!("尝试绑定IPv6双栈监听器...");
    let ws_listener = match create_dual_stack_listener(&ws_v6_host, ws_v6_port).await {
        Ok(listener) => {
            crate::log_info!(
                "✓ 成功绑定IPv6双栈监听器: [{}]:{}",
                if ws_v6_host == "::" { "::" } else { &ws_v6_host },
                ws_v6_port
            );
            listener
        }
        Err(e) => {
            crate::log_warn!("无法绑定IPv6双栈监听器: {}, 尝试绑定IPv4...", e);
            // 回退到只绑定IPv4
            let ws_v4_addr = format!(
                "{}:{}",
                config.websocket_config.ipv4.host, config.websocket_config.ipv4.port
            );
            match create_ipv4_listener(&config.websocket_config.ipv4.host, config.websocket_config.ipv4.port).await {
                Ok(listener) => {
                    crate::log_info!("成功绑定IPv4监听器: {}", ws_v4_addr);
                    listener
                }
                Err(e) => {
                    crate::log_error!("无法绑定IPv4监听器 {}: {}", ws_v4_addr, e);
                    return Err(e.into());
                }
            }
        }
    };

    crate::log_info!("--------------------------------------");
    crate::log_info!("AT WebSocket 服务器启动成功！");
    crate::log_info!("监听端口: {}", ws_v6_port);
    crate::log_info!("支持协议: IPv4 和 IPv6 (双栈)");
    if !config.websocket_config.auth_key.is_empty() {
        crate::log_info!("认证模式: 已启用 (密钥长度: {})", config.websocket_config.auth_key.len());
    } else {
        crate::log_info!("认证模式: 未启用 (允许无密钥访问)");
    }
    crate::log_info!("--------------------------------------");

    // 网络变化监控（制式 / 频段 / 载波聚合 / 服务小区）
    if config.netmon.enabled {
        netmon::monitor_loop(at_client.clone(), config.netmon.interval).await;
    }

    // 启动WebSocket服务器
    crate::log_info!("WebSocket 服务器运行中...");
    loop {
        match ws_listener.accept().await {
            Ok((stream, addr)) => {
                let client = at_client.clone();
                let config = config.clone();
                tokio::spawn(async move {
                    let _ = websocket::handle_connection(stream, addr, client, config).await;
                });
            }
            Err(e) => {
                crate::log_error!("接受连接失败: {}", e);
                break;
            }
        }
    }

    Ok(())
}

/// 打印配置摘要
fn print_config_summary(config: &Config) {
    crate::log_info!("{}", "=".repeat(60));
    crate::log_info!("当前运行配置:");
    crate::log_info!("{}", "=".repeat(60));
    crate::log_info!("连接类型: {}", config.at_config.conn_type);

    if config.at_config.conn_type == "NETWORK" {
        crate::log_info!(
            "  网络地址: {}:{}",
            config.at_config.network.host, config.at_config.network.port
        );
        crate::log_info!("  网络超时: {}秒", config.at_config.network.timeout);
    } else {
        crate::log_info!("  串口设备: {}", config.at_config.serial.port);
        crate::log_info!("  波特率: {}", config.at_config.serial.baudrate);
        crate::log_info!("  串口超时: {}秒", config.at_config.serial.timeout);
        crate::log_info!("  串口方法: {}", config.at_config.serial.method);
        crate::log_info!("  串口功能: {}", config.at_config.serial.feature);
    }

    crate::log_info!("\nWebSocket 配置:");
    crate::log_info!("  监听端口: {}", config.websocket_config.ipv4.port);
    crate::log_info!("  IPv4 绑定: {}", config.websocket_config.ipv4.host);
    crate::log_info!("  IPv6 绑定: {}", config.websocket_config.ipv6.host);
    crate::log_info!(
        "  认证密钥: {}",
        if config.websocket_config.auth_key.is_empty() {
            "无"
        } else {
            "已设置"
        }
    );

    crate::log_info!("\n运行日志:");
    crate::log_info!(
        "  状态: {}",
        if config.logging.enabled {
            "启用"
        } else {
            "禁用"
        }
    );
    crate::log_info!("  级别: {}", config.logging.level);

    crate::log_info!("\n通知配置:");
    crate::log_info!(
        "  企业微信: {}",
        if config.notification_config.wechat_webhook.is_empty() {
            "未启用"
        } else {
            "已启用"
        }
    );
    crate::log_info!(
        "  日志文件: {}",
        if config.notification_config.log_file.is_empty() {
            "未启用"
        } else {
            &config.notification_config.log_file
        }
    );

    crate::log_info!("  通知类型:");
    crate::log_info!(
        "    - 短信通知: {}",
        if config.notification_config.notification_types.sms {
            "✓ 启用"
        } else {
            "✗ 禁用"
        }
    );
    crate::log_info!(
        "    - 来电通知: {}",
        if config.notification_config.notification_types.call {
            "✓ 启用"
        } else {
            "✗ 禁用"
        }
    );
    crate::log_info!(
        "    - 存储满通知: {}",
        if config.notification_config.notification_types.memory_full {
            "✓ 启用"
        } else {
            "✗ 禁用"
        }
    );
    crate::log_info!(
        "    - 信号通知: {}",
        if config.notification_config.notification_types.signal {
            "✓ 启用"
        } else {
            "✗ 禁用"
        }
    );

    crate::log_info!("\n自动重启飞行模式配置:");
    crate::log_info!(
        "  启用: {}",
        if config.auto_airplane.enabled { "是" } else { "否" }
    );
    crate::log_info!(
        "重启执行时间：{} ",
        if config.auto_airplane.action_time.is_empty() {
            "未设置".to_string()
        } else {
            config.auto_airplane.action_time.to_string()
        }
    );

    crate::log_info!("\n网络变化监控:");
    crate::log_info!(
        "  启用: {}",
        if config.netmon.enabled { "是" } else { "否" }
    );
    crate::log_info!("  轮询间隔: {} 秒", config.netmon.interval);

    crate::log_info!("{}", "=".repeat(60));
}