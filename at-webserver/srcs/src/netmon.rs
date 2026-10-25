use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::at::ATClient;
use crate::notify;

/// 单个载波（PCC 或 SCC）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carrier {
    pub arfcn: u64,
    pub freq_khz: u64,
    pub bw_khz: u64,
}

/// 一次采样得到的网络状态。
#[derive(Debug, Clone, Default)]
pub struct NetworkState {
    /// 服务小区制式：NR / LTE / WCDMA / GSM，空字符串表示无服务
    pub rat: String,
    pub band: Option<u32>,
    pub carriers: Vec<Carrier>,
    /// 小区身份（MCC/MNC/TAC/CellID/PCI 等），不含信号强度
    pub cell_id: String,
    pub pci: Option<u32>,
    pub rsrp: Option<i32>,
    pub rsrq: Option<i32>,
    pub sinr: Option<i32>,
}

fn rat_label(rat: &str) -> String {
    match rat.trim().to_ascii_uppercase().as_str() {
        "NR" | "NR5G" | "NR-5GC" => "5G NR".to_string(),
        "LTE" | "LTE-A" | "LTE-A-PRO" => "4G LTE".to_string(),
        "WCDMA" | "HSPA" | "HSPA+" | "UMTS" => "3G WCDMA".to_string(),
        "GSM" | "GPRS" | "EDGE" => "2G GSM".to_string(),
        "" => "无服务".to_string(),
        other => other.to_string(),
    }
}

fn band_label(rat: &str, band: u32) -> String {
    match rat.trim().to_ascii_uppercase().as_str() {
        "NR" => format!("n{}", band),
        "LTE" => format!("B{}", band),
        _ => format!("band {}", band),
    }
}

fn band_text(state: &NetworkState) -> String {
    match state.band {
        Some(band) => band_label(&state.rat, band),
        None => "未知".to_string(),
    }
}

/// 解析 AT^MONSC，例如：
/// ^MONSC: NR,460,11,633984,1,81B0EA001,244,81B003,-74,-10,21
/// 字段：制式,MCC,MNC,TAC,?,CellID,PCI,gNBID,RSRP,RSRQ,SINR
/// 不同制式字段个数可能不同，因此只把“最后三个字段”当信号值，其余当小区身份，
/// 避免信号强度每 1dB 的浮动被误判成小区切换。
fn parse_monsc(response: &str) -> Option<NetworkState> {
    let line = response
        .lines()
        .find(|line| line.trim_start().to_ascii_uppercase().starts_with("^MONSC"))?;
    let payload = line.split_once(':')?.1.trim();
    let fields: Vec<&str> = payload.split(',').map(|field| field.trim()).collect();
    if fields.len() < 4 {
        return None;
    }

    let rat = fields[0].to_string();
    if rat.eq_ignore_ascii_case("NOSERVICE") {
        return Some(NetworkState::default());
    }

    let signal_start = fields.len().saturating_sub(3).max(1);
    let cell_id = fields[1..signal_start].join(",");

    let number = |index: usize| -> Option<i32> {
        fields.get(index).and_then(|value| value.parse::<i32>().ok())
    };

    // NR/LTE 的 MONSC 第 7 个字段是 PCI
    let pci = fields.get(6).and_then(|value| value.parse::<u32>().ok());

    Some(NetworkState {
        rat,
        band: None,
        carriers: Vec::new(),
        cell_id,
        pci,
        rsrp: number(signal_start),
        rsrq: number(signal_start + 1),
        sinr: number(signal_start + 2),
    })
}

/// 解析 AT^HFREQINFO?，例如：
/// ^HFREQINFO: 1,7,78,636664,3549960,100000,636664,3549960,100000
/// 结构：<模式>,<保留>,<频段>,然后每个载波三个值：<ARFCN>,<频率kHz>,<带宽kHz>
fn parse_freq_info(response: &str) -> Option<(Option<u32>, Vec<Carrier>)> {
    let line = response
        .lines()
        .find(|line| line.trim_start().to_ascii_uppercase().starts_with("^HFREQINFO"))?;
    let payload = line.split_once(':')?.1.trim();

    let numbers: Vec<u64> = payload
        .split(',')
        .filter_map(|value| value.trim().parse::<u64>().ok())
        .collect();

    if numbers.len() < 6 {
        return None;
    }

    let band = if (1..=1024).contains(&numbers[2]) {
        Some(numbers[2] as u32)
    } else {
        None
    };

    let carriers: Vec<Carrier> = numbers[3..]
        .chunks_exact(3)
        .map(|chunk| Carrier {
            arfcn: chunk[0],
            freq_khz: chunk[1],
            bw_khz: chunk[2],
        })
        .collect();

    if carriers.is_empty() {
        None
    } else {
        Some((band, carriers))
    }
}

