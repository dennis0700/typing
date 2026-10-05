//! 正确率 / WPM 计算。

use std::time::Duration;

/// 纯函数：打字速度 WPM = (总输入字符数 / 5) / 用时(分钟)，保留 1 位小数。
///
/// - `total_inputs`：本次练习环节的总输入字符数。
/// - `duration`：本次练习所用时间。
///
/// 结果恒为非负值（`total_inputs` 与 `duration` 均不可能为负）。
///
/// 边界情况：当 `duration` 为零时（例如练习在同一时刻开始与结束的极端场景），
/// 为避免除以零，函数返回 `0.0`，而不是 `NaN` 或 `Infinity`。
///
/// **Validates: Requirements 4.2**
pub fn calc_wpm(total_inputs: u32, duration: Duration) -> f32 {
    let duration_minutes = duration.as_secs_f64() / 60.0;
    if duration_minutes <= 0.0 {
        return 0.0;
    }

    let wpm = (total_inputs as f64 / 5.0) / duration_minutes;
    (round_to_1_decimal(wpm) as f32).max(0.0)
}

/// 将 `f64` 四舍五入保留 1 位小数。
fn round_to_1_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// 纯函数：计算正确率。
///
/// 正确率 = 正确输入字符数 / 总输入字符数 * 100，保留 1 位小数，
/// 取值范围恒为 `[0.0, 100.0]`（Requirements 4.1）。
///
/// 当 `total_inputs` 为 0（尚未产生任何输入）时，定义正确率为 `0.0`，
/// 避免除以零。
pub fn calc_accuracy(correct_inputs: u32, total_inputs: u32) -> f32 {
    if total_inputs == 0 {
        return 0.0;
    }

    let raw = correct_inputs as f32 / total_inputs as f32 * 100.0;
    let rounded = (raw * 10.0).round() / 10.0;
    rounded.clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn zero_total_inputs_returns_zero() {
        assert_eq!(calc_accuracy(0, 0), 0.0);
    }

    #[test]
    fn all_correct_returns_one_hundred() {
        assert_eq!(calc_accuracy(10, 10), 100.0);
    }

    #[test]
    fn no_correct_returns_zero() {
        assert_eq!(calc_accuracy(0, 10), 0.0);
    }

    #[test]
    fn rounds_to_one_decimal_place() {
        // 1/3 * 100 = 33.333... -> 33.3
        assert_eq!(calc_accuracy(1, 3), 33.3);
        // 2/3 * 100 = 66.666... -> 66.7
        assert_eq!(calc_accuracy(2, 3), 66.7);
    }

    #[test]
    fn zero_duration_returns_zero() {
        assert_eq!(calc_wpm(100, Duration::from_secs(0)), 0.0);
    }

    #[test]
    fn one_minute_duration_matches_formula() {
        // (150 / 5) / 1 = 30.0 WPM
        assert_eq!(calc_wpm(150, Duration::from_secs(60)), 30.0);
    }

    #[test]
    fn zero_total_inputs_returns_zero_wpm() {
        assert_eq!(calc_wpm(0, Duration::from_secs(60)), 0.0);
    }

    #[test]
    fn wpm_rounds_to_one_decimal_place() {
        // (10 / 5) / (30/60) = 2 / 0.5 = 4.0
        assert_eq!(calc_wpm(10, Duration::from_secs(30)), 4.0);
        // (100 / 5) / (37/60) = 20 / 0.61666... = 32.432... -> 32.4
        assert_eq!(calc_wpm(100, Duration::from_secs(37)), 32.4);
    }

    // Feature: typing-desktop-app, Property 8: 正确率与打字速度计算公式
    //
    // 对于任意非负整数 correct_inputs <= total_inputs 与任意正的练习用时 duration：
    // calc_accuracy(correct_inputs, total_inputs) 恒等于
    // round(correct_inputs / total_inputs * 100, 1) 且落在 [0.0, 100.0] 区间；
    // calc_wpm(total_inputs, duration) 恒等于
    // round((total_inputs / 5) / duration_minutes, 1) 且为非负值。
    //
    // Validates: Requirements 4.1, 4.2
    proptest! {
        #[test]
        fn prop_accuracy_and_wpm_match_formula(
            total_inputs in 0u32..=1_000_000,
            correct_ratio in 0.0f64..=1.0,
            duration_millis in 0u64..=3_600_000u64,
        ) {
            // 由 total_inputs 与一个 [0.0, 1.0] 的比例派生出 correct_inputs，
            // 从而保证 correct_inputs <= total_inputs 恒成立，同时覆盖 0/边界/中间值。
            let correct_inputs = ((total_inputs as f64) * correct_ratio).round() as u32;
            let correct_inputs = correct_inputs.min(total_inputs);
            let duration = Duration::from_millis(duration_millis);

            // --- calc_accuracy ---
            let accuracy = calc_accuracy(correct_inputs, total_inputs);

            prop_assert!((0.0..=100.0).contains(&accuracy));

            // 正确率的性质断言刻意**不**照抄实现内部的浮点运算过程。
            // `calc_accuracy` 用 f32 计算，若这里用 f64 重算一遍再逐位比较，
            // 遇到"恰好落在四舍五入分界上"的输入时两条精度路径会给出相反的
            // 进位方向：proptest 找到的 total_inputs=80 / correct_inputs=23
            // 就是这种情况——f32 下 23/80*100 == 28.75，进位成 28.8；f64 下
            // 同一表达式是 28.749999999999996，退位成 28.7。这不是实现的
            // 缺陷，而是"用另一种精度重算的预期值"这个 oracle 本身不可靠。
            //
            // 因此改为直接断言需求本身（Req 4.1："保留 1 位小数"）：
            // 结果与精确比值的偏差不超过半个刻度，且结果本身是 0.1 的整数倍。
            // 这两条对任何合法的四舍五入实现都成立，与进位方向无关。
            if total_inputs == 0 {
                prop_assert_eq!(accuracy, 0.0);
            } else {
                let exact = correct_inputs as f64 / total_inputs as f64 * 100.0;
                // 1e-4 的余量用于吸收 f32 表示误差（accuracy <= 100.0，f32
                // 在该量级的表示误差约 1e-5）。
                prop_assert!(
                    (accuracy as f64 - exact).abs() <= 0.05 + 1e-4,
                    "accuracy {} 与精确比值 {} 的偏差超过半个刻度（0.05）",
                    accuracy,
                    exact
                );
                let scaled = accuracy as f64 * 10.0;
                prop_assert!(
                    (scaled - scaled.round()).abs() < 1e-3,
                    "accuracy {} 不是 1 位小数",
                    accuracy
                );
            }

            // --- calc_wpm ---
            let wpm = calc_wpm(total_inputs, duration);

            prop_assert!(wpm >= 0.0);

            let duration_minutes = duration.as_secs_f64() / 60.0;
            let expected_wpm = if duration_minutes <= 0.0 {
                0.0
            } else {
                let raw = (total_inputs as f64 / 5.0) / duration_minutes;
                (round_to_1_decimal(raw) as f32).max(0.0)
            };
            prop_assert!(
                (wpm - expected_wpm).abs() < 1e-3,
                "wpm {} did not match expected {}",
                wpm,
                expected_wpm
            );
        }
    }
}

