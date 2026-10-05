//! 按键音效：为每一次击键提供听觉反馈，补足"敲击感"。
//!
//! ## 为什么自己合成波形而不是播放音频文件
//!
//! 击键音只有几十毫秒，用几个振荡器加噪声就能合成出来。这样做省掉了三件麻烦
//! 事：不需要往仓库里放二进制音频资源、不需要引入音频解码器（`rodio` 之类为了
//! 解码 wav/ogg/mp3 会带进上百个依赖）、也不需要在运行时定位资源文件路径
//! （打包/移动应用目录都可能让路径失效）。
//!
//! 因此依赖只有 [`tinyaudio`]——一个只做"打开输出设备并周期性回调填充采样"
//! 这一件事的薄封装（在 macOS 上仅链接 CoreAudio，无其他传递依赖）。
//!
//! ## 为什么预渲染成波表，而不是在回调里逐采样求值
//!
//! 机械键盘的敲击声主体是**宽带瞬态**（噪声）经带通滤波后的短促"咔"，而带通
//! 滤波器是递归的（当前输出依赖前若干个采样的状态），无法写成"给定采样下标即
//! 可独立求值"的闭式表达式。因此这里改为：启动时把每种音效**一次性渲染**成一段
//! 波表（约 60ms，≈10KB），音频回调只按下标读取。
//!
//! 这样做同时更符合实时性要求：回调里只剩一次数组读取，没有 `exp`/`sin`/滤波
//! 运算。渲染函数 [`render_click`] 则是确定性纯函数（噪声取自固定种子的
//! xorshift），因此波形的所有性质都能在单测里断言。
//!
//! ## 实时性约束
//!
//! `tinyaudio` 的数据回调运行在音频线程上：它必须在一个缓冲区时长内返回，
//! 否则会产生爆音（xrun）。因此回调里**不加锁、不分配内存、不做 I/O**——
//! UI 线程与音频线程之间只通过两个 [`AtomicU64`] 计数器通信（每种音效一个），
//! UI 线程自增，音频线程比较自己已服务的计数值来决定是否起音。
//!
//! ## 失败与静音
//!
//! 设备打开失败（无音频输出设备、被独占、权限受限等）不是错误：
//! [`KeyClickPlayer::new`] 返回 `None`，调用方照常运行，只是没有声音。
//! 另外 `TYPING_MUTE=1` 可以显式关掉音效（家长在安静场合使用）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tinyaudio::{OutputDevice, OutputDeviceParameters, run_output_device};

/// 关闭按键音效的环境变量名。取值判定复用 [`crate::trace::is_flag_enabled`]
/// （`1`/`true` 为真，其余一律为假）。
pub const MUTE_ENV_VAR: &str = "TYPING_MUTE";

/// 输出采样率。44.1kHz 是最通用的取值，避免设备端做重采样。
const SAMPLE_RATE: f32 = 44_100.0;

/// 每次回调渲染的采样数。256 帧 ≈ 5.8ms，在"延迟足够低（敲下去就响）"与
/// "回调不至于过于频繁"之间取平衡——击键反馈对延迟敏感，几十毫秒的滞后就会
/// 让人觉得声音"跟不上手"。
const BUFFER_FRAMES: usize = 256;

/// 音效种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickSound {
    /// 输入正确：机械键盘的敲击声。
    Correct,
    /// 输入错误：沉闷的低音，没有明亮的咔哒瞬态。
    ///
    /// 刻意不做成刺耳的警报声：Req 7.2 要求错误反馈不带负面评价，听觉反馈同理，
    /// 目的是让孩子察觉"这一下不对"，而不是惩罚他。
    Error,
}

impl ClickSound {
    /// 该音效在原子计数器数组中的下标。
    const fn slot(self) -> usize {
        match self {
            ClickSound::Correct => 0,
            ClickSound::Error => 1,
        }
    }

    /// 波表总时长（秒）。
    const fn duration_secs(self) -> f32 {
        match self {
            // 一次完整的机械键盘击键声：咔哒瞬态 + 触底闷响的余韵。
            ClickSound::Correct => 0.060,
            ClickSound::Error => 0.090,
        }
    }

    /// 归一化后的峰值音量（0.0-1.0）。取值偏保守，避免突然的响声惊到孩子。
    const fn amplitude(self) -> f32 {
        match self {
            ClickSound::Correct => 0.24,
            ClickSound::Error => 0.17,
        }
    }

    /// 波表长度（采样数）。
    fn sample_count(self) -> usize {
        (self.duration_secs() * SAMPLE_RATE) as usize
    }
}

