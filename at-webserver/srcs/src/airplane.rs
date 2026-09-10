use chrono::{Timelike, Utc};
use chrono_tz::Asia::Shanghai;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::at::ATClient;

/// 自动开关飞行模式功能
pub struct AutoAirPlaneMode {
    client: Arc<ATClient>,
}

impl AutoAirPlaneMode {
    pub fn new(client: Arc<ATClient>) -> Self {
        if client.config.auto_airplane.enabled {
            crate::log_info!("{}", "=".repeat(60));
            crate::log_info!("自动开关飞行模式功能已启用");
            crate::log_info!("  操作时间: {}", client.config.auto_airplane.action_time);
            crate::log_info!("{}", "=".repeat(60));
        }

        Self { client }
    }

    pub fn is_enbale(&self) -> bool {
        self.client.config.auto_airplane.enabled
    }

    fn parse_action_time(&self) -> Result<(u32, u32), Box<dyn Error + Send + Sync>> {
        let parts: Vec<&str> = self
            .client
            .config
            .auto_airplane
            .action_time
            .split(':')
            .collect();
        if parts.len() != 2 {
            return Err("无效的时间格式，需为 HH:MM".into());
        }

        let hour: u32 = parts[0].parse().map_err(|_| "无效的小时值")?;
        let minute: u32 = parts[1].parse().map_err(|_| "无效的分钟值")?;

        if hour >= 24 || minute >= 60 {
            return Err("小时必须在0-23之间，分钟必须在0-59之间".into());
        }

        Ok((hour, minute))
    }

    fn is_action_time(&self, now: &chrono::DateTime<chrono_tz::Tz>) -> bool {
        if let Ok((action_hour, action_minute)) = self.parse_action_time() {
            return now.hour() == action_hour && now.minute() == action_minute;
        }
        false
    }

    fn restart_airplane_mode(&self) {
        let client = self.client.clone();
        tokio::spawn(async move {
            crate::log_info!(
                "[{}] 自动重启飞行模式开始...",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            );

            // 关闭飞行模式 (CFUN=0 开启飞行模式)
            match client.send_command("AT+CFUN=0".into()).await {
                Ok(_) => crate::log_info!("飞行模式已开启"),
                Err(e) => crate::log_error!("开启飞行模式失败: {}", e),
            }

            // 等待10秒
            sleep(Duration::from_secs(15)).await;

            // 打开飞行模式 (CFUN=1 关闭飞行模式)
            match client.send_command("AT+CFUN=1".into()).await {
                Ok(_) => crate::log_info!("飞行模式已关闭"),
                Err(e) => crate::log_error!("关闭飞行模式失败: {}", e),
            }

            crate::log_info!(
                "[{}] 自动重启飞行模式完成",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            );
        });
    }

    /// 启动监控循环
    pub async fn monitor_loop(self) {
        tokio::spawn(async move {
            loop {
                if self.is_enbale() {
                    let now = Utc::now().with_timezone(&Shanghai);
                    crate::log_debug!("当前时间: {}", now.format("%H:%M"));

                    if self.is_action_time(&now) {
                        self.restart_airplane_mode();
                        // 等待60秒，避免在同一分钟内重复触发
                        sleep(Duration::from_secs(60)).await;
                    }
                }
                // 每分钟查询一次
                sleep(Duration::from_secs(60)).await;
            }
        });
    }
}