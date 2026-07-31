use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::time::Duration;

/// 串口连接参数
#[derive(Debug, Clone)]
pub struct PortParams {
    pub name: String,
    pub baud: u32,
    pub databits: u8,
    pub stopbits: String,
    pub parity: String,
    pub flow: String,
}

impl PortParams {
    pub fn open(&self) -> anyhow::Result<Box<dyn serialport::SerialPort>> {
        let stop = match self.stopbits.as_str() {
            "2" => StopBits::Two,
            _ => StopBits::One,
        };
        let parity = match self.parity.as_str() {
            "odd" => Parity::Odd,
            "even" => Parity::Even,
            _ => Parity::None,
        };
        let flow = match self.flow.as_str() {
            "software" => FlowControl::Software,
            "hardware" => FlowControl::Hardware,
            _ => FlowControl::None,
        };
        let databits = match self.databits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        };

        let port = serialport::new(&self.name, self.baud)
            .data_bits(databits)
            .stop_bits(stop)
            .parity(parity)
            .flow_control(flow)
            .timeout(Duration::from_millis(60))
            .open()?;
        Ok(port)
    }
}

/// 串口设备信息（端口名 + 硬件/芯片信息）
#[derive(Debug, Clone)]
pub struct PortDevice {
    /// 端口名，如 COM3 / /dev/ttyUSB0
    pub name: String,
    /// 端口类型描述（USB / PCI / Bluetooth / 未知）
    pub kind: &'static str,
    /// 芯片型号（通过 VID/PID 映射，未知时回退 product/manufacturer）
    pub chip: Option<String>,
    /// 厂商描述（如 WCH.CN / Silicon Labs）
    pub manufacturer: Option<String>,
    /// 产品描述（如 USB-SERIAL CH340）
    pub product: Option<String>,
    /// 序列号
    pub serial: Option<String>,
    /// VID（十六进制字符串，如 1A86）
    pub vid: Option<String>,
    /// PID（十六进制字符串，如 7523）
    pub pid: Option<String>,
}

impl PortDevice {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 设备信息摘要（用于设置面板显示），无信息时返回 None
    pub fn summary(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = &self.chip {
            parts.push(c.clone());
        }
        if let Some(p) = &self.product {
            parts.push(p.clone());
        }
        if let Some(m) = &self.manufacturer {
            parts.push(m.clone());
        }
        if let (Some(v), Some(p)) = (&self.vid, &self.pid) {
            parts.push(format!("VID:{} PID:{}", v, p));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" · "))
        }
    }
}