/// 确定性伪随机噪声源（xorshift64*）。
///
/// 用固定种子而不是 `rand`：一是音效波形不需要每次运行都不同（反而希望完全
/// 一致，便于测试与回归对比），二是渲染发生在启动路径上，没必要为此引入随机源。
struct Noise(u64);

impl Noise {
    fn new(seed: u64) -> Self {
        // 种子不能为 0，否则 xorshift 会永远输出 0。
        Self(seed | 1)
    }

    /// 下一个 `[-1.0, 1.0)` 区间内的样本。
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let bits = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // 取高 24 位映射到 [-1, 1)。
        ((bits >> 40) as f32 / (1u32 << 23) as f32) - 1.0
    }
}

/// 二阶带通滤波器（RBJ biquad，constant skirt gain 形式）。
///
/// 机械键盘的"咔"本质是被机械结构染色过的宽带噪声：直接用白噪声听起来像"嘶"，
/// 必须把能量收窄到某个频段附近才会像"咔哒"。这就是需要滤波器、进而需要预渲染
/// 波表（滤波器有状态、无法按下标独立求值）的原因。
struct BandPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BandPass {
    fn new(freq_hz: f32, q: f32) -> Self {
        let w0 = std::f32::consts::TAU * freq_hz / SAMPLE_RATE;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);

        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x0: f32) -> f32 {
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        y0
    }
}

/// 纯函数：渲染一种音效的完整波表。
///
/// 相同输入恒产出完全相同的波表（噪声取自固定种子）。
///
/// ## 正确音的构成（模拟机械键盘轴体）
///
/// 真实的机械键盘敲击声不是单一事件，而是若干个错开几毫秒的短瞬态叠在一个
/// 低频"触底闷响"上。这里按同样的结构合成四层：
///
/// 1. **按下的咔哒**（t=0）：3.1kHz 带通噪声，3.5ms 极快衰减——声音的"脆"来自
///    这一层；
/// 2. **触底的第二次瞬态**（t=8ms）：4.6kHz 带通噪声，2.5ms 衰减。两次瞬态错开
///    几毫秒是"机械感"的关键：单个瞬态听起来像鼠标点击，两个才像键盘；
/// 3. **触底闷响**（t=0）：165Hz 阻尼正弦（含二次谐波），28ms 衰减，提供重量感；
/// 4. **外壳共鸣**（t=0）：880Hz 阻尼正弦，12ms 衰减，补上塑料壳的"空腔"色彩。
///
/// ## 错误音的构成
///
/// 只保留低频闷响（220Hz + 110Hz 阻尼正弦）与一层很暗的 400Hz 带通噪声，
/// **没有明亮瞬态**——因此与正确音在听觉上一眼可辨，却不刺耳。
pub fn render_click(sound: ClickSound) -> Vec<f32> {
    let total = sound.sample_count();
    let mut buffer = vec![0.0f32; total];

    // 每种音效用不同的噪声种子，避免两种音听起来像同一段噪声换了包络。
    let mut noise = Noise::new(match sound {
        ClickSound::Correct => 0x9E37_79B9_7F4A_7C15,
        ClickSound::Error => 0xBF58_476D_1CE4_E5B9,
    });

    match sound {
        ClickSound::Correct => {
            let mut click_bp = BandPass::new(3_100.0, 1.1);
            let mut tick_bp = BandPass::new(4_600.0, 1.6);
            // 第二次瞬态相对第一次的延迟：8ms。
            let tick_delay = (0.008 * SAMPLE_RATE) as usize;

            for (index, out) in buffer.iter_mut().enumerate() {
                let t = index as f32 / SAMPLE_RATE;
                let raw = noise.next();

                // 1. 按下的咔哒
                //
                // 乘 5.0 是为了补偿带通滤波器的通带增益：constant-skirt 形式的
                // biquad 在 3.1kHz/Q=1.1 下输出只有输入的约 0.16 倍，不补偿的话
                // 咔哒声会被下面的低频触底完全盖住——实测谱质心只有 433Hz，
                // 听感是"闷响"而不是"机械键盘"。
                let click = click_bp.process(raw) * (-t / 0.0045).exp() * 5.0;

                // 2. 触底的第二次瞬态（延迟起振；滤波器仍需持续推进以保持状态连续）
                let tick_input = if index >= tick_delay { raw } else { 0.0 };
                let tick_env = if index >= tick_delay {
                    let t2 = (index - tick_delay) as f32 / SAMPLE_RATE;
                    (-t2 / 0.0025).exp()
                } else {
                    0.0
                };
                let tick = tick_bp.process(tick_input) * tick_env * 3.5;

                // 3. 触底闷响
                let body_phase = std::f32::consts::TAU * 165.0 * t;
                // 触底闷响提供重量感，但必须让位于瞬态：增益压到 0.22、衰减
                // 缩短到 20ms，否则它的能量积分（幅度² × 时长）会压过只持续几
                // 毫秒的咔哒声。
                let body =
                    (body_phase.sin() + 0.35 * (2.0 * body_phase).sin()) * (-t / 0.020).exp() * 0.22;

                // 4. 外壳共鸣
                let shell =
                    (std::f32::consts::TAU * 880.0 * t).sin() * (-t / 0.010).exp() * 0.10;

                *out = click + tick + body + shell;
            }
        }
        ClickSound::Error => {
            let mut thud_bp = BandPass::new(400.0, 0.9);

            for (index, out) in buffer.iter_mut().enumerate() {
                let t = index as f32 / SAMPLE_RATE;
                let raw = noise.next();

                let thud = thud_bp.process(raw) * (-t / 0.016).exp() * 0.8;
                let low_phase = std::f32::consts::TAU * 220.0 * t;
                let sub_phase = std::f32::consts::TAU * 110.0 * t;
                // 时间常数取 28ms：波表 90ms 长，到末尾时包络已衰减到
                // exp(-90/28) ≈ 4%，配合收尾淡出即完全静音。若时间常数取得过大
                // （比如 55ms），声音在波表结束时仍有可听残留，抢占播放下一次
                // 击键时会产生"咔"的截断感。
                let tone = (low_phase.sin() * 0.6 + sub_phase.sin() * 0.4) * (-t / 0.028).exp();

                *out = thud + tone;
            }
        }
    }

    // 起振淡入（0.5ms）：直接从 0 跳到瞬态峰值会额外产生一个"啪"的直流突变。
    let fade_in = (0.0005 * SAMPLE_RATE) as usize;
    for (index, value) in buffer.iter_mut().enumerate().take(fade_in.min(total)) {
        *value *= index as f32 / fade_in as f32;
    }

    // 收尾淡出（3ms）：波表末尾若非零，循环读取的边界处会有可听的断点。
    let fade_out = (0.003 * SAMPLE_RATE) as usize;
    for offset in 0..fade_out.min(total) {
        let index = total - 1 - offset;
        buffer[index] *= offset as f32 / fade_out as f32;
    }

    // 归一化到目标峰值：各层叠加后的峰值不可预知，靠归一化而不是手调增益来
    // 保证既不削波、又能达到设定音量。
    let peak = buffer.iter().fold(0.0f32, |acc, &value| acc.max(value.abs()));
    if peak > 0.0 {
        let gain = sound.amplitude() / peak;
        for value in buffer.iter_mut() {
            *value *= gain;
        }
    }

    buffer
}

