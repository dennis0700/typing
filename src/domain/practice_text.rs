//! 练习文本生成（单键/词语/句子）。

use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::domain::curriculum::{Lesson, LessonGoal};
use crate::domain::keyboard_layout::{KeyCode, key_to_char, shifted_char};

/// 将一个键位映射为练习文本实际使用的字符：`shift` 为 `true` 时取其 Shift
/// 组合字符（`shifted_char`），否则取其基础字符（`key_to_char`）。
///
/// 说明：`shifted_char` 对 `KeyCode::Space` 返回 `None`（空格没有 Shift
/// 变体），此时退回基础字符（即空格本身），保证空格始终可用作词语/句子的
/// 分隔符，不受 `shift` 取值影响——这与 Shift 课程仍然用空格分隔词语/句子的
/// 直觉一致（Shift 修饰的是符号本身，不是分隔符）。
fn effective_char(key: KeyCode, shift: bool) -> char {
    if shift {
        shifted_char(key).unwrap_or_else(|| key_to_char(key))
    } else {
        key_to_char(key)
    }
}

/// 单键练习文本的最短长度（Req 8.1）。
pub const SINGLE_KEY_TEXT_MIN_LEN: usize = 20;
/// 单键练习文本的最长长度（Req 8.1）。
pub const SINGLE_KEY_TEXT_MAX_LEN: usize = 60;

/// 词语练习：单词最长长度（Req 8.2）。
pub const WORD_TEXT_MAX_WORD_LEN: usize = 10;
/// 词语练习文本总长度下限（Req 8.2）。
pub const WORD_TEXT_MIN_LEN: usize = 20;
/// 词语练习文本总长度上限（Req 8.2）。
pub const WORD_TEXT_MAX_LEN: usize = 60;

/// 句子练习：单句最短长度（Req 8.3）。
pub const SENTENCE_TEXT_MIN_SENTENCE_LEN: usize = 10;
/// 句子练习：单句最长长度（Req 8.3）。
pub const SENTENCE_TEXT_MAX_SENTENCE_LEN: usize = 80;
/// 句子练习文本总长度下限（Req 8.3）。
pub const SENTENCE_TEXT_MIN_LEN: usize = 20;
/// 句子练习文本总长度上限（Req 8.3）。
pub const SENTENCE_TEXT_MAX_LEN: usize = 120;

/// 练习文本生成失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextGenError {
    /// 已解锁/目标键位不足以生成满足长度要求的文本（触发 Req 8.5 降级策略）。
    InsufficientKeys,
}

/// 纯函数：生成单键练习文本（Req 8.1）。
///
/// 从 `target_keys` 对应的字符集合中均匀随机采样，生成长度在
/// `[SINGLE_KEY_TEXT_MIN_LEN, SINGLE_KEY_TEXT_MAX_LEN]`（20–60，含两端）之间的文本，
/// 文本中的每个字符都能映射回 `target_keys` 中的某个键位（通过 `char_to_key`
/// 反向对应，见 `effective_char`）。
///
/// `shift` 为 `true` 时，取每个键位的 Shift 组合字符（`shifted_char`，即
/// 大写字母/`~!@#$%^&*()_+{}|:"<>?`）而非基础字符；`false` 时行为与此前
/// 完全一致。
///
/// `rng_seed` 用于构造可复现的伪随机数生成器：相同的 `target_keys`、`shift`
/// 与 `rng_seed` 组合恒产生相同的输出文本（确定性，便于测试与回归对比）。
///
/// # 错误
///
/// 若 `target_keys` 为空，无法采样出任何字符，返回 `Err(TextGenError::InsufficientKeys)`。
pub fn generate_single_key_text(
    target_keys: &[KeyCode],
    shift: bool,
    rng_seed: u64,
) -> Result<String, TextGenError> {
    if target_keys.is_empty() {
        return Err(TextGenError::InsufficientKeys);
    }

    let chars: Vec<char> = target_keys.iter().map(|&key| effective_char(key, shift)).collect();

    let mut rng = StdRng::seed_from_u64(rng_seed);
    // 长度采样范围为 [MIN, MAX]（闭区间），random_range 的上界为排他，故 +1。
    let len = rng.random_range(SINGLE_KEY_TEXT_MIN_LEN..=SINGLE_KEY_TEXT_MAX_LEN);

    let text: String = (0..len)
        .map(|_| {
            let idx = rng.random_range(0..chars.len());
            chars[idx]
        })
        .collect();

    Ok(text)
}

