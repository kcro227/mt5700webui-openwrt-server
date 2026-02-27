use chrono::{Timelike, Utc};
use chrono_tz::Asia::Shanghai;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::at::ATClient;
use crate::config::Config;

/// 自动开关飞行模式功能
pub struct AutoAirPlaneMode {
    client: Arc<ATClient>,
    enabled: bool,
    action_time: String,
}

impl AutoAirPlaneMode {
    pub fn new(client: Arc<ATClient>, config: Arc<Config>) -> Self {
        let auto_airplane = &config.auto_airplane;

        let mode = Self {
            client,
            enabled: auto_airplane.enabled,
            action_time: auto_airplane.action_time.clone(),
        };

        if mode.enabled {
            println!("{}", "=".repeat(60));
            println!("自动开关飞行模式功能已启用");
            println!("  操作时间: {}", mode.action_time);
            println!("{}", "=".repeat(60));
        }

        mode
    }

    pub fn is_enbale(&self) -> bool {
        if self.enabled
        {
            return true;
        }
        else {
            false
        }
    }

    fn parse_action_time(&self) -> Result<(u32, u32), Box<dyn Error + Send + Sync>> {
        let parts: Vec<&str> = self.action_time.split(':').collect();
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
            println!(
                "[{}] 自动重启飞行模式开始...",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            );

            // 关闭飞行模式 (CFUN=0 开启飞行模式)
            match client.send_command("AT+CFUN=0".into()).await {
                Ok(_) => println!("飞行模式已开启"),
                Err(e) => println!("开启飞行模式失败: {}", e),
            }

            // 等待10秒
            sleep(Duration::from_secs(5)).await;

            // 打开飞行模式 (CFUN=1 关闭飞行模式)
            match client.send_command("AT+CFUN=1".into()).await {
                Ok(_) => println!("飞行模式已关闭"),
                Err(e) => println!("关闭飞行模式失败: {}", e),
            }

            println!(
                "[{}] 自动重启飞行模式完成",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            );
        });
    }

    /// 启动监控循环
    pub async fn monitor_loop(self) {
        tokio::spawn(async move {
            loop {
                if self.enabled {
                    let now = Utc::now().with_timezone(&Shanghai);
                    println!("当前时间: {}", now.format("%H:%M"));

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