/// 两种音效的波表。启动时渲染一次，之后只被音频线程按下标读取。
struct ClickTables {
    correct: Vec<f32>,
    error: Vec<f32>,
}

impl ClickTables {
    fn new() -> Self {
        Self {
            correct: render_click(ClickSound::Correct),
            error: render_click(ClickSound::Error),
        }
    }

    fn table(&self, sound: ClickSound) -> &[f32] {
        match sound {
            ClickSound::Correct => &self.correct,
            ClickSound::Error => &self.error,
        }
    }
}

/// 按键音效播放器。持有输出设备句柄，drop 时自动关闭设备。
pub struct KeyClickPlayer {
    /// 每种音效的"请求次数"计数器，由 UI 线程自增、音频线程读取。
    requests: Arc<[AtomicU64; 2]>,
    /// 输出设备句柄。必须持有：一旦 drop，音频回调就停止。
    _device: OutputDevice,
}

impl KeyClickPlayer {
    /// 打开音频输出设备并启动渲染回调。
    ///
    /// 返回 `None` 的情况：`TYPING_MUTE=1` 显式静音，或设备打开失败（无输出
    /// 设备/被占用/权限受限）。两种情况调用方都应静默继续——没有声音不影响
    /// 打字练习的任何功能。
    pub fn new() -> Option<Self> {
        if crate::trace::is_flag_enabled(std::env::var(MUTE_ENV_VAR).ok().as_deref()) {
            crate::trace!("audio: {MUTE_ENV_VAR} 已设置，跳过音频设备初始化（静音运行）");
            return None;
        }

        let requests: Arc<[AtomicU64; 2]> = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);
        let render_requests = requests.clone();
        // 波表在这里（UI 线程、启动路径上）一次性渲染完成，音频回调只读不算。
        let tables = ClickTables::new();

