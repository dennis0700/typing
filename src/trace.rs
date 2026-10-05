//! 开发期调试追踪设施（**临时诊断代码，非产品功能**）。
//!
//! 用途：定位"创建档案后课程序列界面显示不出来"这类只在真实窗口里才复现、
//! 无法用 `cargo test` 覆盖的渲染问题。Rust 侧的数据链路已由单元测试证明
//! 完好，缺的是**运行时**的可观测性——本模块提供的就是这层可观测性。
//!
//! ## 设计约束
//!
//! - **零新增依赖**：不引入 `log`/`tracing` 等第三方日志设施。这是开发期
//!   临时设施，定位完成后应整体移除，不值得为它增加长期依赖。
//! - **默认零开销**：由环境变量 `TYPING_TRACE` 控制开关。未启用时
//!   [`trace!`] 展开成一次 `OnceLock` 读取 + 分支跳过，**不会**求值任何
//!   格式化参数（`format_args!` 只在分支内被构造），因此可以放心地在
//!   trace 里写 `{:?}` 打印整个 `Vec<UiCommand>`。
//! - **输出到 stderr**：与 Slint 语言内建的 `debug()` 输出同一个流，
//!   便于把 Rust 侧与 `.slint` 侧的诊断行按时间顺序对齐阅读。
//!
//! ## 用法
//!
//! ```ignore
//! crate::trace!("dispatch -> {:?}", event);
//! ```
//!
//! 输出形如：
//!
//! ```text
//! [typing-trace] 12:34:56.789 dispatch -> CreateProfile { nickname: "调试档案" }
//! ```
//!
//! ## 启用方式
//!
//! ```sh
//! TYPING_TRACE=1 cargo run
//! ```

use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// 控制 trace 输出开关的环境变量名。
pub const TRACE_ENV_VAR: &str = "TYPING_TRACE";

/// 所有 trace 行的统一前缀，便于用 `grep '\[typing-trace\]'` 从混杂的
/// stderr（Slint `debug()`、winit/wgpu 的警告等）中筛出本设施的输出。
pub const TRACE_PREFIX: &str = "[typing-trace]";

/// 纯函数：把一个"开关型环境变量"的取值解析为布尔开关。
///
/// 判定规则（大小写不敏感，两端空白被忽略）：
/// - `Some("1")` / `Some("true")` -> `true`
/// - 其他任何取值（含 `Some("0")`、`Some("")`、`Some("yes")`）-> `false`
/// - `None`（变量未设置）-> `false`
///
/// 只认 `1`/`true` 而不做"非空即真"的宽松解析，是为了让
/// `TYPING_TRACE=0` 这种"显式关闭"的写法符合直觉。
pub fn is_flag_enabled(value: Option<&str>) -> bool {
    match value {
        Some(raw) => {
            let normalized = raw.trim();
            normalized.eq_ignore_ascii_case("1") || normalized.eq_ignore_ascii_case("true")
        }
        None => false,
    }
}

/// 纯函数：`TYPING_TRACE` 取值 -> 是否启用 trace 输出。
///
/// 语义与 [`is_flag_enabled`] 完全一致；单独保留这个名字是因为
/// "trace 开关判定"是本模块对外的主要语义入口，也便于单测直接针对它断言
/// （无需触碰真实进程环境变量，避免测试间的环境污染与并发干扰）。
pub fn is_trace_enabled(value: Option<&str>) -> bool {
    is_flag_enabled(value)
}

/// 读取名为 `name` 的环境变量并按 [`is_flag_enabled`] 判定其开关状态。
///
/// 每次调用都会重新读取环境变量（不缓存）——除 `TYPING_TRACE` 之外的开发期
/// 开关（`TYPING_DEV_AUTOSTART` 等）只在启动路径上判定一次，不需要缓存。
pub fn env_flag_enabled(name: &str) -> bool {
    is_flag_enabled(std::env::var(name).ok().as_deref())
}

/// 当前进程是否启用了 trace 输出。
///
/// 结果在首次调用时从 `TYPING_TRACE` 读取并**永久缓存**：trace 埋点位于
/// 每一次回调/派发/刷新的热路径上，不应该每行都去查一次环境变量；同时
/// 缓存也保证了整个进程生命周期内 trace 的开关状态是一致的。
pub fn trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| is_trace_enabled(std::env::var(TRACE_ENV_VAR).ok().as_deref()))
}