/// 人类可读的当前网络描述。
pub fn describe(state: &NetworkState) -> String {
    let mut parts = vec![rat_label(&state.rat)];

    if let Some(band) = state.band {
        parts.push(format!("频段 {}", band_label(&state.rat, band)));
    }

    if !state.carriers.is_empty() {
        parts.push(format!("{}CC", state.carriers.len()));
    }

    if let Some(pci) = state.pci {
        parts.push(format!("PCI {}", pci));
    }

    if let Some(rsrp) = state.rsrp {
        parts.push(format!("RSRP {}dBm", rsrp));
    }

    if let Some(sinr) = state.sinr {
        parts.push(format!("SINR {}dB", sinr));
    }

    parts.join(" ")
}

/// 比较两次采样，返回变化描述。
fn diff(previous: &NetworkState, current: &NetworkState) -> Vec<String> {
    let mut changes = Vec::new();

    if !previous.rat.eq_ignore_ascii_case(&current.rat) {
        changes.push(format!(
            "制式 {} → {}",
            rat_label(&previous.rat),
            rat_label(&current.rat)
        ));
    }

    if previous.band != current.band {
        changes.push(format!(
            "频段 {} → {}",
            band_text(previous),
            band_text(current)
        ));
    }

    let previous_arfcn: Vec<u64> = previous.carriers.iter().map(|carrier| carrier.arfcn).collect();
    let current_arfcn: Vec<u64> = current.carriers.iter().map(|carrier| carrier.arfcn).collect();

    if previous.carriers.len() != current.carriers.len() {
        changes.push(format!(
            "载波聚合 {}CC → {}CC",
            previous.carriers.len(),
            current.carriers.len()
        ));
    } else if previous_arfcn != current_arfcn {
        changes.push("载波频点变化".to_string());
    }

    if previous.cell_id != current.cell_id {
        match (previous.pci, current.pci) {
            (Some(old), Some(new)) if old != new => {
                changes.push(format!("服务小区 PCI {} → {}", old, new));
            }
            _ => changes.push("服务小区变化".to_string()),
        }
    }

    changes
}

/// 供 LuCI 页面读取的当前网络状态（tmpfs，重启即失效）。
const STATE_FILE: &str = "/tmp/at-netmon.json";

fn write_state_file(state: &NetworkState) {
    let now = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);
    let payload = serde_json::json!({
        "timestamp": now.timestamp(),
        "time": now.format("%Y-%m-%d %H:%M:%S").to_string(),
        "rat": state.rat,
        "rat_label": rat_label(&state.rat),
        "band": state.band,
        "band_label": state.band.map(|band| band_label(&state.rat, band)),
        "carriers": state.carriers.len(),
        "pci": state.pci,
        "rsrp": state.rsrp,
        "rsrq": state.rsrq,
        "sinr": state.sinr,
        "desc": describe(state),
    });

    if let Ok(text) = serde_json::to_string(&payload) {
        if let Err(err) = std::fs::write(STATE_FILE, text) {
            crate::log_debug!("写入网络状态文件失败 ({}): {}", STATE_FILE, err);
        }
    }
}