use crate::domain::curriculum::{LessonId, PracticeResult};

/// 本次成绩与历史最佳成绩的比较结果（Req 4.4, 4.5）。
///
/// - `best` 为空时（该课程尚不存在历史最佳成绩）返回 [`ComparisonResult::FirstRecord`]，
///   不进行数值比较。
/// - `best` 存在时，依据 `current` 相对于 `best` 的整体表现返回 `Higher`/`Lower`/`Equal`，
///   并在 `Higher`/`Lower` 分支中携带正确率与打字速度各自的数值差值
///   （`accuracy_delta`/`wpm_delta` 恒等于 `current` 与 `best` 对应字段的差，符号已经
///   体现方向：`Higher` 时为正或零，`Lower` 时为负或零）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComparisonResult {
    /// 该课程尚不存在历史最佳成绩，本次为首次记录，不进行比较。
    FirstRecord,
    /// 本次成绩优于历史最佳成绩。
    Higher { accuracy_delta: f32, wpm_delta: f32 },
    /// 本次成绩劣于历史最佳成绩。
    Lower { accuracy_delta: f32, wpm_delta: f32 },
    /// 本次成绩与历史最佳成绩完全相等。
    Equal,
}

/// 纯函数：将本次成绩与历史最佳成绩比较（Req 4.4, 4.5）。
///
/// - 若 `best` 为 `None`（该课程尚不存在历史最佳成绩），返回
///   [`ComparisonResult::FirstRecord`]，不进行数值比较（Req 4.5）。
/// - 若 `best` 存在，比较 `current` 与 `best` 的正确率、打字速度：
///   - 两者在正确率与打字速度上都完全相等时，返回 [`ComparisonResult::Equal`]；
///   - 否则，依据"正确率优先，相等时比较打字速度"的整体优劣判定返回
///     `Higher`/`Lower`，并携带 `accuracy_delta = current.accuracy - best.accuracy`、
///     `wpm_delta = current.wpm - best.wpm`（Req 4.4）。
pub fn compare_with_best(
    current: &PracticeResult,
    best: Option<&PracticeResult>,
) -> ComparisonResult {
    let Some(best) = best else {
        return ComparisonResult::FirstRecord;
    };

    let accuracy_delta = current.accuracy - best.accuracy;
    let wpm_delta = current.wpm - best.wpm;

    if accuracy_delta == 0.0 && wpm_delta == 0.0 {
        return ComparisonResult::Equal;
    }

    // 整体优劣判定：正确率优先，正确率相等时以打字速度决定方向。
    let is_higher = if accuracy_delta != 0.0 {
        accuracy_delta > 0.0
    } else {
        wpm_delta > 0.0
    };

    if is_higher {
        ComparisonResult::Higher {
            accuracy_delta,
            wpm_delta,
        }
    } else {
        ComparisonResult::Lower {
            accuracy_delta,
            wpm_delta,
        }
    }
}