/// 纯函数：生成词语练习文本（Req 8.2）。
///
/// 由于应用不依赖外部词典数据源，词语练习文本通过"从已解锁键位对应字符集合中
/// 随机组合出词语状字符块"的策略生成：随机确定每个词语的长度（1 至
/// `WORD_TEXT_MAX_WORD_LEN`，即 10 个字符），并从 `unlocked_keys` 对应字符集合中
/// 均匀随机采样组成该词语，多个词语之间以空格连接，直至练习文本总长度落在
/// `[WORD_TEXT_MIN_LEN, WORD_TEXT_MAX_LEN]`（20–60，含两端）区间内。
///
/// 生成文本中的每个字符（不含词语间的分隔空格）都能映射回 `unlocked_keys` 中的
/// 某个键位（通过 `char_to_key` 反向对应，见 `effective_char`）。
///
/// 说明：空格字符（`KeyCode::Space`）本身被用作词语之间的分隔符，因此不会被
/// 采样为词语内容字符——即使 `unlocked_keys` 中包含 `KeyCode::Space`，词语内容
/// 仍只从其余已解锁字符中采样，以保证词语与分隔符在文本中可被区分。
///
/// `shift` 为 `true` 时，词语内容取每个键位的 Shift 组合字符而非基础字符
/// （语义与 [`generate_single_key_text`] 的 `shift` 参数一致）。
///
/// `rng_seed` 用于构造可复现的伪随机数生成器：相同的 `unlocked_keys`、`shift`
/// 与 `rng_seed` 组合恒产生相同的输出文本（确定性，便于测试与回归对比）。
///
/// # 错误
///
/// 若 `unlocked_keys` 为空，或已解锁键位在排除空格后无任何可用字符（例如
/// `unlocked_keys` 仅包含 `KeyCode::Space`），无法采样出任何词语内容字符，
/// 返回 `Err(TextGenError::InsufficientKeys)`。
pub fn generate_word_practice_text(
    unlocked_keys: &[KeyCode],
    shift: bool,
    rng_seed: u64,
) -> Result<String, TextGenError> {
    if unlocked_keys.is_empty() {
        return Err(TextGenError::InsufficientKeys);
    }

    // 空格保留作为词语分隔符，不参与词语内容字符采样。
    let chars: Vec<char> = unlocked_keys
        .iter()
        .map(|&key| effective_char(key, shift))
        .filter(|&c| c != ' ')
        .collect();

    if chars.is_empty() {
        return Err(TextGenError::InsufficientKeys);
    }

    let mut rng = StdRng::seed_from_u64(rng_seed);
    let target_len = rng.random_range(WORD_TEXT_MIN_LEN..=WORD_TEXT_MAX_LEN);

    let mut text = String::new();
    while text.chars().count() < target_len {
        let separator_len = if text.is_empty() { 0 } else { 1 };

        // 剩余额度：以硬上限 WORD_TEXT_MAX_LEN（而非软目标 target_len）计算，
        // 保证至少还能放入 1 个字符的词语，从而不会为避免"未达标"而提前结束循环
        // 导致总长度跌破 WORD_TEXT_MIN_LEN；硬上限保证总长度恒不超过
        // WORD_TEXT_MAX_LEN。
        let hard_remaining = WORD_TEXT_MAX_LEN - text.chars().count() - separator_len;

        if hard_remaining == 0 {
            // 已达硬上限，无法再放入分隔符+至少 1 个字符，结束生成。
            break;
        }

        if separator_len == 1 {
            text.push(' ');
        }

        // 词语长度：1 至 WORD_TEXT_MAX_WORD_LEN，且不超过硬上限剩余额度。
        let max_word_len = WORD_TEXT_MAX_WORD_LEN.min(hard_remaining);
        let word_len = rng.random_range(1..=max_word_len);

        for _ in 0..word_len {
            let idx = rng.random_range(0..chars.len());
            text.push(chars[idx]);
        }
    }

    Ok(text)
}

/// 纯函数：生成句子练习文本（Req 8.3）。
///
/// 由于应用不依赖外部语料数据源，句子练习文本通过"从已解锁键位对应字符集合中
/// 随机组合出句子状字符块"的策略生成：随机确定每个句子的长度（
/// `SENTENCE_TEXT_MIN_SENTENCE_LEN` 至 `SENTENCE_TEXT_MAX_SENTENCE_LEN`，即
/// 10–80 个字符），并从 `unlocked_keys` 对应字符集合中均匀随机采样组成该句子，
/// 多个句子之间以空格连接，直至练习文本总长度落在
/// `[SENTENCE_TEXT_MIN_LEN, SENTENCE_TEXT_MAX_LEN]`（20–120，含两端）区间内。
///
/// 生成文本中的每个字符（不含句子间的分隔空格）都能映射回 `unlocked_keys` 中的
/// 某个键位（通过 `char_to_key` 反向对应，见 `effective_char`）。
///
/// 说明：空格字符（`KeyCode::Space`）本身被用作句子之间的分隔符，因此不会被
/// 采样为句子内容字符——即使 `unlocked_keys` 中包含 `KeyCode::Space`，句子内容
/// 仍只从其余已解锁字符中采样，以保证句子与分隔符在文本中可被区分。
///
/// `shift` 为 `true` 时，句子内容取每个键位的 Shift 组合字符而非基础字符
/// （语义与 [`generate_single_key_text`] 的 `shift` 参数一致）。
///
/// `rng_seed` 用于构造可复现的伪随机数生成器：相同的 `unlocked_keys`、`shift`
/// 与 `rng_seed` 组合恒产生相同的输出文本（确定性，便于测试与回归对比）。
///
/// # 错误
///
/// 若 `unlocked_keys` 为空，或已解锁键位在排除空格后无任何可用字符（例如
/// `unlocked_keys` 仅包含 `KeyCode::Space`），无法采样出任何句子内容字符，
/// 返回 `Err(TextGenError::InsufficientKeys)`。
pub fn generate_sentence_practice_text(
    unlocked_keys: &[KeyCode],
    shift: bool,
    rng_seed: u64,
) -> Result<String, TextGenError> {
    if unlocked_keys.is_empty() {
        return Err(TextGenError::InsufficientKeys);
    }

    // 空格保留作为句子分隔符，不参与句子内容字符采样。
    let chars: Vec<char> = unlocked_keys
        .iter()
        .map(|&key| effective_char(key, shift))
        .filter(|&c| c != ' ')
        .collect();

    if chars.is_empty() {
        return Err(TextGenError::InsufficientKeys);
    }

    let mut rng = StdRng::seed_from_u64(rng_seed);
    let target_len = rng.random_range(SENTENCE_TEXT_MIN_LEN..=SENTENCE_TEXT_MAX_LEN);

    let mut text = String::new();
    while text.chars().count() < target_len {
        let separator_len = if text.is_empty() { 0 } else { 1 };

        // 剩余额度：以硬上限 SENTENCE_TEXT_MAX_LEN（而非软目标 target_len）计算，
        // 保证至少还能放入 1 个字符的句子，从而不会为避免"未达标"而提前结束循环
        // 导致总长度跌破 SENTENCE_TEXT_MIN_LEN；硬上限保证总长度恒不超过
        // SENTENCE_TEXT_MAX_LEN。
        let hard_remaining = SENTENCE_TEXT_MAX_LEN - text.chars().count() - separator_len;

        if hard_remaining == 0 {
            // 已达硬上限，无法再放入分隔符+至少 1 个字符，结束生成。
            break;
        }

        if separator_len == 1 {
            text.push(' ');
        }

        // 句子长度：SENTENCE_TEXT_MIN_SENTENCE_LEN 至 SENTENCE_TEXT_MAX_SENTENCE_LEN，
        // 且不超过硬上限剩余额度；若硬上限剩余额度小于单句最短长度，则以剩余额度
        // 作为该句子的（退化）长度上限，保证不超过硬上限。
        let max_sentence_len = SENTENCE_TEXT_MAX_SENTENCE_LEN.min(hard_remaining);
        let min_sentence_len = SENTENCE_TEXT_MIN_SENTENCE_LEN.min(max_sentence_len);
        let sentence_len = rng.random_range(min_sentence_len..=max_sentence_len);

        for _ in 0..sentence_len {
            let idx = rng.random_range(0..chars.len());
            text.push(chars[idx]);
        }
    }

    Ok(text)
}

