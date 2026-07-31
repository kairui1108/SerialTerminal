use std::fmt::Write as _;

/// 串口编码模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Gbk,
    Auto,
    Raw,
}

impl Encoding {
    pub fn from_str_name(s: &str) -> Self {
        match s {
            "gbk" => Encoding::Gbk,
            "auto" => Encoding::Auto,
            "raw" => Encoding::Raw,
            _ => Encoding::Utf8,
        }
    }

    pub fn as_str_name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf8",
            Encoding::Gbk => "gbk",
            Encoding::Auto => "auto",
            Encoding::Raw => "raw",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Gbk => "GBK",
            Encoding::Auto => "AUTO",
            Encoding::Raw => "HEX",
        }
    }
}

/// 流式解码器：在字节边界处理任意 chunk 切割，
/// 内部持有半字符状态，彻底解决跨 chunk 中文乱码。
pub struct StreamDecoder {
    enc: Encoding,
    utf8: encoding_rs::Decoder,
    gbk: encoding_rs::Decoder,
    // Auto 检测：缓冲 + 判定后的解码器
    auto_buf: Vec<u8>,
    auto_dec: Option<encoding_rs::Decoder>,
}

impl StreamDecoder {
    pub fn new(enc: Encoding) -> Self {
        Self {
            enc,
            utf8: encoding_rs::UTF_8.new_decoder(),
            gbk: encoding_rs::GBK.new_decoder(),
            auto_buf: Vec::new(),
            auto_dec: None,
        }
    }

    /// 输入串口字节块，输出可渲染的 Unicode 文本
    pub fn push(&mut self, bytes: &[u8]) -> String {
        match self.enc {
            Encoding::Raw => {
                // 一次预分配，避免逐字节 format! 的重复分配
                let mut s = String::with_capacity(bytes.len() * 5);
                for b in bytes {
                    let _ = write!(s, "[{:02X}]", b);
                }
                s
            }
            Encoding::Utf8 => decode_stream(&mut self.utf8, bytes),
            Encoding::Gbk => decode_stream(&mut self.gbk, bytes),
            Encoding::Auto => self.push_auto(bytes),
        }
    }

    fn push_auto(&mut self, bytes: &[u8]) -> String {
        if let Some(dec) = &mut self.auto_dec {
            return decode_stream(dec, bytes);
        }
        // 未判定：缓冲（不输出），到达阈值后用检测结果解码全部缓冲
        self.auto_buf.extend_from_slice(bytes);
        if self.auto_buf.len() >= 4096 {
            let mut det = chardetng::EncodingDetector::new();
            det.feed(&self.auto_buf, true);
            let enc = det.guess(None, true);
            let mut dec = match enc.name() {
                "GBK" | "GB18030" => encoding_rs::GBK.new_decoder(),
                "UTF-16LE" => encoding_rs::UTF_16LE.new_decoder(),
                "UTF-16BE" => encoding_rs::UTF_16BE.new_decoder(),
                _ => encoding_rs::UTF_8.new_decoder(),
            };
            let old = std::mem::take(&mut self.auto_buf);
            let out = decode_stream(&mut dec, &old);
            self.auto_dec = Some(dec);
            out
        } else {
            String::new()
        }
    }
}

/// 增量解码：先按最大输出长度预留容量。
/// 注意：`decode_to_string` 用 dst 的 capacity 作为输出缓冲，
/// 空字符串容量为 0 会返回 `OutputFull`，必须预先 reserve。
fn decode_stream(dec: &mut encoding_rs::Decoder, bytes: &[u8]) -> String {
    let mut dst = String::new();
    if let Some(need) = dec.max_utf8_buffer_length(bytes.len()) {
        dst.reserve(need);
    }
    let _ = dec.decode_to_string(bytes, &mut dst, false);
    dst
}

/// 发送侧编码器：Unicode -> 设备期望编码
pub struct StreamEncoder {
    enc: Encoding,
}

impl StreamEncoder {
    pub fn new(enc: Encoding) -> Self {
        Self { enc }
    }

    pub fn encode(&mut self, text: &str) -> Vec<u8> {
        match self.enc {
            // 一次性编码：输入为完整文本，无跨调用状态需求
            Encoding::Gbk => encoding_rs::GBK.encode(text).0.into_owned(),
            _ => text.as_bytes().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gbk_split_chunk() {
        // "你" 的 GBK 编码为 C4E3，拆成两个 chunk
        let mut dec = StreamDecoder::new(Encoding::Gbk);
        let first = dec.push(&[0xC4]);
        let second = dec.push(&[0xE3]);
        assert_eq!(first + &second, "你");
    }

    #[test]
    fn utf8_split_4byte() {
        // "🙂" UTF-8 为 F0 9F 99 82，拆三次
        let mut dec = StreamDecoder::new(Encoding::Utf8);
        let mut out = String::new();
        for b in [0xF0u8, 0x9F, 0x99, 0x82] {
            out.push_str(&dec.push(&[b]));
        }
        assert_eq!(out, "🙂");
    }

    #[test]
    fn gbk_encode() {
        let mut enc = StreamEncoder::new(Encoding::Gbk);
        assert_eq!(enc.encode("你"), vec![0xC4, 0xE3]);
    }

    #[test]
    fn auto_detect_gbk() {
        let mut dec = StreamDecoder::new(Encoding::Auto);
        // 构造超过 4096 字节的 GBK 文本以触发检测
        let mut bytes = Vec::new();
        let text = "你好，串口终端。";
        let encoded = encoding_rs::GBK.encode(text).0.into_owned();
        while bytes.len() < 4096 {
            bytes.extend_from_slice(&encoded);
        }
        let out = dec.push(&bytes);
        assert!(!out.is_empty());
        assert!(out.contains("你好"));
    }
}
