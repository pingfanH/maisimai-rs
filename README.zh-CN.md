# maisimai

Pure-Rust **Simai (maimai)** 谱面解析与导出库，移植自 [MaiConverter](https://github.com/donmai-me/MaiConverter) 的 `simai` 模块。

## 功能

- 解析 `maidata.txt` 文件（`&title`、`&artist`、`&first`、`&lv_N`、`&inote_N` 等元数据）
- 解析谱面正文：Tap、Hold、Slide、TouchTap、TouchHold
- 支持 BPM 变速 `(NUM)` 和节拍细分 `{N}` 指令
- 支持全部常见 Slide 形状：`-`、`^`、`<`、`>`、`s`、`z`、`v`、`p`、`q`、`pp`、`qq`、`V`、`w`
- Slide 链式连接（`*`）
- `measure ↔ seconds` 时间转换工具函数
- 将内部数据结构导出回 Simai 格式文本

## 快速开始

```toml
# Cargo.toml
[dependencies]
maisimai = { path = "../maisimai" }
```

### 解析谱面文本

```rust
use maisimai::{parse_chart_text, SimaiChart};

let text = "(160){4}1,3,5h[4:1],7,E";
let chart: SimaiChart = parse_chart_text(text).unwrap();
println!("notes: {}", chart.notes.len());
```

### 解析完整 maidata.txt

```rust
use maisimai::parse_file;

let content = std::fs::read_to_string("maidata.txt").unwrap();
let file = parse_file(&content).unwrap();
println!("title: {}", file.title);
for (diff, chart) in &file.charts {
    println!("  diff {} — {} notes", diff, chart.notes.len());
}
```

### 导出

```rust
use maisimai::{export_chart, export_file};

// 导出单个谱面
let simai_text = export_chart(&chart);

// 导出完整文件
let maidata = export_file(&file);
std::fs::write("maidata_out.txt", maidata).unwrap();
```

### 时间转换

```rust
use maisimai::{measure_to_seconds, seconds_to_measure, Bpm};

let bpms = vec![Bpm { measure: 1.0, bpm: 150.0 }];
let secs = measure_to_seconds(3.0, &bpms); // 第3小节对应秒数
let meas = seconds_to_measure(secs, &bpms); // 秒数转回小节
```

## 数据模型

| 类型 | 说明 |
|------|------|
| `SimaiFile` | 完整 maidata 文件（标题、艺术家、偏移、多难度谱面） |
| `SimaiChart` | 单个难度谱面（BPM 列表 + 音符列表） |
| `SimaiNote` | 音符枚举：`Tap`、`Hold`、`Slide`、`TouchTap`、`TouchHold` |
| `SlidePattern` | Slide 形状枚举（Line、Caret、Left、Right、LowerV、BigV、S、Z、P、Q、Pp、Qq、Wi） |
| `Bpm` | BPM 变速点（小节位置 + BPM 值） |

## 支持的 Simai 语法

```
(BPM)       — 变速
{N}         — 节拍细分（N 分音符/小节）
1-8         — Tap（键位 1~8）
1h[4:1]     — Hold（时值 = 4分音符 × 1拍）
1-5[8:3]    — Slide（从键1到键5，时值 8分 × 3）
C / B3 / E2 — Touch（区域 C / B3 / E2）
Cf          — Touch Hold（firework 修饰）
1b          — Break Tap
/           — Each（同时多个音符）
,           — 分隔（前进一个细分单位）
E           — 谱面结束标记
```

## 许可证

MIT