/// 纯函数：按课程学习目标生成练习文本，并在已解锁键位不足时自动降级（Req 8.5）。
///
/// 这是练习时（runtime）的文本生成入口，与仅在配置/加载阶段做静态冲突检测的
/// `validate_lesson_config` 不同：后者只判定"是否可能"，本函数负责实际产出
/// 一段可用的练习文本，即使目标 `goal` 对应的生成策略失败也不会向上层暴露
/// 错误、不阻塞课程进入。
///
/// # 两个键位集合的区别（Req 8.1 vs 8.2/8.3）
///
/// 需求对三种学习目标规定的取字范围**并不相同**，因此本函数需要两个入参：
///
/// - `lesson_keys`：本课程指定的目标键位（`Lesson::target_keys`）。Req 8.1 要求
///   单键熟悉课程"生成**仅由该课程指定按键**对应字符组成"的文本——练习右手
///   基准键 `jkl;` 时，文本里不应该出现左手的 `asdfg`，否则这门课就不再是
///   针对这几个键位的专项练习，虚拟键盘上的高亮键位集合（`is-active`，
///   Req 2.2）也会跟着糊成一大片。
/// - `unlocked_keys`：学员当前已解锁的全部键位。Req 8.2/8.3 要求词语/句子
///   练习"仅由**学员已解锁键位**对应字符组成"——这类课程的目的是在更大的
///   字母集合上组词造句，只用本课新增的几个键位根本拼不出词。
///
/// 降级链路：
/// - `LessonGoal::SingleKey`：从 `lesson_keys` 调用 [`generate_single_key_text`]
///   （已是最简单的形式，无需降级）。
/// - `LessonGoal::Word`：从 `unlocked_keys` 调用 [`generate_word_practice_text`]；
///   若返回 `Err(InsufficientKeys)`，降级为基于 `unlocked_keys` 的
///   [`generate_single_key_text`]（Req 8.5 明确降级后的文本同样是"仅由学员已
///   解锁键位组成"，故此处不切回 `lesson_keys`）。
/// - `LessonGoal::Sentence`：`generate_sentence_practice_text` ->
///   `generate_word_practice_text` -> `generate_single_key_text`，全部基于
///   `unlocked_keys`。
///
/// 仅当降级链路中最简单的 [`generate_single_key_text`] 也失败时（即对应的键位
/// 集合为空，无法采样出任何字符），本函数才返回
/// `Err(TextGenError::InsufficientKeys)`。
///
/// `shift`：是否使用键位的 Shift 组合字符而非基础字符（对应
/// `Lesson::requires_shift`），透传给降级链路途经的每一步生成函数，保证一门
/// Shift 课程在降级后仍然只出现 Shift 组合字符，不会突然混入基础字符。
///
/// `rng_seed` 透传给实际执行的生成函数，保持该函数确定性输出的性质不变。
///
/// **Validates: Requirements 8.1, 8.2, 8.3, 8.5**
pub fn generate_practice_text(
    goal: LessonGoal,
    lesson_keys: &[KeyCode],
    unlocked_keys: &[KeyCode],
    shift: bool,
    rng_seed: u64,
) -> Result<String, TextGenError> {
    match goal {
        LessonGoal::SingleKey => generate_single_key_text(lesson_keys, shift, rng_seed),
        LessonGoal::Word => generate_word_practice_text(unlocked_keys, shift, rng_seed)
            .or_else(|_| generate_single_key_text(unlocked_keys, shift, rng_seed)),
        LessonGoal::Sentence => generate_sentence_practice_text(unlocked_keys, shift, rng_seed)
            .or_else(|_| generate_word_practice_text(unlocked_keys, shift, rng_seed))
            .or_else(|_| generate_single_key_text(unlocked_keys, shift, rng_seed)),
    }
}

/// 课程配置校验失败的具体原因（Req 1.8, 8.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LessonConfigError {
    /// 课程未指定任何目标键位，无法生成任何练习内容（数据缺失/损坏，Req 1.8）。
    NoTargetKeys,
    /// 该课程的目标键位在理论上无法生成满足其 `goal` 长度约束的练习文本
    /// （配置冲突，Req 8.4）。
    InsufficientKeysForGoal,
}

impl std::fmt::Display for LessonConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LessonConfigError::NoTargetKeys => write!(f, "课程未配置任何目标键位"),
            LessonConfigError::InsufficientKeysForGoal => {
                write!(
                    f,
                    "目标键位数量无法生成满足本课程学习目标长度要求的练习内容"
                )
            }
        }
    }
}

