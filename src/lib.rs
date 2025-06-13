use plugin_interfaces::{
    create_plugin_interface_from_handler, log_info, log_warn,
    pluginui::{Context, Ui},
    PluginHandler, PluginInstanceContext, PluginInterface,
};
use std::sync::Arc;
use tokio::{runtime::Runtime, sync::Mutex};

use crate::config::{
    create_default_user_config, load_user_config, save_user_config, DataFormat, Parity, UserConfig,
};
use crate::error::SerialError;
use crate::serial_client::{PortInfo, SerialClient};

mod config;
mod error;
mod serial_client;

/// 串口调试助手插件
#[derive(Clone)]
pub struct SerialDebugAssistant {
    /// 用户配置
    config: Arc<Mutex<UserConfig>>,
    /// 串口客户端
    client: Arc<SerialClient>,
    /// 可用端口列表
    available_ports: Arc<Mutex<Vec<PortInfo>>>,
    /// 选择的端口
    selected_port: Option<String>,
    /// 连接状态
    is_connected: Arc<Mutex<bool>>,
    /// Tokio运行时
    runtime: Option<Arc<Runtime>>,
}

impl SerialDebugAssistant {
    fn new() -> Self {
        Self {
            config: Arc::new(Mutex::new(load_user_config().unwrap_or_else(|_| {
                log_warn!("Failed to load user config, using default");
                create_default_user_config()
            }))),
            client: Arc::new(SerialClient::new()),
            available_ports: Arc::new(Mutex::new(Vec::new())),
            selected_port: None,
            is_connected: Arc::new(Mutex::new(false)),
            runtime: None,
        }
    }

    /// 异步扫描可用端口
    async fn scan_ports(&self) -> Vec<PortInfo> {
        match SerialClient::get_available_ports().await {
            Ok(ports) => {
                log_info!("扫描到 {} 个串口", ports.len());
                ports
            }
            Err(e) => {
                log_warn!("扫描串口失败: {}", e);
                Vec::new()
            }
        }
    }

    /// 连接串口
    async fn connect_serial(&self, plugin_ctx: &PluginInstanceContext) -> Result<(), SerialError> {
        if let Some(port_name) = &self.selected_port {
            let config = self.config.lock().await;
            let serial_config = &config.serial;

            // 创建串口配置
            let mut port_config = serial_config.clone();
            port_config.port = port_name.clone();

            match self.client.connect(port_config).await {
                Ok(_) => {
                    *self.is_connected.lock().await = true;
                    plugin_ctx.send_message_to_frontend(&format!("串口连接成功: {}", port_name));
                    plugin_ctx.refresh_ui();
                    Ok(())
                }
                Err(e) => {
                    plugin_ctx.send_message_to_frontend(&format!("串口连接失败: {}", e));
                    Err(e)
                }
            }
        } else {
            let error = SerialError::PluginError("未选择端口".to_string());
            plugin_ctx.send_message_to_frontend("未选择端口");
            Err(error)
        }
    }

    /// 断开串口连接
    async fn disconnect_serial(&self, plugin_ctx: &PluginInstanceContext) {
        match self.client.disconnect().await {
            Ok(_) => {
                *self.is_connected.lock().await = false;
                plugin_ctx.send_message_to_frontend("串口已断开");
                plugin_ctx.refresh_ui();
            }
            Err(e) => {
                plugin_ctx.send_message_to_frontend(&format!("断开串口失败: {}", e));
            }
        }
    }

    /// 发送数据
    async fn send_data(&self, data: &str, plugin_ctx: &PluginInstanceContext) {
        if !*self.is_connected.lock().await {
            plugin_ctx.send_message_to_frontend("请先连接串口");
            return;
        }

        let config = self.config.lock().await;
        let data_format = &config.data.format;

        // 根据格式转换数据
        let bytes = match data_format {
            DataFormat::Text => data.as_bytes().to_vec(),
            DataFormat::Hex => {
                // 解析十六进制字符串
                hex_to_bytes(data).unwrap_or_else(|_| {
                    plugin_ctx.send_message_to_frontend("十六进制格式错误");
                    Vec::new()
                })
            }
            DataFormat::Binary => {
                // 解析二进制字符串
                binary_to_bytes(data).unwrap_or_else(|_| {
                    plugin_ctx.send_message_to_frontend("二进制格式错误");
                    Vec::new()
                })
            }
        };

        if !bytes.is_empty() {
            match self.client.send_data(&bytes, data_format.clone()).await {
                Ok(_) => {
                    plugin_ctx
                        .send_message_to_frontend(&format!("发送数据成功: {} 字节", bytes.len()));
                }
                Err(e) => {
                    plugin_ctx.send_message_to_frontend(&format!("发送数据失败: {}", e));
                }
            }
        }
    }

    /// 保存当前配置
    async fn save_current_config(&self) {
        let config = self.config.lock().await;
        if let Err(e) = save_user_config(&config) {
            log_warn!("保存配置失败: {}", e);
        }
    }
}

