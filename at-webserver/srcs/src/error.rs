use std::error::Error;
use std::fmt;

/// 简单的字符串错误类型，用于快速包装错误消息
#[derive(Debug)]
pub struct StringError(pub String);

impl fmt::Display for StringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for StringError {}