/// 纯函数：校验课程配置是否能生成满足其 `goal` 长度约束的练习内容（Req 1.8, 8.4）。
///
/// 校验规则：
/// - `target_keys` 为空 -> `Err(NoTargetKeys)`（课程数据缺失，Req 1.8）。
/// - 单键练习（`LessonGoal::SingleKey`）：仅需 `target_keys` 非空即可从中采样生成
///   任意长度（20–60）的文本（可重复采样单个字符），因此非空即通过。
/// - 词语/句子练习（`LessonGoal::Word`/`LessonGoal::Sentence`）：直接以固定种子
///   （`0`）分别调用 [`generate_word_practice_text`]/[`generate_sentence_practice_text`]
///   作为可行性探测——这与运行时（`generate_practice_text`）实际会执行的生成
///   策略完全一致，因此本校验判定的"是否可能"与运行时生成器的真实行为不存在
///   偏差。种子的具体取值不影响可行性判定结果：两个生成函数仅在
///   `target_keys`/`unlocked_keys` 排除分隔空格后为空时才返回
///   `Err(InsufficientKeys)`，该条件与种子无关，因此固定种子足以作为纯粹的
///   可行性检查（不产出实际练习文本）。
///   - `Ok(_)` -> `Ok(())`：该目标键位集合能够生成满足长度约束的文本。
///   - `Err(TextGenError::InsufficientKeys)` -> `Err(InsufficientKeysForGoal)`：
///     目标键位（排除空格后）为空，理论上无法组出任何词语/句子。
///
/// 校验失败的课程不会进入 `LessonState::Unlocked`/`Locked` 判定，而是被
/// `CurriculumState` 标记为 `LessonState::Unavailable(reason)`。
pub fn validate_lesson_config(lesson: &Lesson) -> Result<(), LessonConfigError> {
    if lesson.target_keys.is_empty() {
        return Err(LessonConfigError::NoTargetKeys);
    }

    /// 用于可行性探测的固定种子：本校验只关心生成是否可能（`Ok`/`Err`），
    /// 不关心具体产出的文本内容，因此种子取值是任意的。
    const FEASIBILITY_CHECK_SEED: u64 = 0;

    match lesson.goal {
        LessonGoal::SingleKey => Ok(()),
        LessonGoal::Word => generate_word_practice_text(
            &lesson.target_keys,
            lesson.requires_shift,
            FEASIBILITY_CHECK_SEED,
        )
        .map(|_| ())
        .map_err(|TextGenError::InsufficientKeys| LessonConfigError::InsufficientKeysForGoal),
        LessonGoal::Sentence => {
            generate_sentence_practice_text(
                &lesson.target_keys,
                lesson.requires_shift,
                FEASIBILITY_CHECK_SEED,
            )
            .map(|_| ())
            .map_err(|TextGenError::InsufficientKeys| LessonConfigError::InsufficientKeysForGoal)
        }
    }
}

#[cfg(test)]
mod validate_lesson_config_tests {
    use super::*;
    use crate::domain::{LessonId, UnlockCriteria};

    fn lesson(goal: LessonGoal, target_keys: Vec<KeyCode>) -> Lesson {
        Lesson {
            id: LessonId("lesson-under-test".to_string()),
            title: "被测课程".to_string(),
            goal,
            target_keys,
            requires_shift: false,
            unlock_criteria: UnlockCriteria {
                min_accuracy: None,
                max_duration_secs: None,
                min_attempts: None,
            },
        }
    }

    #[test]
    fn empty_target_keys_is_no_target_keys_error() {
        let l = lesson(LessonGoal::SingleKey, vec![]);
        assert_eq!(
            validate_lesson_config(&l),
            Err(LessonConfigError::NoTargetKeys)
        );
    }

    #[test]
    fn single_key_goal_with_one_key_is_valid() {
        let l = lesson(LessonGoal::SingleKey, vec![KeyCode::F]);
        assert_eq!(validate_lesson_config(&l), Ok(()));
    }

    #[test]
    fn word_goal_with_only_space_key_is_conflict() {
        // target_keys 排除分隔空格后无任何可用字符，generate_word_practice_text
        // 恒返回 InsufficientKeys，理论上无法组出任何"词语"。
        let l = lesson(LessonGoal::Word, vec![KeyCode::Space]);
        assert_eq!(
            validate_lesson_config(&l),
            Err(LessonConfigError::InsufficientKeysForGoal)
        );
    }

    #[test]
    fn word_goal_with_single_distinct_key_is_valid() {
        // 单个非空格键位重复多次即可满足 generate_word_practice_text 的采样
        // 需求（词语内容按字符独立采样，不要求键位"不同"），因此应通过校验。
        let l = lesson(LessonGoal::Word, vec![KeyCode::F, KeyCode::F]);
        assert_eq!(validate_lesson_config(&l), Ok(()));
    }

    #[test]
    fn word_goal_with_two_distinct_keys_is_valid() {
        let l = lesson(LessonGoal::Word, vec![KeyCode::F, KeyCode::J]);
        assert_eq!(validate_lesson_config(&l), Ok(()));
    }

    #[test]
    fn sentence_goal_with_only_space_key_is_conflict() {
        let l = lesson(LessonGoal::Sentence, vec![KeyCode::Space]);
        assert_eq!(
            validate_lesson_config(&l),
            Err(LessonConfigError::InsufficientKeysForGoal)
        );
    }

    #[test]
    fn sentence_goal_with_single_distinct_key_is_valid() {
        let l = lesson(LessonGoal::Sentence, vec![KeyCode::A]);
        assert_eq!(validate_lesson_config(&l), Ok(()));
    }