/// 同一练习题目内连续输入错误达到阈值时触发的反馈命令（Req 7.2, 7.6）。
///
/// - 连续错误次数达到 3 次时触发 [`ErrorThresholdCommand::ShowEncouragement`]，
///   展示不含负面评价词汇的鼓励性提示，允许学员继续尝试或跳过该题目（Req 7.2）。
/// - 连续错误次数达到 5 次时触发 [`ErrorThresholdCommand::ShowCorrectAnswer`]，
///   展示正确答案或提示，允许学员选择继续下一题目（Req 7.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorThresholdCommand {
    /// 展示鼓励性提示，允许学员继续尝试或跳过当前题目。
    ShowEncouragement,
    /// 展示正确答案或提示，允许学员继续下一题目。
    ShowCorrectAnswer,
}

/// 连续错误触发鼓励提示所需的最小次数（Req 7.2）。
pub const ENCOURAGEMENT_THRESHOLD: u32 = 3;

/// 连续错误触发展示正确答案所需的最小次数（Req 7.6）。
pub const SHOW_ANSWER_THRESHOLD: u32 = 5;

/// 纯函数：依据同一练习题目内的连续错误次数，判定应触发的反馈命令（Req 7.2, 7.6）。
///
/// - `consecutive_errors < 3`：不触发任何提示命令，返回 `None`。
/// - `3 <= consecutive_errors < 5`：返回
///   [`ErrorThresholdCommand::ShowEncouragement`]。
/// - `consecutive_errors >= 5`：返回
///   [`ErrorThresholdCommand::ShowCorrectAnswer`]。
///
/// 使用 `>=` 而非 `==` 判定，确保调用方即使跳过某个中间计数（或在阈值以上的
/// 任意次数重复调用）也能得到稳定一致的触发结果。
pub fn check_error_threshold(consecutive_errors: u32) -> Option<ErrorThresholdCommand> {
    if consecutive_errors >= SHOW_ANSWER_THRESHOLD {
        Some(ErrorThresholdCommand::ShowCorrectAnswer)
    } else if consecutive_errors >= ENCOURAGEMENT_THRESHOLD {
        Some(ErrorThresholdCommand::ShowEncouragement)
    } else {
        None
    }
}

/// 鼓励文案候选池（Req 7.2）。
///
/// 池中每一条文案均不包含 [`NEGATIVE_WORD_BLACKLIST`] 中定义的负面评价词汇，
/// 该约束由 [`validate_encouragement_pool`] 校验，并在单元测试中断言成立。
pub const ENCOURAGEMENT_POOL: &[&str] = &[
    "再试一次，你可以的！",
    "别着急，慢慢来，你会越来越棒！",
    "继续加油，你已经很努力了！",
    "没关系，多练习几次就熟悉了！",
    "你做得很好，再来一次试试！",
    "坚持一下，胜利就在眼前！",
];