/// 常见 USB 串口芯片 VID/PID 映射表（用于识别具体芯片型号）
const CHIP_DB: &[(u16, u16, &str)] = &[
    // ===== WCH 沁恒（国产）=====
    (0x1A86, 0x7523, "WCH CH340"),
    (0x1A86, 0x7522, "WCH CH340G"),
    (0x1A86, 0x7525, "WCH CH341"),
    (0x1A86, 0x55D3, "WCH CH342"),
    (0x1A86, 0x55D4, "WCH CH9102"),
    (0x1A86, 0x55D5, "WCH CH9103"),
    (0x1A86, 0x55E0, "WCH CH9101"),
    (0x1A86, 0x55D6, "WCH CH9143"),
    (0x1A86, 0x7524, "WCH CH9344"),
    (0x1A86, 0x5512, "WCH CH9340"),
    (0x1A86, 0x55D7, "WCH CH9434"),
    (0x1A86, 0x5524, "WCH CH9326"),
    (0x0D6F, 0x0005, "WCH CH9101"),
    // ===== Silicon Labs 芯科 =====
    (0x10C4, 0xEA60, "Silicon Labs CP2102/CP210x"),
    (0x10C4, 0xEA70, "Silicon Labs CP2105"),
    (0x10C4, 0xEA71, "Silicon Labs CP2108"),
    (0x10C4, 0xEA80, "Silicon Labs CP2110"),
    (0x10C4, 0xEA90, "Silicon Labs CP2615"),
    (0x10C4, 0xEA7A, "Silicon Labs EFM32"),
    (0x10C4, 0xEA7B, "Silicon Labs Gecko"),
    (0x10C4, 0x84C4, "Silicon Labs USB-UART"),
    // ===== FTDI 飞特蒂（模拟也常见）=====
    (0x0403, 0x6001, "FTDI FT232R"),
    (0x0403, 0x6010, "FTDI FT2232C"),
    (0x0403, 0x6011, "FTDI FT4232"),
    (0x0403, 0x6014, "FTDI FT232H"),
    (0x0403, 0x6015, "FTDI FT230X"),
    (0x0403, 0x601C, "FTDI FT231X"),
    (0x0403, 0x6004, "FTDI FT232B"),
    (0x0403, 0x6006, "FTDI FT232BM"),
    (0x0403, 0x6016, "FTDI FT2232H"),
    (0x0403, 0x6018, "FTDI FT2233H"),
    (0x0403, 0x601F, "FTDI FT2232D"),
    (0x0403, 0xCB48, "FTDI FT232F"),
    // ===== Prolific 旺玖 =====
    (0x067B, 0x2303, "Prolific PL2303"),
    (0x1BCB, 0x0001, "Prolific PL2303HXA"),
    (0x0658, 0x0200, "Prolific PL2303GC"),
    (0x0658, 0x0201, "Prolific PL2303GS"),
    (0x0658, 0x0202, "Prolific PL2303GB"),
    (0x067B, 0x23A3, "Prolific PL2303TA"),
    (0x067B, 0x23B3, "Prolific PL2303TB"),
    (0x067B, 0x23C3, "Prolific PL2303TC"),
    // ===== Espressif 乐鑫（ESP 系列原生 USB）=====
    (0x303A, 0x0001, "Espressif ESP32-C3 (USB Serial)"),
    (0x303A, 0x0002, "Espressif ESP32-S2 (USB Serial)"),
    (0x303A, 0x0003, "Espressif ESP32-S3 (USB Serial)"),
    (0x303A, 0x0004, "Espressif ESP32-C6 (USB Serial)"),
    (0x303A, 0x0005, "Espressif ESP32-H2 (USB Serial)"),
    (0x303A, 0x0006, "Espressif ESP32-C2 (USB Serial)"),
    (0x303A, 0x0009, "Espressif ESP32-P4 (USB Serial)"),
    (0x303A, 0x1001, "Espressif ESP32-C3 (USB Serial/JTAG)"),
    (0x303A, 0x1002, "Espressif ESP32-S2 (USB Serial/JTAG)"),
    (0x303A, 0x1003, "Espressif ESP32-S3 (USB Serial/JTAG)"),
    (0x303A, 0x1004, "Espressif ESP32-C6 (USB Serial/JTAG)"),
    (0x303A, 0x1005, "Espressif ESP32-H2 (USB Serial/JTAG)"),
    (0x303A, 0x1006, "Espressif ESP32-C2 (USB Serial/JTAG)"),
    (0x303A, 0x1009, "Espressif ESP32-P4 (USB Serial/JTAG)"),
    // ===== ST 意法半导体（STM32 板载调试/串口）=====
    (0x0483, 0x5740, "ST STM32 VCP (Virtual COM Port)"),
    (0x0483, 0x5741, "ST STM32 USB CDC"),
    (0x0483, 0x374B, "ST ST-LINK/V2"),
    (0x0483, 0x374E, "ST ST-LINK/V3"),
    (0x0483, 0x3748, "ST ST-LINK/V2"),
    (0x0483, 0x3747, "ST ST-LINK"),
    (0x0483, 0x374A, "ST Nucleo Virtual COM"),
    (0x0483, 0x572B, "ST Motor Control VCP"),
    (0x0483, 0x5750, "ST STM32 Discovery"),
    // ===== Microchip / Atmel =====
    (0x03EB, 0x2104, "Microchip MCP2200"),
    (0x03EB, 0x2120, "Microchip MCP2221"),
    (0x03EB, 0x2404, "Microchip SAMD USB CDC"),
    (0x03EB, 0x6124, "Microchip AVR (CDC)"),
    (0x03EB, 0x6127, "Microchip AVR (Caterina/Leonardo)"),
    (0x03EB, 0x2044, "Microchip AVR (Mega)"),
    (0x03EB, 0x2144, "Microchip AVR (Uno R3)"),
    (0x16C0, 0x0483, "Teensy (AVR)"),
    (0x16C0, 0x0484, "Teensy (USB Serial)"),
    (0x16C0, 0x04D0, "Arduino/Genuino Uno"),
    (0x2341, 0x0043, "Arduino Uno R3 (ATmega328P)"),
    (0x2341, 0x0001, "Arduino Uno"),
    (0x2341, 0x0243, "Arduino Mega 2560"),
    (0x2341, 0x0042, "Arduino Mega 2560 R3"),
    (0x2341, 0x0041, "Arduino Mega ADK"),
    (0x2341, 0x0036, "Arduino Leonardo"),
    (0x2341, 0x8036, "Arduino Leonardo (old)"),
    (0x2341, 0x0039, "Arduino Micro"),
    (0x2341, 0x8037, "Arduino Micro (old)"),
    (0x2341, 0x003B, "Arduino Nano Every"),
    (0x2341, 0x0058, "Arduino Nano BLE"),
    (0x1A86, 0x55D2, "Arduino (WCH)"),
    (0x2A03, 0x0043, "Arduino Uno (AVR ISP)"),
    // ===== NXP / Freescale / TI =====
    (0x15A2, 0x0042, "NXP Freescale (CDC)"),
    (0x15A2, 0x0073, "NXP FRDM-KL25Z"),
    (0x15A2, 0x0076, "NXP FRDM-K64F"),
    (0x15A2, 0x0033, "NXP LPC (CDC)"),
    (0x0D28, 0x0204, "NXP mbed"),
    (0x1D50, 0x60C4, "TI MSP430 (MSP-EXP432)"),
    (0x0451, 0xF432, "TI MSP432 LaunchPad"),
    (0x0451, 0xC32A, "TI Tiva/Stellaris"),
    (0x0451, 0x16C2, "TI CC3200"),
    // ===== Raspberry Pi Pico / RP2040 =====
    (0x2E8A, 0x000A, "Raspberry Pi Pico (RP2040 CDC)"),
    (0x2E8A, 0x0005, "Raspberry Pi Pico (RP2040)"),
    (0x2E8A, 0x0007, "Raspberry Pi Pico W (RP2040)"),
    (0x2E8A, 0x000D, "Raspberry Pi Pico (Picoprobe)"),
    (0x2E8A, 0x0003, "Raspberry Pi Pico (BOOTSEL)"),
    // ===== 其他常见 =====
    (0x04F3, 0x4000, "ELAN"),
    (0x413C, 0x2106, "Dell"),
    (0x0461, 0x4A2A, "HP"),
    (0x0FE4, 0x0178, "Jolla"),
    (0x0403, 0x600A, "FTDI FT232L"),
];