    #[test]
    fn sentence_goal_with_multiple_distinct_keys_is_valid() {
        let l = lesson(
            LessonGoal::Sentence,
            vec![KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F],
        );
        assert_eq!(validate_lesson_config(&l), Ok(()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 全部标准键位列表，供属性测试生成任意非空 `target_keys` 子集使用。
    fn all_key_codes() -> Vec<KeyCode> {
        vec![
            KeyCode::Backquote,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
            KeyCode::Digit0,
            KeyCode::Minus,
            KeyCode::Equal,
            KeyCode::Q,
            KeyCode::W,
            KeyCode::E,
            KeyCode::R,
            KeyCode::T,
            KeyCode::Y,
            KeyCode::U,
            KeyCode::I,
            KeyCode::O,
            KeyCode::P,
            KeyCode::BracketLeft,
            KeyCode::BracketRight,
            KeyCode::Backslash,
            KeyCode::A,
            KeyCode::S,
            KeyCode::D,
            KeyCode::F,
            KeyCode::G,
            KeyCode::H,
            KeyCode::J,
            KeyCode::K,
            KeyCode::L,
            KeyCode::Semicolon,
            KeyCode::Quote,
            KeyCode::Z,
            KeyCode::X,
            KeyCode::C,
            KeyCode::V,
            KeyCode::B,
            KeyCode::N,
            KeyCode::M,
            KeyCode::Comma,
            KeyCode::Period,
            KeyCode::Slash,
            KeyCode::Space,
        ]
    }

    /// 生成任意非空 `target_keys` 子集的策略（1–5 个键位，允许重复以覆盖单键课程常见场景）。
    fn arb_target_keys() -> impl Strategy<Value = Vec<KeyCode>> {
        let pool = all_key_codes();
        prop::collection::vec(prop::sample::select(pool), 1..=5)
    }

    #[test]
    fn empty_target_keys_returns_insufficient_keys_error() {
        let result = generate_single_key_text(&[], false, 42);
        assert_eq!(result, Err(TextGenError::InsufficientKeys));
    }

    #[test]
    fn empty_unlocked_keys_returns_insufficient_keys_error_for_word_text() {
        let result = generate_word_practice_text(&[], false, 42);
        assert_eq!(result, Err(TextGenError::InsufficientKeys));
    }

    #[test]
    fn unlocked_keys_containing_only_space_returns_insufficient_keys_error() {
        // Space 被保留为词语分隔符，若已解锁键位排除空格后无任何可用字符，
        // 应视为键位不足。
        let result = generate_word_practice_text(&[KeyCode::Space], false, 42);
        assert_eq!(result, Err(TextGenError::InsufficientKeys));
    }

    #[test]
    fn word_text_total_length_within_bounds() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        for seed in 0..50u64 {
            let text = generate_word_practice_text(&keys, false, seed).expect("非空键位应生成成功");
            let len = text.chars().count();
            assert!(
                (WORD_TEXT_MIN_LEN..=WORD_TEXT_MAX_LEN).contains(&len),
                "总长度 {len} 超出 [{WORD_TEXT_MIN_LEN}, {WORD_TEXT_MAX_LEN}] 范围（seed={seed}）"
            );
        }
    }

    #[test]
    fn word_text_each_word_length_within_bounds() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        for seed in 0..50u64 {
            let text = generate_word_practice_text(&keys, false, seed).expect("非空键位应生成成功");
            for word in text.split(' ') {
                let word_len = word.chars().count();
                assert!(word_len >= 1, "词语长度不应为 0（seed={seed}）");
                assert!(
                    word_len <= WORD_TEXT_MAX_WORD_LEN,
                    "词语长度 {word_len} 超过上限 {WORD_TEXT_MAX_WORD_LEN}（seed={seed}）"
                );
            }
        }
    }

    #[test]
    fn word_text_chars_are_all_within_unlocked_key_char_set() {
        let keys = [KeyCode::J, KeyCode::K, KeyCode::L];
        let allowed: std::collections::HashSet<char> =
            keys.iter().map(|&key| key_to_char(key)).collect();
        let text = generate_word_practice_text(&keys, false, 99).expect("非空键位应生成成功");
        for c in text.chars() {
            assert!(
                c == ' ' || allowed.contains(&c),
                "字符 {c:?} 不属于已解锁键位对应字符集合"
            );
        }
    }

    #[test]
    fn word_text_same_seed_and_keys_produce_deterministic_output() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        let text1 = generate_word_practice_text(&keys, false, 7).unwrap();
        let text2 = generate_word_practice_text(&keys, false, 7).unwrap();
        assert_eq!(text1, text2, "相同 unlocked_keys 与 rng_seed 应产生相同输出");
    }

    #[test]
    fn empty_unlocked_keys_returns_insufficient_keys_error_for_sentence_text() {
        let result = generate_sentence_practice_text(&[], false, 42);
        assert_eq!(result, Err(TextGenError::InsufficientKeys));
    }

    #[test]
    fn unlocked_keys_containing_only_space_returns_insufficient_keys_error_for_sentence_text() {
        // Space 被保留为句子分隔符，若已解锁键位排除空格后无任何可用字符，
        // 应视为键位不足。
        let result = generate_sentence_practice_text(&[KeyCode::Space], false, 42);
        assert_eq!(result, Err(TextGenError::InsufficientKeys));
    }

