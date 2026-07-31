use ratatui::style::Color;

/// UI 主题
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub rx: Color,
    pub tx: Color,
    pub sys: Color,
    pub err: Color,
    pub accent: Color,
    pub border: Color,
}

impl Theme {
    /// 全部可用主题名（供 :theme 补全与 CLI 校验）
    pub const NAMES: &'static [&'static str] = &[
        "github-dark",
        "dracula",
        "monokai",
        "solarized-dark",
        "nord",
        "one-dark",
        "gruvbox",
        "tokyo-night",
    ];

    /// 根据名称返回主题；未知名称回退到 github-dark
    pub fn from_name(name: &str) -> Self {
        match name {
            "dracula" => Self::dracula(),
            "monokai" => Self::monokai(),
            "solarized-dark" => Self::solarized_dark(),
            "nord" => Self::nord(),
            "one-dark" => Self::one_dark(),
            "gruvbox" => Self::gruvbox(),
            "tokyo-night" => Self::tokyo_night(),
            _ => Self::github_dark(),
        }
    }

    /// 主题名（用于持久化，未知时回退）
    pub fn name_of(&self) -> &'static str {
        if *self == Self::dracula() {
            "dracula"
        } else if *self == Self::monokai() {
            "monokai"
        } else if *self == Self::solarized_dark() {
            "solarized-dark"
        } else if *self == Self::nord() {
            "nord"
        } else if *self == Self::one_dark() {
            "one-dark"
        } else if *self == Self::gruvbox() {
            "gruvbox"
        } else if *self == Self::tokyo_night() {
            "tokyo-night"
        } else {
            "github-dark"
        }
    }

    pub fn github_dark() -> Self {
        Self {
            bg: Color::Rgb(0x0d, 0x11, 0x17),
            fg: Color::Rgb(0xc9, 0xd1, 0xd9),
            rx: Color::Rgb(0x3f, 0xb9, 0x50),
            tx: Color::Rgb(0xd2, 0x99, 0x22),
            sys: Color::Rgb(0x58, 0xa6, 0xff),
            err: Color::Rgb(0xf8, 0x51, 0x49),
            accent: Color::Rgb(0x22, 0xd3, 0xee),
            border: Color::Rgb(0x30, 0x3d, 0x50),
        }
    }

    pub fn dracula() -> Self {
        Self {
            bg: Color::Rgb(0x28, 0x2a, 0x36),
            fg: Color::Rgb(0xf8, 0xf8, 0xf2),
            rx: Color::Rgb(0x50, 0xfa, 0x7b),
            tx: Color::Rgb(0xf1, 0xfa, 0x8c),
            sys: Color::Rgb(0x8b, 0xe9, 0xfd),
            err: Color::Rgb(0xff, 0x55, 0x55),
            accent: Color::Rgb(0xff, 0x79, 0xc6),
            border: Color::Rgb(0x44, 0x4b, 0x5a),
        }
    }

    /// Monokai：高对比鲜艳配色
    pub fn monokai() -> Self {
        Self {
            bg: Color::Rgb(0x27, 0x28, 0x22),
            fg: Color::Rgb(0xf8, 0xf8, 0xf2),
            rx: Color::Rgb(0xa6, 0xe2, 0x2e),
            tx: Color::Rgb(0xfd, 0x97, 0x1f),
            sys: Color::Rgb(0x66, 0xd9, 0xef),
            err: Color::Rgb(0xf9, 0x26, 0x72),
            accent: Color::Rgb(0xae, 0x81, 0xff),
            border: Color::Rgb(0x75, 0x71, 0x5e),
        }
    }

    /// Solarized Dark：低饱和暖色调
    pub fn solarized_dark() -> Self {
        Self {
            bg: Color::Rgb(0x00, 0x2b, 0x36),
            fg: Color::Rgb(0x83, 0x94, 0x96),
            rx: Color::Rgb(0x85, 0x99, 0x00),
            tx: Color::Rgb(0xb5, 0x89, 0x00),
            sys: Color::Rgb(0x26, 0x8b, 0xd2),
            err: Color::Rgb(0xdc, 0x32, 0x2f),
            accent: Color::Rgb(0x2a, 0xa1, 0x98),
            border: Color::Rgb(0x58, 0x6e, 0x75),
        }
    }

    /// Nord：极简冷色调
    pub fn nord() -> Self {
        Self {
            bg: Color::Rgb(0x2e, 0x34, 0x40),
            fg: Color::Rgb(0xd8, 0xde, 0xe9),
            rx: Color::Rgb(0xa3, 0xbe, 0x8c),
            tx: Color::Rgb(0xeb, 0xcb, 0x8b),
            sys: Color::Rgb(0x88, 0xc0, 0xd0),
            err: Color::Rgb(0xbf, 0x61, 0x6a),
            accent: Color::Rgb(0x81, 0xa1, 0xc1),
            border: Color::Rgb(0x4c, 0x56, 0x6a),
        }
    }

    /// One Dark：Atom 编辑器风格
    pub fn one_dark() -> Self {
        Self {
            bg: Color::Rgb(0x28, 0x2c, 0x34),
            fg: Color::Rgb(0xab, 0xb2, 0xbf),
            rx: Color::Rgb(0x98, 0xc3, 0x79),
            tx: Color::Rgb(0xe5, 0xc0, 0x7b),
            sys: Color::Rgb(0x61, 0xaf, 0xef),
            err: Color::Rgb(0xe0, 0x6c, 0x75),
            accent: Color::Rgb(0x56, 0xb6, 0xc2),
            border: Color::Rgb(0x3e, 0x44, 0x51),
        }
    }

    /// Gruvbox Dark：暖色调复古
    pub fn gruvbox() -> Self {
        Self {
            bg: Color::Rgb(0x28, 0x28, 0x28),
            fg: Color::Rgb(0xeb, 0xdb, 0xb2),
            rx: Color::Rgb(0xb8, 0xbb, 0x26),
            tx: Color::Rgb(0xfe, 0xb9, 0x65),
            sys: Color::Rgb(0x83, 0xa5, 0x98),
            err: Color::Rgb(0xfb, 0x49, 0x34),
            accent: Color::Rgb(0xd7, 0x99, 0x21),
            border: Color::Rgb(0x50, 0x49, 0x45),
        }
    }

    /// Tokyo Night：夜间蓝紫风
    pub fn tokyo_night() -> Self {
        Self {
            bg: Color::Rgb(0x1a, 0x1b, 0x26),
            fg: Color::Rgb(0xc0, 0xca, 0xf5),
            rx: Color::Rgb(0x9e, 0xce, 0x6a),
            tx: Color::Rgb(0xe0, 0xaf, 0x68),
            sys: Color::Rgb(0x7a, 0xa2, 0xf7),
            err: Color::Rgb(0xf7, 0x76, 0x8e),
            accent: Color::Rgb(0xbb, 0x9a, 0xf7),
            border: Color::Rgb(0x33, 0x35, 0x4a),
        }
    }
}

// 帮助面板内容由 engine.rs 依据 COMMANDS/HELP_GROUPS 动态渲染，此处不再维护静态文本