/// 负面评价词汇黑名单（Req 7.2）。
///
/// 鼓励文案候选池中的任意文案都不应包含此列表中的词汇，以确保展示给学员的
/// 提示信息始终是正向、鼓励性的，不包含负面评价。
///
/// 运行时没有调用方：候选池 `ENCOURAGEMENT_POOL` 是编译期写死的常量，其
/// 合规性由 `validate_encouragement_pool` 的单测/属性测试在 `cargo test`
/// 时静态验证，不需要在应用启动时再校验一遍。黑名单与校验函数因此是
/// "把 Req 7.2 变成可自动验证的断言"的载体，保留而不删除。
#[cfg_attr(not(test), allow(dead_code))]
pub const NEGATIVE_WORD_BLACKLIST: &[&str] = &[
    "错", "笨", "差", "失败", "糟糕", "不行", "蠢", "废物", "垃圾", "弱智",
];

/// 纯函数：校验鼓励文案候选池是否不含负面评价词汇黑名单中的任意词汇（Req 7.2）。
///
/// 返回 `Ok(())` 表示整个候选池校验通过；否则返回 `Err`，其中包含每一条
/// 命中黑名单的 `(文案, 命中的黑名单词汇)` 组合，便于定位具体问题文案。
///
/// 只在测试中调用，原因见 [`NEGATIVE_WORD_BLACKLIST`]。
#[cfg_attr(not(test), allow(dead_code))]
pub fn validate_encouragement_pool(pool: &[&str]) -> Result<(), Vec<(String, String)>> {
    let mut violations = Vec::new();

    for phrase in pool {
        for banned_word in NEGATIVE_WORD_BLACKLIST {
            if phrase.contains(banned_word) {
                violations.push((phrase.to_string(), banned_word.to_string()));
            }
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

#[cfg(test)]
mod error_threshold_tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn below_threshold_produces_no_command() {
        assert_eq!(check_error_threshold(0), None);
        assert_eq!(check_error_threshold(1), None);
        assert_eq!(check_error_threshold(2), None);
    }

    #[test]
    fn reaching_three_triggers_encouragement() {
        assert_eq!(
            check_error_threshold(3),
            Some(ErrorThresholdCommand::ShowEncouragement)
        );
        assert_eq!(
            check_error_threshold(4),
            Some(ErrorThresholdCommand::ShowEncouragement)
        );
    }

    #[test]
    fn reaching_five_triggers_show_correct_answer() {
        assert_eq!(
            check_error_threshold(5),
            Some(ErrorThresholdCommand::ShowCorrectAnswer)
        );
    }

    #[test]
    fn beyond_five_keeps_triggering_show_correct_answer() {
        assert_eq!(
            check_error_threshold(6),
            Some(ErrorThresholdCommand::ShowCorrectAnswer)
        );
        assert_eq!(
            check_error_threshold(100),
            Some(ErrorThresholdCommand::ShowCorrectAnswer)
        );
    }

    #[test]
    fn encouragement_pool_is_not_empty() {
        assert!(!ENCOURAGEMENT_POOL.is_empty());
    }

    #[test]
    fn encouragement_pool_passes_blacklist_validation() {
        assert_eq!(validate_encouragement_pool(ENCOURAGEMENT_POOL), Ok(()));
    }

    #[test]
    fn validation_detects_blacklisted_word() {
        let bad_pool = ["你怎么这么笨，再试一次吧"];
        let result = validate_encouragement_pool(&bad_pool);
        assert!(result.is_err());
        let violations = result.unwrap_err();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].0, "你怎么这么笨，再试一次吧");
        assert_eq!(violations[0].1, "笨");
    }

    // Feature: typing-desktop-app, Property 17: 错误计数阈值触发行为
    //
    // 对于任意同一练习题目内的连续错误输入次数 n：当 n 首次达到 3 时，产生
    // "鼓励性提示"命令；当 n 首次达到 5 时，产生"展示正确答案"命令；n 小于 3
    // 时不产生上述任一提示命令。鼓励文案候选池中的每一条文案均不包含预定义
    // 的负面评价词汇黑名单中的任意词汇。
    //
    // Validates: Requirements 7.2, 7.6
    proptest! {
        #[test]
        fn prop_error_threshold_triggers_and_pool_is_clean(
            consecutive_errors in 0u32..=1000,
        ) {
            let command = check_error_threshold(consecutive_errors);

            if consecutive_errors < ENCOURAGEMENT_THRESHOLD {
                prop_assert_eq!(command, None);
            } else if consecutive_errors < SHOW_ANSWER_THRESHOLD {
                prop_assert_eq!(command, Some(ErrorThresholdCommand::ShowEncouragement));
            } else {
                prop_assert_eq!(command, Some(ErrorThresholdCommand::ShowCorrectAnswer));
            }

            // 鼓励文案候选池始终不含黑名单词汇，与 consecutive_errors 取值无关，
            // 在同一属性测试中一并断言以覆盖 Property 17 的第二部分描述。
            prop_assert_eq!(validate_encouragement_pool(ENCOURAGEMENT_POOL), Ok(()));
        }
    }
}