/// 采样一次网络状态。
async fn sample(client: &ATClient) -> Option<NetworkState> {
    if let Ok(response) = client.send_command("AT^MONSC".to_string()).await {
        if let Some(mut state) = parse_monsc(&response) {
            if let Ok(freq) = client.send_command("AT^HFREQINFO?".to_string()).await {
                if let Some((band, carriers)) = parse_freq_info(&freq) {
                    if band.is_some() {
                        state.band = band;
                    }
                    if !carriers.is_empty() {
                        state.carriers = carriers;
                    }
                }
            }
            return Some(state);
        }
    }

    // 无服务时 MONSC 直接 ERROR，用 SYSINFOEX 兜底判断“真的没网”还是“命令失败”。
    if let Ok(info) = client.send_command("AT^SYSINFOEX".to_string()).await {
        if info.to_ascii_uppercase().contains("NOSERVICE") {
            return Some(NetworkState::default());
        }
    }

    None
}

/// 启动网络变化监控循环。
pub async fn monitor_loop(client: Arc<ATClient>, interval_secs: u64) {
    let seconds = interval_secs.clamp(10, 3600);
    crate::log_info!("网络变化监控已启动: 每 {} 秒采样一次", seconds);

    tokio::spawn(async move {
        // 等 AT 连接建立后再开始采样，否则第一次必然失败。
        sleep(Duration::from_secs(2)).await;

        let mut previous: Option<NetworkState> = None;

        loop {

            match sample(&client).await {
                Some(current) => {
                    match &previous {
                        None => notify::event("网络", &format!("当前网络: {}", describe(&current))),
                        Some(previous) => {
                            let changes = diff(previous, &current);
                            if changes.is_empty() {
                                crate::log_debug!("网络无变化: {}", describe(&current));
                            } else {
                                notify::event(
                                    "网络",
                                    &format!("{}；当前: {}", changes.join("；"), describe(&current)),
                                );
                            }
                        }
                    }
                    write_state_file(&current);
                    previous = Some(current);
                    sleep(Duration::from_secs(seconds)).await;
                }
                None => {
                    crate::log_debug!("读取网络信息失败，5 秒后重试");
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nr_monsc() {
        let state = parse_monsc(
            "^MONSC: NR,460,11,633984,1,81B0EA001,244,81B003,-74,-10,21\r\nOK",
        )
        .expect("should parse");

        assert_eq!(state.rat, "NR");
        assert_eq!(state.pci, Some(244));
        assert_eq!(state.rsrp, Some(-74));
        assert_eq!(state.rsrq, Some(-10));
        assert_eq!(state.sinr, Some(21));
        assert!(state.cell_id.contains("81B0EA001"));
        assert!(!state.cell_id.contains("-74"));
    }

    #[test]
    fn parses_two_carriers() {
        let (band, carriers) = parse_freq_info(
            "^HFREQINFO: 1,7,78,636664,3549960,100000,636664,3549960,100000\r\nOK",
        )
        .expect("should parse");

        assert_eq!(band, Some(78));
        assert_eq!(carriers.len(), 2);
        assert_eq!(carriers[0].arfcn, 636664);
        assert_eq!(carriers[0].bw_khz, 100000);
    }

    #[test]
    fn detects_rat_band_and_carrier_changes() {
        let previous = NetworkState {
            rat: "NR".into(),
            band: Some(78),
            carriers: vec![Carrier::default(); 2],
            cell_id: "a".into(),
            pci: Some(244),
            ..Default::default()
        };
        let current = NetworkState {
            rat: "LTE".into(),
            band: Some(3),
            carriers: vec![Carrier::default(); 1],
            cell_id: "b".into(),
            pci: Some(120),
            ..Default::default()
        };

        let changes = diff(&previous, &current);
        assert!(changes.iter().any(|item| item.contains("5G NR → 4G LTE")));
        assert!(changes.iter().any(|item| item.contains("n78 → B3")));
        assert!(changes.iter().any(|item| item.contains("2CC → 1CC")));
        assert!(changes.iter().any(|item| item.contains("PCI 244 → 120")));
    }

    #[test]
    fn signal_only_change_is_not_a_cell_change() {
        let mut previous = parse_monsc("^MONSC: NR,460,11,633984,1,81B0EA001,244,81B003,-74,-10,21").unwrap();
        let current = parse_monsc("^MONSC: NR,460,11,633984,1,81B0EA001,244,81B003,-80,-12,15").unwrap();

        assert!(diff(&previous, &current).is_empty());
        previous.rsrp = Some(-80);
        assert!(diff(&previous, &current).is_empty());
    }
}