impl PluginHandler for SerialDebugAssistant {
    fn update_ui(&mut self, _ctx: &Context, ui: &mut Ui, _plugin_ctx: &PluginInstanceContext) {
        ui.label("串口调试助手");

        // 端口选择区域
        ui.horizontal(|ui| {
            ui.label("选择端口:");

            // 获取端口列表
            let mut port_options = vec!["请选择端口".to_string()];
            if let Ok(ports) = self.available_ports.try_lock() {
                for port in ports.iter() {
                    port_options.push(port.port_name.clone());
                }
            }

            let combo_response =
                ui.combo_box(port_options, &mut self.selected_port, "选择串口端口");
            if combo_response.clicked() {
                log_info!("选择端口: {:?}", self.selected_port);
            }
        });

        // 串口配置区域
        if let Ok(mut config) = self.config.try_lock() {
            ui.horizontal(|ui| {
                ui.label("波特率:");
                let baud_options = vec![
                    "9600".to_string(),
                    "19200".to_string(),
                    "38400".to_string(),
                    "57600".to_string(),
                    "115200".to_string(),
                    "230400".to_string(),
                    "460800".to_string(),
                    "921600".to_string(),
                ];
                let mut baud_selected = Some(config.serial.baud_rate.to_string());

                let baud_response = ui.combo_box(baud_options, &mut baud_selected, "选择波特率");
                if baud_response.clicked() {
                    if let Some(selected) = &baud_selected {
                        if let Ok(new_baud) = selected.parse::<u32>() {
                            config.serial.baud_rate = new_baud;
                            log_info!("波特率改为: {}", new_baud);
                        }
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("数据位:");
                let mut data_bits_str = config.serial.data_bits.to_string();
                let data_bits_response = ui.text_edit_singleline(&mut data_bits_str);
                if data_bits_response.changed() {
                    if let Ok(new_data_bits) = data_bits_str.parse::<u8>() {
                        if (5..=8).contains(&new_data_bits) {
                            config.serial.data_bits = new_data_bits;
                            log_info!("数据位改为: {}", new_data_bits);
                        }
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("停止位:");
                let mut stop_bits_str = config.serial.stop_bits.to_string();
                let stop_bits_response = ui.text_edit_singleline(&mut stop_bits_str);
                if stop_bits_response.changed() {
                    if let Ok(new_stop_bits) = stop_bits_str.parse::<u8>() {
                        if (1..=2).contains(&new_stop_bits) {
                            config.serial.stop_bits = new_stop_bits;
                            log_info!("停止位改为: {}", new_stop_bits);
                        }
                    }
                }
            });

            // 校验位选择
            ui.horizontal(|ui| {
                ui.label("校验位:");
                let parity_options =
                    vec!["None".to_string(), "Even".to_string(), "Odd".to_string()];
                let mut parity_selected = Some(match config.serial.parity {
                    Parity::None => "None".to_string(),
                    Parity::Even => "Even".to_string(),
                    Parity::Odd => "Odd".to_string(),
                });

                let parity_response =
                    ui.combo_box(parity_options, &mut parity_selected, "选择校验位");
                if parity_response.clicked() {
                    if let Some(selected) = &parity_selected {
                        config.serial.parity = match selected.as_str() {
                            "Even" => Parity::Even,
                            "Odd" => Parity::Odd,
                            _ => Parity::None,
                        };
                        log_info!("校验位改为: {:?}", config.serial.parity);
                    }
                }
            });
        }

        ui.label(""); // 空行

        // 显示连接状态
        let is_connected = self
            .is_connected
            .try_lock()
            .map(|guard| *guard)
            .unwrap_or(false);

        ui.label(&format!(
            "连接状态: {}",
            if is_connected {
                "已连接"
            } else {
                "未连接"
            }
        ));

        // 刷新端口按钮
        if ui.button("刷新端口").clicked() {
            log_info!("点击刷新端口按钮");
            if let Some(runtime) = &self.runtime {
                let self_clone = self.clone();
                runtime.spawn(async move {
                    let ports = self_clone.scan_ports().await;
                    *self_clone.available_ports.lock().await = ports;
                });
            }
        }
    }

    fn on_mount(
        &mut self,
        plugin_ctx: &PluginInstanceContext,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = plugin_ctx.get_metadata();
        log_info!("[{}] 串口调试助手插件挂载成功", metadata.name);

        // 初始化 tokio 异步运行时
        match Runtime::new() {
            Ok(runtime) => {
                self.runtime = Some(Arc::new(runtime));
                log_info!("Tokio 运行时初始化成功");

                // 在挂载时扫描可用端口
                let self_clone = self.clone();
                if let Some(rt) = &self.runtime {
                    rt.spawn(async move {
                        let ports = self_clone.scan_ports().await;
                        *self_clone.available_ports.lock().await = ports;
                    });
                }
            }
            Err(e) => {
                log_warn!("初始化 Tokio 运行时失败: {}", e);
            }
        }

        Ok(())
    }

    fn on_connect(
        &mut self,
        plugin_ctx: &PluginInstanceContext,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = plugin_ctx.get_metadata();
        log_info!("[{}] 串口调试助手插件连接中", metadata.name);

        // 连接时自动连接串口（如果选择了端口）
        if self.selected_port.is_none() {
            return Err("未选择端口".into());
        }

        if let Some(runtime) = &self.runtime {
            let self_clone = self.clone();
            let plugin_ctx_clone = plugin_ctx.clone();

            // 同步等待连接结果
            let result =
                runtime.block_on(async move { self_clone.connect_serial(&plugin_ctx_clone).await });

            match result {
                Ok(_) => {
                    log_info!("[{}] 串口调试助手插件连接成功", metadata.name);
                    Ok(())
                }
                Err(e) => {
                    log_warn!("[{}] 串口连接失败: {}", metadata.name, e);
                    Err(e.into())
                }
            }
        } else {
            Err("运行时未初始化".into())
        }
    }

    fn on_disconnect(
        &mut self,
        plugin_ctx: &PluginInstanceContext,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = plugin_ctx.get_metadata();
        log_info!("[{}] 串口调试助手插件断开连接", metadata.name);

        // 断开时自动关闭串口连接
        if let Some(runtime) = &self.runtime {
            let self_clone = self.clone();
            let plugin_ctx_clone = plugin_ctx.clone();
            runtime.spawn(async move {
                self_clone.disconnect_serial(&plugin_ctx_clone).await;
            });
        }

        Ok(())
    }

    fn on_dispose(
        &mut self,
        plugin_ctx: &PluginInstanceContext,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = plugin_ctx.get_metadata();
        log_info!("[{}] 串口调试助手插件卸载", metadata.name);

        // 卸载时保存配置并清理资源
        if let Some(runtime) = &self.runtime {
            let self_clone = self.clone();
            runtime.spawn(async move {
                self_clone.save_current_config().await;
                let _ = self_clone.client.disconnect().await;
            });
        }

        Ok(())
    }

    fn handle_message(
        &mut self,
        message: &str,
        plugin_ctx: &PluginInstanceContext,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let metadata = plugin_ctx.get_metadata();
        log_info!("[{}] 收到消息: {}", metadata.name, message);

        // 如果串口已连接，将消息作为数据发送
        let is_connected = self
            .is_connected
            .try_lock()
            .map(|guard| *guard)
            .unwrap_or(false);

        if is_connected {
            if let Some(runtime) = &self.runtime {
                let self_clone = self.clone();
                let plugin_ctx_clone = plugin_ctx.clone();
                let message_data = message.to_string();
                runtime.spawn(async move {
                    self_clone.send_data(&message_data, &plugin_ctx_clone).await;
                });
            }
        } else {
            plugin_ctx.send_message_to_frontend("串口未连接，无法发送数据");
        }

        Ok("消息已处理".to_string())
    }
}

/// 十六进制字符串转字节数组
fn hex_to_bytes(hex_str: &str) -> Result<Vec<u8>, String> {
    let hex_str = hex_str.replace(" ", "").replace("\n", "").replace("\r", "");
    if hex_str.len() % 2 != 0 {
        return Err("十六进制字符串长度必须是偶数".to_string());
    }

    let mut bytes = Vec::new();
    for i in (0..hex_str.len()).step_by(2) {
        let hex_byte = &hex_str[i..i + 2];
        match u8::from_str_radix(hex_byte, 16) {
            Ok(byte) => bytes.push(byte),
            Err(_) => return Err(format!("无效的十六进制字符: {}", hex_byte)),
        }
    }
    Ok(bytes)
}

/// 二进制字符串转字节数组
fn binary_to_bytes(binary_str: &str) -> Result<Vec<u8>, String> {
    let binary_str = binary_str
        .replace(" ", "")
        .replace("\n", "")
        .replace("\r", "");
    if binary_str.len() % 8 != 0 {
        return Err("二进制字符串长度必须是8的倍数".to_string());
    }

    let mut bytes = Vec::new();
    for i in (0..binary_str.len()).step_by(8) {
        let binary_byte = &binary_str[i..i + 8];
        match u8::from_str_radix(binary_byte, 2) {
            Ok(byte) => bytes.push(byte),
            Err(_) => return Err(format!("无效的二进制字符: {}", binary_byte)),
        }
    }
    Ok(bytes)
}

/// 创建插件实例的导出函数
#[no_mangle]
pub extern "C" fn create_plugin() -> *mut PluginInterface {
    let plugin = SerialDebugAssistant::new();
    create_plugin_interface_from_handler(Box::new(plugin))
}

/// 销毁插件实例的导出函数
///
/// # Safety
///
/// 此函数是不安全的，因为它直接操作原始指针。
/// 调用者必须确保传入的指针是有效的，并且是通过 `create_plugin` 创建的。
#[no_mangle]
pub unsafe extern "C" fn destroy_plugin(plugin: *mut PluginInterface) {
    if !plugin.is_null() {
        let _ = Box::from_raw(plugin);
    }
}
