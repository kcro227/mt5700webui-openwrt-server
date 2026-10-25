# AT WebServer API 文档

## WebSocket 配置信息 API

### 端点
```
GET /cgi-bin/at-ws-info
```

### 描述
获取当前 AT WebServer 的 WebSocket 配置信息，用于前端动态获取 WebSocket 连接地址。

### 请求示例
```bash
curl http://192.168.1.1/cgi-bin/at-ws-info
```

### 响应格式
```json
{
  "success": true,
  "data": {
    "host": "192.168.1.1",
    "port": 8765,
    "allow_wan": 0,
    "ws_url": "ws://192.168.1.1:8765",
    "timestamp": 1729756800
  }
}
```

### 响应字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| success | boolean | 请求是否成功 |
| data.host | string | WebSocket 主机地址 |
| data.port | number | WebSocket 端口号 |
| data.allow_wan | number | 是否允许外网访问 (0=否, 1=是) |
| data.ws_url | string | 完整的 WebSocket URL |
| data.timestamp | number | Unix 时间戳 |

### 前端使用示例

#### JavaScript (原生)
```javascript
async function getWebSocketInfo() {
    const response = await fetch('/cgi-bin/at-ws-info');
    const result = await response.json();
    
    if (result.success) {
        const ws = new WebSocket(result.data.ws_url);
        ws.onopen = () => console.log('WebSocket 已连接');
    }
}

getWebSocketInfo();
```

#### JavaScript (jQuery)
```javascript
$.getJSON('/cgi-bin/at-ws-info', function(result) {
    if (result.success) {
        var ws = new WebSocket(result.data.ws_url);
        ws.onopen = function() {
            console.log('WebSocket 已连接');
        };
    }
});
```

#### Vue.js
```javascript
export default {
    data() {
        return {
            wsInfo: null,
            ws: null
        }
    },
    async mounted() {
        const response = await fetch('/cgi-bin/at-ws-info');
        const result = await response.json();
        
        if (result.success) {
            this.wsInfo = result.data;
            this.ws = new WebSocket(this.wsInfo.ws_url);
        }
    }
}
```

#### React
```javascript
import { useState, useEffect } from 'react';

function App() {
    const [wsInfo, setWsInfo] = useState(null);

    useEffect(() => {
        fetch('/cgi-bin/at-ws-info')
            .then(res => res.json())
            .then(result => {
                if (result.success) {
                    setWsInfo(result.data);
                    const ws = new WebSocket(result.data.ws_url);
                    ws.onopen = () => console.log('WebSocket 已连接');
                }
            });
    }, []);

    return (
        <div>
            {wsInfo && <p>WebSocket URL: {wsInfo.ws_url}</p>}
        </div>
    );
}
```

### 注意事项

1. **CORS 支持**：API 已启用 CORS，允许跨域访问
2. **动态主机**：当允许外网访问时，API 会自动返回当前访问的主机名
3. **缓存**：建议在每次需要连接时重新获取配置，而不是缓存
4. **错误处理**：请务必处理 API 请求失败的情况

### 错误处理示例
```javascript
async function getWebSocketInfo() {
    try {
        const response = await fetch('/cgi-bin/at-ws-info');
        
        if (!response.ok) {
            throw new Error(`HTTP error! status: ${response.status}`);
        }
        
        const result = await response.json();
        
        if (!result.success) {
            throw new Error('获取配置失败');
        }
        
        return result.data;
    } catch (error) {
        console.error('获取 WebSocket 配置失败:', error);
        // 使用默认配置作为后备
        return {
            host: window.location.hostname,
            port: 8765,
            ws_url: `ws://${window.location.hostname}:8765`
        };
    }
}
```

### 安全建议

1. 在生产环境中，建议限制此 API 的访问权限
2. 如果不需要外网访问，请在配置中禁用 `allow_wan`
3. 定期检查服务日志，监控异常访问

## WebSocket 协议

连接地址：`ws://<host>:<port>`（端口即 UCI 的 `websocket_port`，默认 8765）。
若配置了 `websocket_auth_key`，连接后需先发送：

```json
{"auth_key": "你的密钥"}
```

服务端返回 `{"success":true,"message":"认证成功"}` 后才可发送 AT 命令。

### 心跳

客户端每 20 秒发送纯文本 `ping`，服务端直接回纯文本 `pong`（不会把 ping 转发给模组）。
请勿在等待 AT 响应时发送其它自定义文本，否则会被当成 AT 命令。

### AT 命令

发送纯文本 AT 命令（无需 `\r`，服务端会补 `\r\n`）：

```
AT+CMGF?
```

响应：

```json
{"success": true, "data": "+CMGF: 0\r\nOK", "error": null}
```

失败时：`{"success": false, "data": null, "error": "ERROR"}`。

### URC 主动上报

模组主动上报（`+CMTI`、`^...` 等）会以如下格式推送给所有已连接客户端：

```json
{"type": "raw_data", "data": "+CMTI: \"SM\",3"}
```

### 网络变化日志

服务端按 `netmon_interval`（默认 30 秒）轮询 `AT^MONSC` 与 `AT^HFREQINFO?`，
制式（5G NR / 4G LTE）、频段（n78 / B3）、载波聚合数量、服务小区（PCI）发生变化时，
写入 `log_file`（LuCI「通知日志」页面可直接查看），格式：

```
[2026-10-24 22:40:25] [网络] 制式 5G NR → 4G LTE；频段 n78 → B3；当前: 4G LTE 频段 B3 1CC PCI 120 RSRP -95dBm SINR 8dB
```

信号强度（RSRP/RSRQ/SINR）的小幅波动不会触发日志。

### 通知日志

服务端会把以下事件写入 `log_file`（LuCI「通知日志」页面可筛选查看）：

- 网络变化：制式 / 频段 / 载波聚合 / 服务小区（PCI）
- 短信：`+CMTI`、`+CMT`、`+CDS`
- 来电：`+CLIP`、`NO CARRIER`、`BUSY`、`NO ANSWER`

同时每次采样会把当前网络写入 `/tmp/at-netmon.json`，供 LuCI 配置页显示。