    #[test]
    fn sentence_text_total_length_within_bounds() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        for seed in 0..50u64 {
            let text = generate_sentence_practice_text(&keys, false, seed).expect("非空键位应生成成功");
            let len = text.chars().count();
            assert!(
                (SENTENCE_TEXT_MIN_LEN..=SENTENCE_TEXT_MAX_LEN).contains(&len),
                "总长度 {len} 超出 [{SENTENCE_TEXT_MIN_LEN}, {SENTENCE_TEXT_MAX_LEN}] 范围（seed={seed}）"
            );
        }
    }

    #[test]
    fn sentence_text_each_sentence_length_within_bounds() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        for seed in 0..50u64 {
            let text = generate_sentence_practice_text(&keys, false, seed).expect("非空键位应生成成功");
            for sentence in text.split(' ') {
                let sentence_len = sentence.chars().count();
                assert!(sentence_len >= 1, "句子长度不应为 0（seed={seed}）");
                assert!(
                    sentence_len <= SENTENCE_TEXT_MAX_SENTENCE_LEN,
                    "句子长度 {sentence_len} 超过上限 {SENTENCE_TEXT_MAX_SENTENCE_LEN}（seed={seed}）"
                );
            }
        }
    }

    #[test]
    fn sentence_text_chars_are_all_within_unlocked_key_char_set() {
        let keys = [KeyCode::J, KeyCode::K, KeyCode::L];
        let allowed: std::collections::HashSet<char> =
            keys.iter().map(|&key| key_to_char(key)).collect();
        let text = generate_sentence_practice_text(&keys, false, 99).expect("非空键位应生成成功");
        for c in text.chars() {
            assert!(
                c == ' ' || allowed.contains(&c),
                "字符 {c:?} 不属于已解锁键位对应字符集合"
            );
        }
    }

    #[test]
    fn sentence_text_same_seed_and_keys_produce_deterministic_output() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        let text1 = generate_sentence_practice_text(&keys, false, 7).unwrap();
        let text2 = generate_sentence_practice_text(&keys, false, 7).unwrap();
        assert_eq!(text1, text2, "相同 unlocked_keys 与 rng_seed 应产生相同输出");
    }

    /// Req 8.1：单键熟悉课程的文本**只能**由该课程指定的按键组成，不得混入
    /// 其他已解锁键位。
    ///
    /// 这是一条曾被违反的约束：桥接层原先把"全部已解锁键位"当作取字范围传进
    /// 来，于是练右手基准键 `jkl;` 时文本里混着左手的 `asdfg`——那门课就不再是
    /// 针对这几个键位的专项练习，虚拟键盘的高亮键位集合（Req 2.2）也跟着糊成
    /// 一大片。
    #[test]
    fn single_key_goal_uses_only_lesson_keys_not_all_unlocked_keys() {
        let lesson_keys = [KeyCode::J, KeyCode::K, KeyCode::L, KeyCode::Semicolon];
        let unlocked_keys = [
            KeyCode::A,
            KeyCode::S,
            KeyCode::D,
            KeyCode::F,
            KeyCode::G,
            KeyCode::J,
            KeyCode::K,
            KeyCode::L,
            KeyCode::Semicolon,
        ];

        // 多个种子都必须成立（不是碰巧某个种子没采到左手键）。
        for seed in 0..40u64 {
            let text =
                generate_practice_text(LessonGoal::SingleKey, &lesson_keys, &unlocked_keys, false, seed)
                    .expect("单键文本生成应成功");
            let allowed: Vec<char> = lesson_keys.iter().map(|&k| key_to_char(k)).collect();
            for ch in text.chars() {
                assert!(
                    allowed.contains(&ch),
                    "seed={seed}：文本出现了本课之外的字符 {ch:?}（文本={text:?}）"
                );
            }
        }
    }

    /// 与上一条互补：词语/句子课程的取字范围仍是"已解锁键位"（Req 8.2, 8.3），
    /// 不能被收窄成本课目标键位——否则只用几个新键根本拼不出词。
    #[test]
    fn word_and_sentence_goals_use_unlocked_keys() {
        let lesson_keys = [KeyCode::J];
        let unlocked_keys = [
            KeyCode::A,
            KeyCode::S,
            KeyCode::D,
            KeyCode::F,
            KeyCode::G,
            KeyCode::H,
            KeyCode::J,
            KeyCode::K,
            KeyCode::L,
        ];

        for goal in [LessonGoal::Word, LessonGoal::Sentence] {
            let text = generate_practice_text(goal, &lesson_keys, &unlocked_keys, false, 7)
                .expect("词语/句子文本生成应成功");
            let unlocked_chars: Vec<char> =
                unlocked_keys.iter().map(|&k| key_to_char(k)).collect();
            for ch in text.chars() {
                assert!(
                    unlocked_chars.contains(&ch) || ch == ' ',
                    "{goal:?}：文本出现了未解锁字符 {ch:?}"
                );
            }
            // 若被错误地收窄到 lesson_keys，文本将只由 'j' 与空格组成。
            assert!(
                text.chars().any(|c| c != 'j' && c != ' '),
                "{goal:?}：文本被错误地限制在本课目标键位内（文本={text:?}）"
            );
        }
    }

    #[test]
    fn generate_practice_text_single_key_goal_generates_directly() {
        let keys = [KeyCode::F];
        let result = generate_practice_text(LessonGoal::SingleKey, &keys, &keys, false, 1).unwrap();
        let direct = generate_single_key_text(&keys, false, 1).unwrap();
        assert_eq!(result, direct);
    }

    #[test]
    fn generate_practice_text_word_goal_with_sufficient_keys_uses_word_generator() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        let result = generate_practice_text(LessonGoal::Word, &keys, &keys, false, 1).unwrap();
        let direct = generate_word_practice_text(&keys, false, 1).unwrap();
        assert_eq!(result, direct);
    }

    #[test]
    fn generate_practice_text_word_goal_with_insufficient_keys_falls_back_to_single_key() {
        // unlocked_keys 仅含 Space：generate_word_practice_text 排除分隔符后无
        // 可用字符，返回 Err(InsufficientKeys)；但 generate_single_key_text 不
        // 区分空格与其他字符，仍能采样出 Space 字符组成的文本，因此应命中降级
        // 路径并成功生成。
        let keys = [KeyCode::Space];
        assert_eq!(
            generate_word_practice_text(&keys, false, 1),
            Err(TextGenError::InsufficientKeys),
            "前置条件：词语生成器在仅有 Space 时应失败"
        );

        let result = generate_practice_text(LessonGoal::Word, &keys, &keys, false, 1).unwrap();
        let fallback = generate_single_key_text(&keys, false, 1).unwrap();
        assert_eq!(
            result, fallback,
            "词语生成失败时应降级为单键文本且输出与直接调用单键生成器一致"
        );
    }

    #[test]
    fn generate_practice_text_sentence_goal_falls_back_through_chain() {
        // 正常情况：足够键位时句子目标直接使用句子生成器。
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        let result = generate_practice_text(LessonGoal::Sentence, &keys, &keys, false, 1).unwrap();
        let direct = generate_sentence_practice_text(&keys, false, 1).unwrap();
        assert_eq!(result, direct);

        // 降级情况：仅含 Space 时，句子/词语生成器均因排除分隔符后无可用字符
        // 而失败，最终应降级到单键生成器成功产出文本。
        let keys = [KeyCode::Space];
        assert_eq!(
            generate_sentence_practice_text(&keys, false, 1),
            Err(TextGenError::InsufficientKeys)
        );
        assert_eq!(
            generate_word_practice_text(&keys, false, 1),
            Err(TextGenError::InsufficientKeys)
        );
        let result = generate_practice_text(LessonGoal::Sentence, &keys, &keys, false, 1).unwrap();
        let fallback = generate_single_key_text(&keys, false, 1).unwrap();
        assert_eq!(
            result, fallback,
            "句子与词语生成均失败时应降级到单键文本"
        );
    }

    #[test]
    fn generate_practice_text_empty_unlocked_keys_returns_err_for_all_goals() {
        for goal in [LessonGoal::SingleKey, LessonGoal::Word, LessonGoal::Sentence] {
            let result = generate_practice_text(goal, &[], &[], false, 1);
            assert_eq!(
                result,
                Err(TextGenError::InsufficientKeys),
                "goal={goal:?} 时空 unlocked_keys 应恒返回 InsufficientKeys"
            );
        }
    }

    #[test]
    fn single_target_key_generates_text_within_length_bounds() {
        let text = generate_single_key_text(&[KeyCode::F], false, 1).expect("非空 target_keys 应生成成功");
        assert!(text.chars().count() >= SINGLE_KEY_TEXT_MIN_LEN);
        assert!(text.chars().count() <= SINGLE_KEY_TEXT_MAX_LEN);
        assert!(text.chars().all(|c| c == 'f'));
    }

    #[test]
    fn same_seed_and_keys_produce_deterministic_output() {
        let keys = [KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F];
        let text1 = generate_single_key_text(&keys, false, 7).unwrap();
        let text2 = generate_single_key_text(&keys, false, 7).unwrap();
        assert_eq!(text1, text2, "相同 target_keys 与 rng_seed 应产生相同输出");
    }

    #[test]
    fn different_seeds_can_produce_different_output() {
        let keys = [
            KeyCode::A,
            KeyCode::S,
            KeyCode::D,
            KeyCode::F,
            KeyCode::J,
            KeyCode::K,
        ];
        let text1 = generate_single_key_text(&keys, false, 1).unwrap();
        let text2 = generate_single_key_text(&keys, false, 2).unwrap();
        // 不同种子理论上可能偶然生成相同文本，但对多字符键位集合几乎不可能，
        // 用于验证种子确实参与了随机性来源（而非被忽略）。
        assert_ne!(text1, text2);
    }

    // Feature: typing-desktop-app, Property 15: 练习文本生成的字符集合与长度约束（单键分支）
    proptest! {
        #[test]
        fn prop_single_key_text_char_set_and_length_bounds(
            target_keys in arb_target_keys(),
            seed in any::<u64>(),
        ) {
            let text = generate_single_key_text(&target_keys, false, seed)
                .expect("非空 target_keys 应恒生成成功");

            let len = text.chars().count();
            prop_assert!(len >= SINGLE_KEY_TEXT_MIN_LEN);
            prop_assert!(len <= SINGLE_KEY_TEXT_MAX_LEN);

            let allowed_chars: std::collections::HashSet<char> =
                target_keys.iter().map(|&key| key_to_char(key)).collect();
            for c in text.chars() {
                prop_assert!(allowed_chars.contains(&c));
            }
        }
    }

    // Feature: typing-desktop-app, Property 15: 练习文本生成的字符集合与长度约束（确定性）
    proptest! {
        #[test]
        fn prop_single_key_text_is_deterministic_for_same_seed(
            target_keys in arb_target_keys(),
            seed in any::<u64>(),
        ) {
            let text1 = generate_single_key_text(&target_keys, false, seed).unwrap();
            let text2 = generate_single_key_text(&target_keys, false, seed).unwrap();
            prop_assert_eq!(text1, text2);
        }
    }

    // Feature: typing-desktop-app, Property 15: 练习文本生成的字符集合与长度约束（词语分支）
    proptest! {
        #[test]
        fn prop_word_text_char_set_and_length_bounds(
            unlocked_keys in arb_target_keys(),
            seed in any::<u64>(),
        ) {
            let result = generate_word_practice_text(&unlocked_keys, false, seed);

            // 排除空格后若无可用字符（例如 unlocked_keys 仅含 Space），
            // 应返回 InsufficientKeys 而非生成非法文本。
            let has_non_space_char = unlocked_keys
                .iter()
                .any(|&key| key_to_char(key) != ' ');
            if !has_non_space_char {
                prop_assert_eq!(result, Err(TextGenError::InsufficientKeys));
                return Ok(());
            }

            let text = result.expect("存在非空格可用字符时应恒生成成功");

            let total_len = text.chars().count();
            prop_assert!(total_len >= WORD_TEXT_MIN_LEN);
            prop_assert!(total_len <= WORD_TEXT_MAX_LEN);

            let allowed_chars: std::collections::HashSet<char> = unlocked_keys
                .iter()
                .map(|&key| key_to_char(key))
                .filter(|&c| c != ' ')
                .collect();

            for word in text.split(' ') {
                let word_len = word.chars().count();
                prop_assert!(word_len >= 1);
                prop_assert!(word_len <= WORD_TEXT_MAX_WORD_LEN);
                for c in word.chars() {
                    prop_assert!(allowed_chars.contains(&c));
                }
            }
        }
    }

    // Feature: typing-desktop-app, Property 15: 练习文本生成的字符集合与长度约束（词语分支，确定性）
    proptest! {
        #[test]
        fn prop_word_text_is_deterministic_for_same_seed(
            unlocked_keys in arb_target_keys(),
            seed in any::<u64>(),
        ) {
            let result1 = generate_word_practice_text(&unlocked_keys, false, seed);
            let result2 = generate_word_practice_text(&unlocked_keys, false, seed);
            prop_assert_eq!(result1, result2);
        }
    }

    // Feature: typing-desktop-app, Property 15: 练习文本生成的字符集合与长度约束（句子分支）
    proptest! {
        #[test]
        fn prop_sentence_text_char_set_and_length_bounds(
            unlocked_keys in arb_target_keys(),
            seed in any::<u64>(),
        ) {
            let result = generate_sentence_practice_text(&unlocked_keys, false, seed);

            // 排除空格后若无可用字符（例如 unlocked_keys 仅含 Space），
            // 应返回 InsufficientKeys 而非生成非法文本。
            let has_non_space_char = unlocked_keys
                .iter()
                .any(|&key| key_to_char(key) != ' ');
            if !has_non_space_char {
                prop_assert_eq!(result, Err(TextGenError::InsufficientKeys));
                return Ok(());
            }

            let text = result.expect("存在非空格可用字符时应恒生成成功");

            let total_len = text.chars().count();
            prop_assert!(total_len >= SENTENCE_TEXT_MIN_LEN);
            prop_assert!(total_len <= SENTENCE_TEXT_MAX_LEN);

            let allowed_chars: std::collections::HashSet<char> = unlocked_keys
                .iter()
                .map(|&key| key_to_char(key))
                .filter(|&c| c != ' ')
                .collect();

            for sentence in text.split(' ') {
                let sentence_len = sentence.chars().count();
                prop_assert!(sentence_len >= 1);
                prop_assert!(sentence_len <= SENTENCE_TEXT_MAX_SENTENCE_LEN);
                for c in sentence.chars() {
                    prop_assert!(allowed_chars.contains(&c));
                }
            }
        }
    }

    // Feature: typing-desktop-app, Property 15: 练习文本生成的字符集合与长度约束（句子分支，确定性）
    proptest! {
        #[test]
        fn prop_sentence_text_is_deterministic_for_same_seed(
            unlocked_keys in arb_target_keys(),
            seed in any::<u64>(),
        ) {
            let result1 = generate_sentence_practice_text(&unlocked_keys, false, seed);
            let result2 = generate_sentence_practice_text(&unlocked_keys, false, seed);
            prop_assert_eq!(result1, result2);
        }
    }

    /// 生成任意 `target_keys` 输入的策略，覆盖 `validate_lesson_config` 需要
    /// 区分的全部边缘情况：
    /// - 空集合（对应 Req 1.8 的 `NoTargetKeys` 分支）；
    /// - 仅含 `KeyCode::Space`（词语/句子生成器排除分隔符后无可用字符）；
    /// - 单个非空格键位（可行，理论上能生成任意长度文本）；
    /// - 多个键位（含 Space 与非 Space 混合）。
    fn arb_target_keys_for_validate() -> impl Strategy<Value = Vec<KeyCode>> {
        let pool = all_key_codes();
        prop_oneof![
            // 空集合：边缘情况，触发 NoTargetKeys。
            Just(Vec::<KeyCode>::new()),
            // 仅含 Space，1-3 个（重复次数不影响"排除空格后无可用字符"这一事实）。
            prop::collection::vec(Just(KeyCode::Space), 1..=3),
            // 任意 1-6 个键位（允许重复，允许与 Space 混合），覆盖单键/多键场景。
            prop::collection::vec(prop::sample::select(pool), 1..=6),
        ]
    }

    /// 与 `Lesson::goal` 一起被测试的三种学习目标。
    fn arb_lesson_goal() -> impl Strategy<Value = LessonGoal> {
        prop_oneof![
            Just(LessonGoal::SingleKey),
            Just(LessonGoal::Word),
            Just(LessonGoal::Sentence),
        ]
    }

    /// 构造仅用于校验逻辑的 `Lesson`（`unlock_criteria` 与本属性测试无关，取默认空值）。
    fn lesson_for_validate(goal: LessonGoal, target_keys: Vec<KeyCode>) -> Lesson {
        use crate::domain::curriculum::{LessonId, UnlockCriteria};
        Lesson {
            id: LessonId("prop-test-lesson".to_string()),
            title: "属性测试课程".to_string(),
            goal,
            target_keys,
            requires_shift: false,
            unlock_criteria: UnlockCriteria {
                min_accuracy: None,
                max_duration_secs: None,
                min_attempts: None,
            },
        }
    }

    /// 与 `validate_lesson_config` 内部使用的固定探测种子保持一致——
    /// 由于两个生成器的成功/失败只取决于排除分隔空格后是否存在可用字符，
    /// 与种子取值无关，本测试固定种子即可复现同样的可行性判定。
    const VALIDATE_FEASIBILITY_SEED: u64 = 0;

    // Feature: typing-desktop-app, Property 16: 课程配置冲突检测
    proptest! {
        #[test]
        fn prop_validate_lesson_config_matches_generator_feasibility(
            goal in arb_lesson_goal(),
            target_keys in arb_target_keys_for_validate(),
        ) {
            let lesson = lesson_for_validate(goal, target_keys.clone());
            let validation = validate_lesson_config(&lesson);

            if target_keys.is_empty() {
                // 空 target_keys：无论 goal 为何，恒返回数据缺失错误。
                prop_assert_eq!(validation, Err(LessonConfigError::NoTargetKeys));
                return Ok(());
            }

            match goal {
                LessonGoal::SingleKey => {
                    // 非空 target_keys 时单键练习恒可行（可重复采样单字符）。
                    prop_assert_eq!(validation, Ok(()));
                }
                LessonGoal::Word => {
                    let generator_result =
                        generate_word_practice_text(&target_keys, false, VALIDATE_FEASIBILITY_SEED);
                    match generator_result {
                        Ok(_) => prop_assert_eq!(validation, Ok(())),
                        Err(TextGenError::InsufficientKeys) => prop_assert_eq!(
                            validation,
                            Err(LessonConfigError::InsufficientKeysForGoal)
                        ),
                    }
                }
                LessonGoal::Sentence => {
                    let generator_result =
                        generate_sentence_practice_text(&target_keys, false, VALIDATE_FEASIBILITY_SEED);
                    match generator_result {
                        Ok(_) => prop_assert_eq!(validation, Ok(())),
                        Err(TextGenError::InsufficientKeys) => prop_assert_eq!(
                            validation,
                            Err(LessonConfigError::InsufficientKeysForGoal)
                        ),
                    }
                }
            }
        }
    }
}
