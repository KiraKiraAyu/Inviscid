<h1 align="center">Inviscid</h1>

<p align="center">
    <a href="./README.zh-CN.md">English</a> | 简体中文
</p>

Inviscid 是一个使用 Rust 与 [GPUI](https://www.gpui.rs/) 构建的原生 GPU 加速 Markdown 编辑器。

<!-- ![Screenshot](docs/assets/screenshot.png) -->

Inviscid 处于 `v0.1` 发布前的早期开发阶段。核心编辑、Live Preview 排版与工作区管理已可日常使用，部分 `v0.1` 关键特性仍在持续完善中。

## 发展目标

- **极致性能** — 追求极低的内存占用与微秒级输入/重绘响应
- **深度优化的小体积** — 精简依赖树与内嵌资源体积，保持轻量纯粹的原生单文件形态。
- **丝滑动效** — 提供丝滑的 UI 和光标动效。

## 核心特性

- **Live Preview 与源码双模式** — 所见即所得；也可随时通过 `Ctrl+E` / `Cmd+E` 切换至纯源码（Source）模式。
- **增量解析与按行布局缓存** — 编辑时仅重扫受影响的块级区间并按行缓存字形排版结果；支持带列对齐的 GFM 表格、可点击切换的任务列表、围栏代码块折叠、多级引用块，以及本地/远程图片的异步尺寸嗅探与 LRU 内存缓存。
- **原生输入与动效细节** — 内置 ~120 FPS 立方缓出平滑滚动插值、软换行（Soft Wrap）精确折行测量、中英文括号与引号选区成对包裹，以及不挤压正文排版的输入法预编辑（IME Preedit）内联覆盖层。
- **工作区与多标签页管理** — 支持单窗口多标签页拖拽排序、基于内容哈希与版本基线的未保存状态追踪、防抖自动保存；内置项目文件树侧边栏，支持内联新建与重命名、文件复制/剪切/粘贴以及移至系统回收站。
- **主题与快捷键定制** — 内置 5 套亮/暗配色主题（_Catppuccin Mocha_、_Catppuccin Latte_、_Dracula_、_GitHub Light_、_Nord_），支持通过外部 `.toml` 文件继承或覆盖调色板，所有应用与文件树操作均可在设置面板中直接按键录制改键。

## 路线图（Roadmap）

- [x] Live Preview 焦点语法展开与纯源码（Source）双模式切换
- [x] GFM 表格、任务列表、围栏代码块折叠与本地/远程图片异步尺寸嗅探
- [x] 多标签页工作区、项目文件树管理、TOML 主题继承与可视化快捷键录制
- [x] 围栏代码块语法高亮
- [ ] 数学公式（LaTeX Math）与图表（Mermaid）原生渲染
- [ ] 文档内查找与替换（Find & Replace）及工作区快速打开 / 全局搜索
- [ ] 文档标题大纲（Outline / TOC）视图与标题锚点跳转
- [ ] 文件系统外部变更监听（File Watcher）与冲突感知热重载
- [ ] 底层文本缓冲区向 Rope 结构演进以支撑超大文档编辑
- [ ] HTML / PDF 导出及 Windows / macOS / Linux 三端 `v0.1` 预编译发行包

## 从源码构建

### 环境要求

- **Rust**：1.85+（Rust 2024 Edition）
- **平台依赖**：
  - **Windows**：Windows 10/11 SDK（通过 `blade-graphics` 调用 DirectX / Vulkan）
  - **macOS**：Xcode Command Line Tools
  - **Linux**：Vulkan Loader、X11/Wayland 开发头文件及 `pkg-config`

### 编译与运行

```bash
# 以 Release 模式编译并运行
cargo run --release

# 运行单元与集成测试
cargo test
```

## 配置文件与自定义主题

| 文件 / 目录         | 用途                          | 默认路径（Windows）                  | 默认路径（macOS / Linux）            |
| :------------------ | :---------------------------- | :----------------------------------- | :----------------------------------- |
| `config.toml`       | 用户偏好设置与自定义快捷键    | `%APPDATA%\inviscid\config.toml`     | `~/.config/inviscid/config.toml`     |
| `state.toml`        | 已打开标签页与最近工作区历史  | `%LOCALAPPDATA%\inviscid\state.toml` | `~/.local/share/inviscid/state.toml` |
| `themes/*.toml`     | 用户自定义主题                | `%APPDATA%\inviscid\themes\`         | `~/.config/inviscid/themes/`         |
| `grammars/`（缓存） | 从 CDN 下载的语法包与高亮查询 | `%LOCALAPPDATA%\inviscid\grammars\`  | `~/.local/share/inviscid/grammars/`  |

### 自定义主题

将 `.toml` 文件放入 `themes/` 目录即可在启动时注册新主题或覆盖同名内置主题。未显式指定的颜色字段会自动回退继承自默认基准调色板（`Catppuccin Mocha`）：

```toml
name = "My Custom Theme"

[colors]
bg_primary = "#1a1b26"
bg_secondary = "#16161e"
text_primary = "#c0caf5"
accent = "#7aa2f7"
```

## 开源协议

GNU General Public License v3.0（GPL-3.0）