/// 课程完成时展示的奖励性反馈展示时长下限（毫秒），对应"不少于2秒"（Req 7.3）。
pub const MIN_REWARD_DISPLAY_DURATION_MS: u64 = 2000;

/// 课程完成时的奖励性反馈元数据（Req 7.3）。
///
/// 三种反馈形式——动画、徽章、鼓励文案——中至少包含一种（`animation`、
/// `badge`、`encouragement_text` 三个字段中至少一个为 `true`/`Some`），
/// 且 `display_duration_ms` 恒不小于 [`MIN_REWARD_DISPLAY_DURATION_MS`]。
///
/// 本结构体只承载"应展示哪些反馈形式及展示多久"的元数据，具体的动画播放、
/// 徽章渲染由 UI 层（`reward_overlay.slint`）根据这些字段实现，领域层不涉及
/// 任何渲染细节。
#[derive(Debug, Clone, PartialEq)]
pub struct RewardFeedback {
    /// 是否展示奖励动画。
    pub animation: bool,
    /// 是否展示徽章。
    pub badge: bool,
    /// 鼓励文案；`Some` 表示展示该文案，`None` 表示不展示鼓励文案这一形式。
    pub encouragement_text: Option<&'static str>,
    /// 反馈展示时长（毫秒），恒不小于 [`MIN_REWARD_DISPLAY_DURATION_MS`]。
    pub display_duration_ms: u64,
}

impl RewardFeedback {
    /// 三种反馈形式中是否至少存在一种（Req 7.3 的核心约束）。
    ///
    /// UI 侧不需要这个判定：`RewardOverlay` 对三种形式各有一个独立的条件
    /// 分支，"至少有一种"是生成器 `generate_lesson_completion_reward` 的
    /// 后置条件，由本方法在单测/属性测试中断言。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_at_least_one_form(&self) -> bool {
        self.animation || self.badge || self.encouragement_text.is_some()
    }
}

/// 课程完成奖励反馈的鼓励文案候选池（Req 7.3）。
///
/// 与 [`ENCOURAGEMENT_POOL`]（错误阈值提示文案）分开维护，因为二者的使用场景
/// 与语气侧重不同（一个是练习中遇到困难时的鼓励，一个是完成课程后的庆祝）；
/// 但同样不包含 [`NEGATIVE_WORD_BLACKLIST`] 中的负面评价词汇。
pub const LESSON_COMPLETION_ENCOURAGEMENT_POOL: &[&str] = &[
    "太棒了，你完成了这个课程！",
    "了不起，你又掌握了新技能！",
    "做得非常好，继续保持！",
    "恭喜你，又前进了一步！",
];

/// 纯函数：生成课程完成时的奖励性反馈元数据（Req 7.3）。
///
/// 恒同时包含动画与徽章两种形式，并从
/// [`LESSON_COMPLETION_ENCOURAGEMENT_POOL`] 中依据 `lesson_id` 稳定地选取一条
/// 鼓励文案，因此返回结果恒满足"三种形式中至少一种存在"；`display_duration_ms`
/// 恒等于 [`MIN_REWARD_DISPLAY_DURATION_MS`]，满足"展示时长不少于2秒"。
///
/// 依据 `lesson_id` 选取文案（而非固定取第一条）只是为了让不同课程的完成反馈
/// 文案有所区分，不影响本函数的正确性约束。
pub fn generate_lesson_completion_reward(lesson_id: &LessonId) -> RewardFeedback {
    let pool = LESSON_COMPLETION_ENCOURAGEMENT_POOL;
    let index = (lesson_id.0.len()) % pool.len();
    let encouragement_text = Some(pool[index]);

    RewardFeedback {
        animation: true,
        badge: true,
        encouragement_text,
        display_duration_ms: MIN_REWARD_DISPLAY_DURATION_MS,
    }
}

