use crate::config::{DataFormat, FlowControl, Parity, SerialConfig};
use crate::error::{connection_failed, send_failed, SerialError, SerialResult};

use plugin_interfaces::{log_debug, log_info};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, RwLock};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

/// 串口连接状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionStatus {
    /// 未连接
    Disconnected,
    /// 连接中
    Connecting,
    /// 已连接
    Connected,
    /// 连接错误
    Error(String),
}

/// 串口端口信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortInfo {
    pub port_name: String,
    pub port_type: String,
    pub description: Option<String>,
    pub hardware_id: Option<String>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
}

/// 数据包
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPacket {
    pub timestamp: String,
    pub direction: DataDirection,
    pub data: Vec<u8>,
    pub format: DataFormat,
    pub display_text: String,
}

/// 数据传输方向
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataDirection {
    /// 发送的数据
    Sent,
    /// 接收的数据
    Received,
}

/// 统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statistics {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub errors_count: u64,
    pub connection_time: Option<Duration>,
    #[serde(skip)]
    pub last_activity: Option<Instant>,
}

/// 串口客户端
pub struct SerialClient {
    /// 当前配置
    config: RwLock<SerialConfig>,
    /// 连接状态
    status: RwLock<ConnectionStatus>,
    /// 串口流
    stream: RwLock<Option<SerialStream>>,
    /// 统计信息
    statistics: RwLock<Statistics>,
    /// 数据接收通道
    data_receiver: RwLock<Option<mpsc::Receiver<DataPacket>>>,
    /// 数据发送通道
    data_sender: RwLock<Option<mpsc::Sender<DataPacket>>>,
    /// 连接开始时间
    connection_start: RwLock<Option<Instant>>,
}

impl SerialClient {
    /// 创建新的串口客户端
    pub fn new() -> Self {
        Self {
            config: RwLock::new(SerialConfig::default()),
            status: RwLock::new(ConnectionStatus::Disconnected),
            stream: RwLock::new(None),
            statistics: RwLock::new(Statistics::new()),
            data_receiver: RwLock::new(None),
            data_sender: RwLock::new(None),
            connection_start: RwLock::new(None),
        }
    }

    /// 获取可用端口列表
    pub async fn get_available_ports() -> SerialResult<Vec<PortInfo>> {
        let ports = tokio::task::spawn_blocking(serialport::available_ports)
            .await
            .map_err(|_e| SerialError::TaskCancelled)?
            .map_err(|e| SerialError::PortEnumerationFailed(e.to_string()))?;

        let port_infos = ports
            .into_iter()
            .map(|port| PortInfo {
                port_name: port.port_name.clone(),
                port_type: format!("{:?}", port.port_type),
                description: match &port.port_type {
                    serialport::SerialPortType::UsbPort(usb) => usb.product.clone(),
                    _ => None,
                },
                hardware_id: match &port.port_type {
                    serialport::SerialPortType::UsbPort(usb) => usb.serial_number.clone(),
                    _ => None,
                },
                vendor_id: match &port.port_type {
                    serialport::SerialPortType::UsbPort(usb) => Some(usb.vid),
                    _ => None,
                },
                product_id: match &port.port_type {
                    serialport::SerialPortType::UsbPort(usb) => Some(usb.pid),
                    _ => None,
                },
            })
            .collect();

        Ok(port_infos)
    }

    /// 连接到串口
    pub async fn connect(&self, config: SerialConfig) -> SerialResult<()> {
        // 更新状态为连接中
        *self.status.write().await = ConnectionStatus::Connecting;

        // 验证配置
        self.validate_config(&config).await?;

        // 尝试建立连接
        let stream = self.create_serial_stream(&config).await?;

        // 保存配置和连接
        *self.config.write().await = config;
        *self.stream.write().await = Some(stream);
        *self.status.write().await = ConnectionStatus::Connected;
        *self.connection_start.write().await = Some(Instant::now());

        // 启动数据接收任务
        self.start_data_receiver().await?;

        log_info!("串口连接成功: {}", self.config.read().await.port);
        Ok(())
    }

    /// 断开连接
    pub async fn disconnect(&self) -> SerialResult<()> {
        // 清理连接
        *self.stream.write().await = None;
        *self.status.write().await = ConnectionStatus::Disconnected;
        *self.connection_start.write().await = None;

        // 关闭数据通道
        *self.data_receiver.write().await = None;
        *self.data_sender.write().await = None;

        log_info!("串口连接已断开");
        Ok(())
    }