fn chip_for_vid_pid(vid: u16, pid: u16) -> Option<String> {
    CHIP_DB
        .iter()
        .find(|(v, p, _)| *v == vid && *p == pid)
        .map(|(_, _, name)| name.to_string())
}

/// 枚举系统可用串口（含硬件/芯片信息）
pub fn list_ports() -> Vec<PortDevice> {
    match serialport::available_ports() {
        Ok(ports) => ports
            .into_iter()
            .map(|p| {
                let (kind, chip, manufacturer, product, serial, vid, pid) = match p.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        let chip = chip_for_vid_pid(info.vid, info.pid);
                        (
                            "USB",
                            chip,
                            info.manufacturer,
                            info.product,
                            info.serial_number,
                            Some(format!("{:04X}", info.vid)),
                            Some(format!("{:04X}", info.pid)),
                        )
                    }
                    serialport::SerialPortType::PciPort => {
                        ("PCI/板载", None, None, None, None, None, None)
                    }
                    serialport::SerialPortType::BluetoothPort => {
                        ("蓝牙", None, None, None, None, None, None)
                    }
                    serialport::SerialPortType::Unknown => {
                        ("未知", None, None, None, None, None, None)
                    }
                };
                PortDevice {
                    name: p.port_name,
                    kind,
                    chip,
                    manufacturer,
                    product,
                    serial,
                    vid,
                    pid,
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 关键厂商/芯片映射必须命中
    #[test]
    fn chip_mapping_known_devices() {
        let cases = [
            (0x1A86, 0x7523, "WCH CH340"),
            (0x10C4, 0xEA60, "Silicon Labs CP2102/CP210x"),
            (0x0403, 0x6001, "FTDI FT232R"),
            (0x067B, 0x2303, "Prolific PL2303"),
            (0x303A, 0x1001, "Espressif ESP32-C3 (USB Serial/JTAG)"),
            (0x303A, 0x0003, "Espressif ESP32-S3 (USB Serial)"),
            (0x0483, 0x5740, "ST STM32 VCP (Virtual COM Port)"),
            (0x2E8A, 0x000A, "Raspberry Pi Pico (RP2040 CDC)"),
        ];
        for (vid, pid, expected) in cases {
            let got = chip_for_vid_pid(vid, pid).expect("映射缺失");
            assert_eq!(got, expected, "VID={:04X} PID={:04X}", vid, pid);
        }
    }

    /// 未知 VID/PID 应返回 None
    #[test]
    fn chip_mapping_unknown_returns_none() {
        assert!(chip_for_vid_pid(0xFFFF, 0xFFFF).is_none());
        assert!(chip_for_vid_pid(0x0000, 0x0000).is_none());
    }

    /// 映射表不应存在重复的 (VID, PID) 条目
    #[test]
    fn chip_db_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for (vid, pid, _) in CHIP_DB {
            let key = (*vid, *pid);
            assert!(seen.insert(key), "重复映射 VID={:04X} PID={:04X}", vid, pid);
        }
    }

    /// VID/PID 十六进制格式化：应输出大写 4 位补零
    #[test]
    fn vid_pid_formatting() {
        let d = PortDevice {
            name: "COM6".into(),
            kind: "USB",
            chip: None,
            manufacturer: None,
            product: None,
            serial: None,
            vid: Some(format!("{:04X}", 0x303A)),
            pid: Some(format!("{:04X}", 0x1001)),
        };
        let s = d.summary().unwrap();
        assert!(s.contains("VID:303A PID:1001"), "摘要: {}", s);
    }

    /// summary 在无任何信息时返回 None
    #[test]
    fn summary_none_when_no_info() {
        let d = PortDevice {
            name: "COM6".into(),
            kind: "USB",
            chip: None,
            manufacturer: None,
            product: None,
            serial: None,
            vid: None,
            pid: None,
        };
        assert!(d.summary().is_none());
    }
}