        // 音频线程的私有状态（不与 UI 线程共享，故无需同步原语）：
        // - `served`：每种音效已经起音过的请求次数；
        // - `voice`：当前正在发声的音效与它在波表中的读取位置。同一时刻只保留
        //   一个 voice，新的击键会直接抢占——击键音很短，重叠播放只会互相糊掉。
        let mut served = [0u64; 2];
        let mut voice: Option<(ClickSound, usize)> = None;

        let device = run_output_device(
            OutputDeviceParameters {
                sample_rate: SAMPLE_RATE as usize,
                channels_count: 1,
                channel_sample_count: BUFFER_FRAMES,
            },
            move |data: &mut [f32]| {
                for sound in [ClickSound::Correct, ClickSound::Error] {
                    let slot = sound.slot();
                    let requested = render_requests[slot].load(Ordering::Relaxed);
                    if requested != served[slot] {
                        served[slot] = requested;
                        voice = Some((sound, 0));
                    }
                }

                for frame in data.iter_mut() {
                    *frame = match voice {
                        Some((sound, index)) => {
                            let table = tables.table(sound);
                            let value = table.get(index).copied().unwrap_or(0.0);
                            let next = index + 1;
                            voice = if next >= table.len() {
                                None
                            } else {
                                Some((sound, next))
                            };
                            value
                        }
                        None => 0.0,
                    };
                }
            },
        )
        .map_err(|err| {
            crate::trace!("audio: 打开输出设备失败，本次运行静音：{err}");
            err
        })
        .ok()?;