    /// 发送数据
    pub async fn send_data(&self, data: &[u8], format: DataFormat) -> SerialResult<()> {
        let mut stream_guard = self.stream.write().await;
        let stream = stream_guard
            .as_mut()
            .ok_or_else(|| SerialError::ConnectionFailed("未建立连接".to_string()))?;

        // 发送数据
        stream
            .write_all(data)
            .await
            .map_err(|e| send_failed(data.len(), &e.to_string()))?;

        // 更新统计信息
        let mut stats = self.statistics.write().await;
        stats.bytes_sent += data.len() as u64;
        stats.packets_sent += 1;
        stats.last_activity = Some(Instant::now());

        // 创建数据包记录
        let packet = DataPacket {
            timestamp: chrono::Utc::now().to_rfc3339(),
            direction: DataDirection::Sent,
            data: data.to_vec(),
            format: format.clone(),
            display_text: self.format_data_for_display(data, &format),
        };

        // 发送到数据通道
        if let Some(sender) = self.data_sender.read().await.as_ref() {
            let _ = sender.send(packet).await;
        }

        log_debug!("发送数据: {} 字节", data.len());
        Ok(())
    }

    /// 验证配置
    async fn validate_config(&self, config: &SerialConfig) -> SerialResult<()> {
        if config.port.is_empty() {
            return Err(SerialError::InvalidConfiguration(
                "端口名称不能为空".to_string(),
            ));
        }

        if config.baud_rate == 0 {
            return Err(SerialError::InvalidConfiguration(
                "波特率必须大于0".to_string(),
            ));
        }

        if ![5, 6, 7, 8].contains(&config.data_bits) {
            return Err(SerialError::InvalidConfiguration(
                "数据位必须为5-8".to_string(),
            ));
        }

        if ![1, 2].contains(&config.stop_bits) {
            return Err(SerialError::InvalidConfiguration(
                "停止位必须为1或2".to_string(),
            ));
        }

        Ok(())
    }

    /// 创建串口流
    async fn create_serial_stream(&self, config: &SerialConfig) -> SerialResult<SerialStream> {
        let port_name = config.port.clone();
        let baud_rate = config.baud_rate;
        let data_bits = convert_data_bits(config.data_bits)?;
        let stop_bits = convert_stop_bits(config.stop_bits)?;
        let parity = convert_parity(&config.parity)?;
        let flow_control = convert_flow_control(&config.flow_control)?;

        tokio::task::spawn_blocking(move || {
            tokio_serial::new(port_name, baud_rate)
                .data_bits(data_bits)
                .stop_bits(stop_bits)
                .parity(parity)
                .flow_control(flow_control)
                .open_native_async()
        })
        .await
        .map_err(|_| SerialError::TaskCancelled)?
        .map_err(|e| connection_failed(&config.port, &e.to_string()))
    }

    /// 启动数据接收任务
    async fn start_data_receiver(&self) -> SerialResult<()> {
        let (tx, rx) = mpsc::channel(1000);
        *self.data_receiver.write().await = Some(rx);
        *self.data_sender.write().await = Some(tx.clone());

        // 这里需要实现实际的数据接收逻辑
        // 由于需要访问 stream，这部分需要进一步实现

        Ok(())
    }

    /// 格式化数据用于显示
    fn format_data_for_display(&self, data: &[u8], format: &DataFormat) -> String {
        match format {
            DataFormat::Text => String::from_utf8_lossy(data).to_string(),
            DataFormat::Hex => data
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" "),
            DataFormat::Binary => data
                .iter()
                .map(|b| format!("{:08b}", b))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

impl Statistics {
    pub fn new() -> Self {
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            errors_count: 0,
            connection_time: None,
            last_activity: None,
        }
    }
}

/// 转换数据位配置
fn convert_data_bits(bits: u8) -> SerialResult<tokio_serial::DataBits> {
    match bits {
        5 => Ok(tokio_serial::DataBits::Five),
        6 => Ok(tokio_serial::DataBits::Six),
        7 => Ok(tokio_serial::DataBits::Seven),
        8 => Ok(tokio_serial::DataBits::Eight),
        _ => Err(SerialError::InvalidConfiguration(format!(
            "不支持的数据位: {}",
            bits
        ))),
    }
}

/// 转换停止位配置
fn convert_stop_bits(bits: u8) -> SerialResult<tokio_serial::StopBits> {
    match bits {
        1 => Ok(tokio_serial::StopBits::One),
        2 => Ok(tokio_serial::StopBits::Two),
        _ => Err(SerialError::InvalidConfiguration(format!(
            "不支持的停止位: {}",
            bits
        ))),
    }
}

/// 转换校验配置
fn convert_parity(parity: &Parity) -> SerialResult<tokio_serial::Parity> {
    match parity {
        Parity::None => Ok(tokio_serial::Parity::None),
        Parity::Odd => Ok(tokio_serial::Parity::Odd),
        Parity::Even => Ok(tokio_serial::Parity::Even),
    }
}

/// 转换流控制配置
fn convert_flow_control(flow: &FlowControl) -> SerialResult<tokio_serial::FlowControl> {
    match flow {
        FlowControl::None => Ok(tokio_serial::FlowControl::None),
        FlowControl::Software => Ok(tokio_serial::FlowControl::Software),
        FlowControl::Hardware => Ok(tokio_serial::FlowControl::Hardware),
    }
}