/// 输出一行 trace 到 stderr（带 [`TRACE_PREFIX`] 前缀与毫秒级时间戳）。
///
/// 由 [`trace!`] 宏在"开关已启用"的分支内调用，调用方无需自行判断开关。
/// 写入失败（stderr 被关闭/重定向到已满的管道等）被静默忽略——诊断设施
/// 不应该因为写日志失败而影响被诊断的程序。
pub fn emit(args: std::fmt::Arguments<'_>) {
    // 取一次锁写完整行，避免多行输出在并发写 stderr 时互相穿插。
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{TRACE_PREFIX} {} {args}", timestamp());
}

/// 当前时刻的毫秒级时间戳文本（`HH:MM:SS.mmm`，UTC）。
pub fn timestamp() -> String {
    format_timestamp(millis_since_epoch())
}

/// 当前时刻距 Unix 纪元的毫秒数；系统时钟早于纪元时退化为 0
/// （诊断设施不为不可能发生的时钟异常 panic）。
fn millis_since_epoch() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_millis())
        .unwrap_or(0)
}

/// 纯函数：毫秒时间戳 -> `HH:MM:SS.mmm` 文本（UTC，不做本地时区换算）。
///
/// 这里手工做除法而不用 `chrono`：trace 只需要"同一次运行内部各行之间的
/// 相对先后与间隔"，不需要正确的日期/时区，因此没必要为它引入格式化依赖。
fn format_timestamp(millis_since_epoch: u128) -> String {
    let total_secs = millis_since_epoch / 1000;
    let millis = millis_since_epoch % 1000;
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// 输出一行开发期 trace（`TYPING_TRACE=1`/`true` 时生效，否则为空操作）。
///
/// 参数语法与 `format!`/`println!` 完全一致。**格式化参数只在开关启用时
/// 才被求值**，因此在热路径上写 `{:?}` 打印大结构体也不会有额外开销。
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::trace::trace_enabled() {
            $crate::trace::emit(format_args!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 开关判定（is_trace_enabled / is_flag_enabled）----

    #[test]
    fn trace_is_enabled_for_one() {
        assert!(is_trace_enabled(Some("1")));
    }

    #[test]
    fn trace_is_enabled_for_true_regardless_of_case() {
        assert!(is_trace_enabled(Some("true")));
        assert!(is_trace_enabled(Some("TRUE")));
        assert!(is_trace_enabled(Some("True")));
    }

    #[test]
    fn trace_is_enabled_ignoring_surrounding_whitespace() {
        assert!(is_trace_enabled(Some(" 1 ")));
        assert!(is_trace_enabled(Some("\ttrue\n")));
    }

    #[test]
    fn trace_is_disabled_for_zero_and_other_values() {
        assert!(!is_trace_enabled(Some("0")));
        assert!(!is_trace_enabled(Some("false")));
        assert!(!is_trace_enabled(Some("")));
        // 刻意不做"非空即真"的宽松解析：yes/on 一律视为未启用。
        assert!(!is_trace_enabled(Some("yes")));
        assert!(!is_trace_enabled(Some("on")));
        assert!(!is_trace_enabled(Some("2")));
    }

    #[test]
    fn trace_is_disabled_when_variable_is_unset() {
        assert!(!is_trace_enabled(None));
    }

    #[test]
    fn flag_and_trace_predicates_agree_on_every_sampled_value() {
        // `is_trace_enabled` 只是 `is_flag_enabled` 的语义别名，二者不得
        // 出现行为分叉（开发期开关 TYPING_DEV_AUTOSTART 复用后者）。
        for value in [
            None,
            Some("1"),
            Some("true"),
            Some("TRUE"),
            Some("0"),
            Some(""),
            Some("nope"),
        ] {
            assert_eq!(is_flag_enabled(value), is_trace_enabled(value), "{value:?}");
        }
    }

    // ---- 时间戳格式化 ----

    #[test]
    fn timestamp_is_formatted_as_hms_with_milliseconds() {
        // 1970-01-01T12:34:56.789Z
        let millis = ((12 * 3600 + 34 * 60 + 56) * 1000 + 789) as u128;
        assert_eq!(format_timestamp(millis), "12:34:56.789");
    }

    #[test]
    fn timestamp_pads_all_fields_and_wraps_at_day_boundary() {
        assert_eq!(format_timestamp(0), "00:00:00.000");
        // 恰好跨过一整天：时分秒重新回到 0。
        assert_eq!(format_timestamp(24 * 3600 * 1000), "00:00:00.000");
        assert_eq!(format_timestamp(24 * 3600 * 1000 + 61_005), "00:01:01.005");
    }

    #[test]
    fn trace_macro_is_usable_and_does_not_panic() {
        // 不断言输出内容（stderr 不便在单测里捕获），只保证宏能展开、
        // 在开关关闭/开启两种状态下都不 panic。
        crate::trace!("trace 宏自检：{} {:?}", 1, Some("x"));
    }
}
