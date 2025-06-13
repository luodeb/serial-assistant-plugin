use thiserror::Error;

/// 串口调试助手错误类型
#[derive(Error, Debug)]
pub enum SerialError {
    /// 串口连接错误
    #[error("串口连接失败: {0}")]
    ConnectionFailed(String),

    /// 串口配置错误
    #[error("串口配置无效: {0}")]
    InvalidConfiguration(String),

    /// 数据发送错误
    #[error("数据发送失败: {0}")]
    SendFailed(String),

    /// 端口枚举错误
    #[error("端口枚举失败: {0}")]
    PortEnumerationFailed(String),

    /// IO错误
    #[error("IO错误: {0}")]
    IoError(#[from] std::io::Error),

    /// 序列化错误
    #[error("序列化错误: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// 串口库错误
    #[error("串口库错误: {0}")]
    SerialPortError(#[from] serialport::Error),

    /// 任务取消错误
    #[error("任务被取消")]
    TaskCancelled,

    /// 插件错误
    #[error("插件错误: {0}")]
    PluginError(String),
}

/// 串口操作结果类型
pub type SerialResult<T> = Result<T, SerialError>;

/// 错误响应结构
#[derive(Debug, serde::Serialize)]
pub struct ErrorResponse {
    pub error_type: String,
    pub message: String,
    pub details: Option<String>,
    pub timestamp: String,
}

impl From<SerialError> for ErrorResponse {
    fn from(error: SerialError) -> Self {
        Self {
            error_type: match &error {
                SerialError::ConnectionFailed(_) => "connection_failed",
                SerialError::InvalidConfiguration(_) => "invalid_configuration",
                SerialError::SendFailed(_) => "send_failed",
                SerialError::PortEnumerationFailed(_) => "port_enumeration_failed",
                SerialError::IoError(_) => "io_error",
                SerialError::SerializationError(_) => "serialization_error",
                SerialError::SerialPortError(_) => "serial_port_error",
                SerialError::TaskCancelled => "task_cancelled",
                SerialError::PluginError(_) => "plugin_error",
            }
            .to_string(),
            message: error.to_string(),
            details: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// 创建连接失败错误
pub fn connection_failed(port: &str, reason: &str) -> SerialError {
    SerialError::ConnectionFailed(format!("端口 {} - {}", port, reason))
}

/// 创建数据发送错误
pub fn send_failed(data_length: usize, reason: &str) -> SerialError {
    SerialError::SendFailed(format!("发送 {} 字节数据失败: {}", data_length, reason))
}