#[cfg(test)]
mod reward_feedback_tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn generated_reward_has_at_least_one_form() {
        let reward = generate_lesson_completion_reward(&LessonId("lesson-1".to_string()));
        assert!(reward.has_at_least_one_form());
    }

    #[test]
    fn generated_reward_meets_minimum_duration() {
        let reward = generate_lesson_completion_reward(&LessonId("lesson-1".to_string()));
        assert!(reward.display_duration_ms >= MIN_REWARD_DISPLAY_DURATION_MS);
    }

    #[test]
    fn generated_reward_includes_non_blacklisted_encouragement_text() {
        let reward = generate_lesson_completion_reward(&LessonId("any-lesson".to_string()));
        let text = reward
            .encouragement_text
            .expect("expected encouragement text");
        for banned_word in NEGATIVE_WORD_BLACKLIST {
            assert!(
                !text.contains(banned_word),
                "text {text} contains banned word {banned_word}"
            );
        }
    }

    #[test]
    fn feedback_without_any_form_reports_false() {
        let empty = RewardFeedback {
            animation: false,
            badge: false,
            encouragement_text: None,
            display_duration_ms: MIN_REWARD_DISPLAY_DURATION_MS,
        };
        assert!(!empty.has_at_least_one_form());
    }

    // Feature: typing-desktop-app, Property 18: 课程完成奖励反馈的存在性
    //
    // 对于任意课程完成事件，生成的 UI 命令集合中至少包含"播放动画"、"展示徽章"、
    // "展示鼓励文案"三种命令类型之一，且展示时长不少于2秒。
    //
    // Validates: Requirements 7.3
    proptest! {
        #[test]
        fn prop_lesson_completion_reward_has_form_and_min_duration(
            lesson_id_str in "[a-zA-Z0-9_-]{1,50}",
        ) {
            let lesson_id = LessonId(lesson_id_str);
            let reward = generate_lesson_completion_reward(&lesson_id);

            prop_assert!(reward.has_at_least_one_form());
            prop_assert!(reward.display_duration_ms >= MIN_REWARD_DISPLAY_DURATION_MS);
        }
    }
}

#[cfg(test)]
mod compare_with_best_tests {
    use super::*;
    use crate::domain::curriculum::LessonId;
    use proptest::prelude::*;

    fn sample_result(accuracy: f32, wpm: f32) -> PracticeResult {
        PracticeResult {
            lesson_id: LessonId("lesson-1".to_string()),
            accuracy,
            wpm,
            error_count: 0,
            duration_ms: 0,
            score: 0,
        }
    }

    #[test]
    fn no_best_returns_first_record() {
        let current = sample_result(80.0, 30.0);
        assert_eq!(
            compare_with_best(&current, None),
            ComparisonResult::FirstRecord
        );
    }

    #[test]
    fn equal_accuracy_and_wpm_returns_equal() {
        let current = sample_result(90.0, 40.0);
        let best = sample_result(90.0, 40.0);
        assert_eq!(
            compare_with_best(&current, Some(&best)),
            ComparisonResult::Equal
        );
    }

    #[test]
    fn higher_accuracy_returns_higher_with_deltas() {
        let current = sample_result(95.0, 38.0);
        let best = sample_result(90.0, 40.0);
        match compare_with_best(&current, Some(&best)) {
            ComparisonResult::Higher {
                accuracy_delta,
                wpm_delta,
            } => {
                assert!((accuracy_delta - 5.0).abs() < 1e-6);
                assert!((wpm_delta - (-2.0)).abs() < 1e-6);
            }
            other => panic!("expected Higher, got {other:?}"),
        }
    }

