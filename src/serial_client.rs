use crate::config::{DataFormat, FlowControl, Parity, SerialConfig};
use crate::error::{connection_failed, SerialError, SerialResult};

use plugin_interfaces::{log_debug, log_info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

/// 串口命令枚举
#[derive(Debug)]
pub enum SerialCommand {
    Connect(SerialConfig),
    Disconnect,
    SendData(Vec<u8>, DataFormat),
    StartReading,
}

/// 串口响应枚举
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum SerialResponse {
    Connected(bool, String),
    Disconnected,
    DataSent(bool, usize, String),
    DataReceived(Vec<u8>),
    ReadError(String),
}

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
    /// 统计信息
    statistics: RwLock<Statistics>,
    /// 连接开始时间
    connection_start: RwLock<Option<Instant>>,
    /// 命令发送通道
    pub command_sender: RwLock<Option<mpsc::UnboundedSender<SerialCommand>>>,
    /// 数据接收通道
    pub data_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<SerialResponse>>>>,
    /// 后台任务句柄
    task_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
}

/// 线程安全标记
unsafe impl Send for SerialClient {}
unsafe impl Sync for SerialClient {}

impl SerialClient {
    /// 创建新的串口客户端
    pub fn new() -> Self {
        Self {
            config: RwLock::new(SerialConfig::default()),
            status: RwLock::new(ConnectionStatus::Disconnected),
            statistics: RwLock::new(Statistics::new()),
            connection_start: RwLock::new(None),
            command_sender: RwLock::new(None),
            data_receiver: Arc::new(Mutex::new(None)),
            task_handle: RwLock::new(None),
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
    /// 后台串口任务处理器
    async fn serial_task_handler(
        mut command_rx: mpsc::UnboundedReceiver<SerialCommand>,
        data_tx: mpsc::UnboundedSender<SerialResponse>,
    ) {
        let mut stream: Option<SerialStream> = None;
        let mut _current_config = SerialConfig::default();
        let mut should_read = false;

        loop {
            tokio::select! {
                // 处理命令
                command = command_rx.recv() => {
                    match command {
                        Some(SerialCommand::Connect(new_config)) => {
                            let result = Self::create_serial_stream_blocking(&new_config).await;
                            match result {
                                Ok(new_stream) => {
                                    stream = Some(new_stream);
                                    _current_config = new_config;
                                    let _ = data_tx.send(SerialResponse::Connected(true, "连接成功".to_string()));
                                }
                                Err(e) => {
                                    let _ = data_tx.send(SerialResponse::Connected(false, e.to_string()));
                                }
                            }
                        }
                        Some(SerialCommand::Disconnect) => {
                            should_read = false;
                            stream = None;
                            let _ = data_tx.send(SerialResponse::Disconnected);
                        }
                        Some(SerialCommand::SendData(data, _format)) => {
                            if let Some(ref mut s) = stream {
                                match s.write_all(&data).await {
                                    Ok(_) => {
                                        let _ = data_tx.send(SerialResponse::DataSent(true, data.len(), "发送成功".to_string()));
                                    }
                                    Err(e) => {
                                        let _ = data_tx.send(SerialResponse::DataSent(false, 0, e.to_string()));
                                    }
                                }
                            } else {
                                let _ = data_tx.send(SerialResponse::DataSent(false, 0, "未连接".to_string()));
                            }
                        }
                        Some(SerialCommand::StartReading) => {
                            should_read = true;
                        }
                        None => break,
                    }
                }
                // 读取数据
                _ = async {
                    if should_read && stream.is_some() {
                        if let Some(ref mut s) = stream {
                            let mut buffer = [0u8; 512];
                            match s.read(&mut buffer).await {
                                Ok(0) => {
                                    // 连接关闭
                                    let _ = data_tx.send(SerialResponse::ReadError("连接已关闭".to_string()));
                                    should_read = false;
                                }
                                Ok(n) => {
                                    // 成功读取数据
                                    let data = buffer[..n].to_vec();
                                    let _ = data_tx.send(SerialResponse::DataReceived(data));
                                }
                                Err(e) => {
                                    // 读取错误
                                    let _ = data_tx.send(SerialResponse::ReadError(e.to_string()));
                                    should_read = false;
                                }
                            }
                        }
                    } else {
                        // 避免忙等待
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    }
                }, if should_read && stream.is_some() => {}
            }
        }
    }

    /// 创建串口流的阻塞版本
    async fn create_serial_stream_blocking(config: &SerialConfig) -> SerialResult<SerialStream> {
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
    /// 连接到串口
    pub async fn connect(&self, config: SerialConfig) -> SerialResult<()> {
        // 更新状态为连接中
        *self.status.write().await = ConnectionStatus::Connecting;

        // 验证配置
        self.validate_config(&config).await?;

        // 创建命令和数据通道
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (data_tx, data_rx) = mpsc::unbounded_channel();

        // 启动后台任务
        let task_handle = tokio::spawn(Self::serial_task_handler(command_rx, data_tx));
        *self.task_handle.write().await = Some(task_handle);
        *self.command_sender.write().await = Some(command_tx.clone());
        *self.data_receiver.lock().await = Some(data_rx);

        // 发送连接命令
        command_tx
            .send(SerialCommand::Connect(config.clone()))
            .map_err(|_| SerialError::ChannelError("发送连接命令失败".to_string()))?;

        // 等待连接结果
        if let Some(receiver) = self.data_receiver.lock().await.as_mut() {
            if let Some(response) = receiver.recv().await {
                match response {
                    SerialResponse::Connected(success, message) => {
                        if success {
                            *self.config.write().await = config;
                            *self.status.write().await = ConnectionStatus::Connected;
                            *self.connection_start.write().await = Some(Instant::now());
                            log_info!("串口连接成功: {}", self.config.read().await.port);
                            Ok(())
                        } else {
                            *self.status.write().await = ConnectionStatus::Error(message.clone());
                            Err(SerialError::ConnectionFailed(message))
                        }
                    }
                    _ => Err(SerialError::ChannelError("收到意外响应".to_string())),
                }
            } else {
                Err(SerialError::ChannelError("未收到连接响应".to_string()))
            }
        } else {
            Err(SerialError::ChannelError(
                "数据接收通道未初始化".to_string(),
            ))
        }
    }
    /// 断开连接
    pub async fn disconnect(&self) -> SerialResult<()> {
        // 发送断开命令
        if let Some(command_sender) = self.command_sender.read().await.as_ref() {
            let _ = command_sender.send(SerialCommand::Disconnect);
        }

        // 等待后台任务完成
        if let Some(handle) = self.task_handle.write().await.take() {
            let _ = handle.await;
        }

        // 清理状态
        *self.status.write().await = ConnectionStatus::Disconnected;
        *self.connection_start.write().await = None;
        *self.command_sender.write().await = None;
        *self.data_receiver.lock().await = None;

        log_info!("串口连接已断开");
        Ok(())
    }
    /// 发送数据
    pub async fn send_data(&self, data: &[u8], format: DataFormat) -> SerialResult<()> {
        if let Some(command_sender) = self.command_sender.read().await.as_ref() {
            // 发送数据命令
            command_sender
                .send(SerialCommand::SendData(data.to_vec(), format.clone()))
                .map_err(|_| SerialError::ChannelError("发送数据命令失败".to_string()))?;

            // 更新发送统计信息
            let mut stats = self.statistics.write().await;
            stats.bytes_sent += data.len() as u64;
            stats.packets_sent += 1;
            stats.last_activity = Some(Instant::now());

            log_debug!("发送数据: {} 字节", data.len());
            Ok(())
        } else {
            Err(SerialError::ConnectionFailed("未建立连接".to_string()))
        }
    }

    /// 更新接收统计信息
    pub async fn update_receive_statistics(&self, bytes_count: usize) {
        let mut stats = self.statistics.write().await;
        stats.bytes_received += bytes_count as u64;
        stats.packets_received += 1;
        stats.last_activity = Some(Instant::now());
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

/// 根据指定格式格式化接收到的数据
pub fn format_received_data(bytes: &[u8], format: &DataFormat) -> String {
    match format {
        DataFormat::Text => String::from_utf8_lossy(bytes)
            .chars()
            .map(|c| {
                if c.is_control() && c != '\n' && c != '\r' && c != '\t' {
                    format!("\\x{:02x}", c as u8)
                } else {
                    c.to_string()
                }
            })
            .collect(),
        DataFormat::Hex => bytes
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<String>>()
            .join(" "),
        DataFormat::Binary => bytes
            .iter()
            .map(|b| format!("{:08b}", b))
            .collect::<Vec<String>>()
            .join(" "),
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
