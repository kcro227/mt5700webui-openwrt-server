use std::error::Error;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use tokio::net::{TcpListener, TcpSocket};

/// 创建双栈监听器，同时支持IPv4和IPv6
pub async fn create_dual_stack_listener(host: &str, port: u16) -> Result<TcpListener, Box<dyn Error>> {
    // 解析IPv6地址
    let ipv6_addr = if host == "::" {
        Ipv6Addr::UNSPECIFIED
    } else {
        Ipv6Addr::from_str(host).map_err(|e| format!("无效的IPv6地址: {}", e))?
    };

    let socket_addr = SocketAddr::new(IpAddr::V6(ipv6_addr), port);

    // 创建IPv6套接字
    let socket = TcpSocket::new_v6()?;
    // 服务重启时允许立即重新绑定同一地址，避免短暂的内核连接状态导致
    // “地址已被使用”，即使系统中已经没有监听进程。
    socket.set_reuseaddr(true)?;

    // 设置套接字选项：允许IPv4映射（在Linux上默认启用）
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = socket.as_raw_fd();

        // 设置IPV6_V6ONLY为0，允许IPv4映射
        let enable: libc::c_int = 0;
        let ret = unsafe {
            libc::setsockopt(
                fd,
                libc::IPPROTO_IPV6,
                libc::IPV6_V6ONLY,
                &enable as *const _ as *const libc::c_void,
                std::mem::size_of_val(&enable) as libc::socklen_t,
            )
        };
        if ret != 0 {
            return Err(format!("设置IPV6_V6ONLY失败: {}", std::io::Error::last_os_error()).into());
        }
    }

    // 绑定地址
    socket.bind(socket_addr)?;

    // 开始监听
    let listener = socket.listen(1024)?;

    Ok(listener)
}

/// 创建可复用的IPv4监听器，用于IPv6不可用时的回退。
pub async fn create_ipv4_listener(host: &str, port: u16) -> Result<TcpListener, Box<dyn Error>> {
    let ipv4_addr = host
        .parse()
        .map_err(|e| format!("无效的IPv4地址: {}", e))?;
    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(SocketAddr::new(IpAddr::V4(ipv4_addr), port))?;
    Ok(socket.listen(1024)?)
}