        crate::trace!(
            "audio: 输出设备已启动（{}Hz，{BUFFER_FRAMES} 帧/回调）",
            SAMPLE_RATE as usize
        );
        Some(Self {
            requests,
            _device: device,
        })
    }

    /// 请求播放一次音效。
    ///
    /// 只做一次原子自增，不阻塞、不分配——可以安全地在 UI 回调里逐键调用。
    /// 实际起音发生在下一次音频回调（≈6ms 内）。
    pub fn play(&self, sound: ClickSound) {
        self.requests[sound.slot()].fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOUNDS: [ClickSound; 2] = [ClickSound::Correct, ClickSound::Error];

    /// 平均相邻采样差（近似"明亮度"/高频含量）。机械敲击声的高频瞬态会让这个
    /// 值明显高于低频闷响。
    fn brightness(buffer: &[f32]) -> f32 {
        let energy: f32 = buffer.iter().map(|v| v.abs()).sum();
        if energy == 0.0 {
            return 0.0;
        }
        let diff: f32 = buffer.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        diff / energy
    }

    fn peak(buffer: &[f32]) -> f32 {
        buffer.iter().fold(0.0f32, |acc, &v| acc.max(v.abs()))
    }

    #[test]
    fn each_sound_has_a_distinct_slot() {
        assert_ne!(ClickSound::Correct.slot(), ClickSound::Error.slot());
        assert!(ClickSound::Correct.slot() < 2 && ClickSound::Error.slot() < 2);
    }

    /// 波表必须可复现：固定种子的噪声保证每次运行、每台机器上的音色完全一致。
    #[test]
    fn rendering_is_deterministic() {
        for sound in SOUNDS {
            assert_eq!(render_click(sound), render_click(sound), "{sound:?} 波表不可复现");
        }
    }

    /// 归一化后峰值必须等于设定音量，且不得削波。
    #[test]
    fn peak_matches_configured_amplitude_and_never_clips() {
        for sound in SOUNDS {
            let buffer = render_click(sound);
            let measured = peak(&buffer);
            assert!(
                (measured - sound.amplitude()).abs() < 1e-3,
                "{sound:?} 峰值 {measured} 与设定音量 {} 不符",
                sound.amplitude()
            );
            for (index, &value) in buffer.iter().enumerate() {
                assert!(value.is_finite(), "{sound:?} 第 {index} 个采样非有限值");
                assert!(
                    (-1.0..=1.0).contains(&value),
                    "{sound:?} 在第 {index} 个采样处削波：{value}"
                );
            }
        }
    }

    /// 必须是"敲击"而不是"渐起"：峰值要出现在整段波形的前 15% 内。
    #[test]
    fn attack_is_immediate() {
        for sound in SOUNDS {
            let buffer = render_click(sound);
            let peak_index = buffer
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(index, _)| index)
                .expect("波表不应为空");
            let limit = buffer.len() * 15 / 100;
            assert!(
                peak_index <= limit,
                "{sound:?} 的峰值出现在第 {peak_index} 个采样（上限 {limit}）：起振太慢，不像敲击"
            );
        }
    }

    /// 包络必须衰减到基本静音，否则连续打字时会互相拖尾。
    #[test]
    fn envelope_decays_to_near_silence() {
        for sound in SOUNDS {
            let buffer = render_click(sound);
            let head = peak(&buffer[..buffer.len() / 10]);
            let tail = peak(&buffer[buffer.len() * 9 / 10..]);
            assert!(
                tail < head * 0.1,
                "{sound:?} 衰减不足：首段峰值 {head}，末段峰值 {tail}"
            );
        }
    }

    /// 波表两端必须归零：起振处避免直流突变的"啪"声，收尾处避免可听的断点。
    #[test]
    fn buffer_starts_and_ends_at_silence() {
        for sound in SOUNDS {
            let buffer = render_click(sound);
            assert_eq!(buffer[0], 0.0, "{sound:?} 起点不为 0");
            assert_eq!(buffer[buffer.len() - 1], 0.0, "{sound:?} 终点不为 0");
        }
    }

    /// 正确音是机械敲击声：既要明显比错误音明亮，也要达到一个**绝对**的明亮度
    /// 下限。
    ///
    /// 为什么两条都要：只做"比错误音亮"的相对判断是不够的——本文件曾有一个版本
    /// 各层配比失衡，低频触底把高频瞬态完全盖住（实测谱质心只有 433Hz，听感是
    /// 闷响而不是咔哒），但它照样通过了相对判断，因为错误音更闷。绝对下限才能
    /// 守住"听起来像机械键盘"这件事。
    ///
    /// 明亮度用平均相邻采样差／平均幅度近似（高频含量的廉价代理）。当前配比下
    /// 实测：正确音 0.61（谱质心 1775Hz），错误音 0.04（谱质心 265Hz）。
    #[test]
    fn correct_sound_is_a_bright_mechanical_click() {
        let correct = brightness(&render_click(ClickSound::Correct));
        let error = brightness(&render_click(ClickSound::Error));

        assert!(
            correct > 0.30,
            "正确音明亮度 {correct} 低于绝对下限 0.30：高频瞬态被低频触底盖住了，\
             听感会是闷响而不是机械键盘的咔哒声"
        );
        assert!(
            correct > error * 5.0,
            "正确音明亮度 {correct} 未显著高于错误音 {error}：两者在听觉上不易区分"
        );
    }

    /// 机械敲击声必须有两个错开的瞬态（按下 + 触底）——只有一个的话听起来像
    /// 鼠标点击而不是键盘。用"前 20ms 内出现两个局部能量峰"来近似判定。
    #[test]
    fn mechanical_click_has_two_transients() {
        let buffer = render_click(ClickSound::Correct);
        // 以 1ms 为窗计算能量包络。
        let window = (0.001 * SAMPLE_RATE) as usize;
        let envelope: Vec<f32> = buffer
            .chunks(window)
            .map(|chunk| chunk.iter().fold(0.0f32, |acc, &v| acc.max(v.abs())))
            .collect();

        // 统计"比前后两个窗都高"的局部极大值（前 20 个窗 = 前 20ms）。
        let peaks = (1..envelope.len().min(20) - 1)
            .filter(|&i| envelope[i] > envelope[i - 1] && envelope[i] > envelope[i + 1])
            .count();
        assert!(
            peaks >= 2,
            "前 20ms 内只找到 {peaks} 个瞬态峰，机械感来自按下与触底两次瞬态"
        );
    }

    /// 击键音必须足够短：超过约 400ms 就会在连续打字时互相拖尾。
    #[test]
    fn sounds_are_short_enough_for_continuous_typing() {
        for sound in SOUNDS {
            let secs = sound.duration_secs();
            assert!(secs <= 0.4, "{sound:?} 时长 {secs}s 过长，连续打字时会互相拖尾");
        }
    }

    /// 错误音应当更沉、更长、更轻——这是它与机械敲击声的区分依据。
    #[test]
    fn error_sound_is_longer_and_quieter_than_correct() {
        assert!(ClickSound::Error.duration_secs() > ClickSound::Correct.duration_secs());
        assert!(ClickSound::Error.amplitude() < ClickSound::Correct.amplitude());
    }
}