    #[test]
    fn lower_accuracy_returns_lower_with_deltas() {
        let current = sample_result(80.0, 45.0);
        let best = sample_result(90.0, 40.0);
        match compare_with_best(&current, Some(&best)) {
            ComparisonResult::Lower {
                accuracy_delta,
                wpm_delta,
            } => {
                assert!((accuracy_delta - (-10.0)).abs() < 1e-6);
                assert!((wpm_delta - 5.0).abs() < 1e-6);
            }
            other => panic!("expected Lower, got {other:?}"),
        }
    }

    #[test]
    fn equal_accuracy_falls_back_to_wpm_for_direction() {
        let current = sample_result(90.0, 42.0);
        let best = sample_result(90.0, 40.0);
        match compare_with_best(&current, Some(&best)) {
            ComparisonResult::Higher {
                accuracy_delta,
                wpm_delta,
            } => {
                assert_eq!(accuracy_delta, 0.0);
                assert!((wpm_delta - 2.0).abs() < 1e-6);
            }
            other => panic!("expected Higher, got {other:?}"),
        }

        let current_lower_wpm = sample_result(90.0, 38.0);
        match compare_with_best(&current_lower_wpm, Some(&best)) {
            ComparisonResult::Lower {
                accuracy_delta,
                wpm_delta,
            } => {
                assert_eq!(accuracy_delta, 0.0);
                assert!((wpm_delta - (-2.0)).abs() < 1e-6);
            }
            other => panic!("expected Lower, got {other:?}"),
        }
    }

    // Feature: typing-desktop-app, Property 9: 历史最佳成绩比较的正确性
    //
    // 对于任意本次成绩 current 与任意可选的历史最佳成绩 best：
    // 当 best 为空时，compare_with_best 返回 FirstRecord；
    // 当 best 存在时，返回值中的 Higher/Lower/Equal 分支与 current 相对于 best
    // 的准确率、速度数值大小关系严格对应，且差值字段恒等于 current 与 best
    // 对应字段的数值差。
    //
    // Validates: Requirements 4.4, 4.5
    proptest! {
        #[test]
        fn prop_compare_with_best_correctness(
            current_accuracy in 0.0f32..=100.0,
            current_wpm in 0.0f32..=300.0,
            best_accuracy_opt in proptest::option::of(0.0f32..=100.0),
            best_wpm in 0.0f32..=300.0,
        ) {
            let current = sample_result(current_accuracy, current_wpm);

            match best_accuracy_opt {
                None => {
                    // best 为空 -> 恒为 FirstRecord。
                    prop_assert_eq!(
                        compare_with_best(&current, None),
                        ComparisonResult::FirstRecord
                    );
                }
                Some(best_accuracy) => {
                    let best = sample_result(best_accuracy, best_wpm);
                    let result = compare_with_best(&current, Some(&best));

                    let expected_accuracy_delta = current_accuracy - best_accuracy;
                    let expected_wpm_delta = current_wpm - best_wpm;

                    match result {
                        ComparisonResult::Equal => {
                            // Equal 当且仅当正确率与速度都完全相等。
                            prop_assert_eq!(expected_accuracy_delta, 0.0);
                            prop_assert_eq!(expected_wpm_delta, 0.0);
                        }
                        ComparisonResult::Higher { accuracy_delta, wpm_delta } => {
                            // 差值字段恒等于 current 与 best 对应字段的数值差。
                            prop_assert_eq!(accuracy_delta, expected_accuracy_delta);
                            prop_assert_eq!(wpm_delta, expected_wpm_delta);
                            // 方向与"正确率优先，相等时比较速度"的规则一致。
                            if expected_accuracy_delta != 0.0 {
                                prop_assert!(expected_accuracy_delta > 0.0);
                            } else {
                                prop_assert!(expected_wpm_delta > 0.0);
                            }
                        }
                        ComparisonResult::Lower { accuracy_delta, wpm_delta } => {
                            prop_assert_eq!(accuracy_delta, expected_accuracy_delta);
                            prop_assert_eq!(wpm_delta, expected_wpm_delta);
                            if expected_accuracy_delta != 0.0 {
                                prop_assert!(expected_accuracy_delta < 0.0);
                            } else {
                                prop_assert!(expected_wpm_delta < 0.0);
                            }
                        }
                        ComparisonResult::FirstRecord => {
                            prop_assert!(false, "best 存在时不应返回 FirstRecord");
                        }
                    }
                }
            }
        }
    }
}
