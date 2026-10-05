//! Lesson/Curriculum 数据结构 + unlock_criteria 判定函数。

use std::collections::{HashMap, HashSet};

use crate::domain::keyboard_layout::KeyCode;

/// 课程唯一标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LessonId(pub String);

/// 课程序列：应用内置的静态课程定义，按难度递增顺序排列（Req 1.1）。
///
/// `Curriculum`/`Lesson` 属于应用内置数据（编译期常量或随应用资源分发的配置），
/// 不属于需要持久化的学员数据，与 `Learner_Profile`/`Learning_Progress` 完全解耦。
#[derive(Debug, Clone, PartialEq)]
pub struct Curriculum {
    /// 按难度递增顺序排列的课程列表。
    pub lessons: Vec<Lesson>,
}

/// 单个课程的定义。
#[derive(Debug, Clone, PartialEq)]
pub struct Lesson {
    pub id: LessonId,
    /// 面向儿童学习者的人类可读课程标题（简体中文，简短一眼可懂），用于
    /// 课程序列界面的卡片标题（Req 1.2）。
    ///
    /// 与 `id`（kebab-case 的稳定内部标识，不面向用户）职责分离：`id` 用于
    /// 数据关联与持久化，`title` 只用于展示，允许在不影响学习进度记录的
    /// 前提下调整文案。标题应侧重"本课涉及的键位范围/学习内容"，避免与
    /// UI 侧由 `goal` 推导的副标题（"单键练习"/"词语练习"/"句子练习"）
    /// 重复表达同一信息。
    pub title: String,
    /// 本课程的学习目标类型（单键 / 词语 / 句子）。
    pub goal: LessonGoal,
    /// 本课程涉及/新增的键位。
    pub target_keys: Vec<KeyCode>,
    /// 本课程的练习内容是否使用这些键位的 Shift 组合字符（大写字母/
    /// `~!@#$%^&*()_+{}|:"<>?`），而不是默认的基础字符。
    ///
    /// 引入原因：`target_keys` 只表示物理键位（`KeyCode`），本身不携带
    /// "是否按 Shift"这一维度——同一个 `KeyCode::Digit1` 既可以是基础课程里
    /// 的字符 `'1'`，也可以是符号课程里的 `'!'`。`practice_text::generate_*`
    /// 系列函数据此字段决定对每个 `target_keys`/`unlocked_keys` 元素调用
    /// `key_to_char`（`false`）还是 `shifted_char`（`true`）。
    ///
    /// 该字段只影响文本生成使用的字符集合，不改变 `target_keys` 本身的物理
    /// 键位含义：虚拟键盘高亮/手指分区提示（Req 2.1, 2.2）仍按同一组
    /// `KeyCode` 计算，Shift 键的高亮由 `current_key_hint` 返回的字符经
    /// `domain::needs_shift` 独立判定（见 `ui::build_key_states`），与本字段
    /// 无耦合。
    pub requires_shift: bool,
    /// 进入下一课程所需满足的可量化解锁条件（Req 1.3）。
    pub unlock_criteria: UnlockCriteria,
}

/// 课程的学习目标类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LessonGoal {
    /// 单键熟悉：练习内容仅由该课程指定按键对应字符组成。
    SingleKey,
    /// 词语练习：练习内容由已解锁键位构成的词语组成。
    Word,
    /// 句子练习：练习内容由已解锁键位构成的句子组成。
    Sentence,
}

/// 课程解锁条件：由准确率、完成时间、练习次数中一项或多项组成（Req 1.3）。
///
/// 各字段为 `None` 表示该项不参与判定；全部为 `None` 视为无门槛（恒满足）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnlockCriteria {
    /// 最低正确率门槛，取值范围 0.0-100.0。
    pub min_accuracy: Option<f32>,
    /// 最长完成用时门槛（秒）。
    pub max_duration_secs: Option<u32>,
    /// 最少练习次数门槛。
    pub min_attempts: Option<u32>,
}

/// 课程在当前学员学习进度下的解锁状态。
#[derive(Debug, Clone, PartialEq)]
pub enum LessonState {
    /// 尚未满足前置课程的解锁条件。
    Locked,
    /// 可进入练习。
    Unlocked,
    /// 课程配置缺失/损坏或存在冲突，无法参与解锁判定（Req 1.8, 8.4）。
    Unavailable(String),
}

/// 单次练习环节的达成结果（领域层引用，供 `meets_unlock_criteria` 判定使用）。
///
/// 与 `storage::PracticeResult`（serde 可序列化持久化结构）字段保持一致，
/// 但领域层不依赖 `serde`/`chrono`，避免纯逻辑层与持久化格式耦合。
#[derive(Debug, Clone, PartialEq)]
pub struct PracticeResult {
    pub lesson_id: LessonId,
    /// 正确率，0.0-100.0，保留1位小数。
    pub accuracy: f32,
    /// 打字速度（WPM），保留1位小数。
    pub wpm: f32,
    pub error_count: u32,
    pub duration_ms: u64,
    pub score: u32,
}

/// 学员在课程序列中的学习进度（领域层引用，供 `compute_lesson_states` 判定使用）。
///
/// 与 `storage::LearningProgress`（serde 可序列化持久化结构）字段保持一致，
/// 但领域层不依赖 `serde`/`chrono`，避免纯逻辑层与持久化格式耦合。
#[derive(Debug, Clone, PartialEq, Default)]
#[allow(non_camel_case_types)] // 命名沿用设计文档 Data Models 中的 `Learning_Progress` 标识
pub struct Learning_Progress {
    /// 每个课程已达成的最佳/历史成绩记录。
    pub lesson_records: HashMap<LessonId, PracticeResult>,
    /// 已解锁课程 id 集合。
    pub unlocked_lesson_ids: HashSet<LessonId>,
}

/// 纯函数：给定某课程的达成结果与其解锁条件，判定是否满足解锁条件（Req 1.4, 1.5）。
///
/// 依据 `min_accuracy`、`max_duration_secs`、`min_attempts` 中已设定（`Some`）的门槛逐一判定：
/// - `min_accuracy`：`result.accuracy` 必须 ≥ 该门槛；
/// - `max_duration_secs`：`result` 的用时（毫秒换算为秒）必须 ≤ 该门槛；
/// - `min_attempts`：练习次数必须 ≥ 该门槛，由调用方通过 `attempt_count` 传入
///   （`PracticeResult` 本身不携带累计练习次数）。
///
/// 值为 `None` 的字段不参与判定；`criteria` 全部字段为 `None` 时恒返回 `true`。
pub fn meets_unlock_criteria(
    result: &PracticeResult,
    criteria: &UnlockCriteria,
    attempt_count: u32,
) -> bool {
    if let Some(min_accuracy) = criteria.min_accuracy
        && result.accuracy < min_accuracy
    {
        return false;
    }

    if let Some(max_duration_secs) = criteria.max_duration_secs {
        let duration_secs = result.duration_ms / 1000;
        if duration_secs > max_duration_secs as u64 {
            return false;
        }
    }

    if let Some(min_attempts) = criteria.min_attempts
        && attempt_count < min_attempts
    {
        return false;
    }

    true
}

/// 纯函数：给定当前学习进度与课程序列，计算每个课程的解锁状态（Req 1.2, 1.4, 1.5, 6.3）。
///
/// 返回值与 `curriculum.lessons` 一一对应、顺序一致（`Vec<LessonState>`，与设计文档签名保持一致）。
///
/// 判定规则：
/// - 第一个课程恒为 `Unlocked`（Req 1.2, 6.3：空/新建进度下第一课可进入）。
/// - 后续课程 `lessons[i]` 是否 `Unlocked`，取决于其前一课程 `lessons[i-1]` 在
///   `progress.lesson_records` 中记录的达成结果是否满足 `lessons[i-1].unlock_criteria`
///   （通过 `meets_unlock_criteria` 判定）；一旦前一课程未达标，判定链条终止，
///   自该课程起（含）后续所有课程保持 `Locked`（课程序列本身是线性递进关系）。
/// - `progress.unlocked_lesson_ids` 中已记录为解锁的课程不会因重新计算而退回 `Locked`
///   （Req 1.4："保留该课程的完成记录"隐含已获得的解锁状态不倒退）。
/// - 本函数不处理 `LessonState::Unavailable`：课程配置校验（Req 1.8, 8.4）由
///   `validate_lesson_config`（`practice_text.rs`）在此之前完成，本函数只感知
///   学习进度维度的 Locked/Unlocked 判定。
pub fn compute_lesson_states(
    curriculum: &Curriculum,
    progress: &Learning_Progress,
) -> Vec<LessonState> {
    let mut states = Vec::with_capacity(curriculum.lessons.len());
    // 一旦前一课程未达标（或本课程之前已经出现过未达标课程），链条断开，
    // 后续课程均保持 Locked，除非其已被记录为曾经解锁（不倒退）。
    let mut chain_unlocked = true;

    for (index, lesson) in curriculum.lessons.iter().enumerate() {
        let already_unlocked = progress.unlocked_lesson_ids.contains(&lesson.id);

        let is_unlocked = if index == 0 {
            // 第一课恒为 Unlocked：空/新建进度下无需任何前置条件（Req 1.2, 6.3）。
            true
        } else if already_unlocked {
            // 已记录为解锁的课程不会因重新计算而退回 Locked（Req 1.4）。
            true
        } else if chain_unlocked {
            let prev_lesson = &curriculum.lessons[index - 1];
            match progress.lesson_records.get(&prev_lesson.id) {
                Some(prev_result) => {
                    // 领域层 `PracticeResult` 不携带累计练习次数，此处以
                    // “存在达成记录”视为至少 1 次练习。
                    meets_unlock_criteria(prev_result, &prev_lesson.unlock_criteria, 1)
                }
                None => false,
            }
        } else {
            false
        };

        if !is_unlocked {
            chain_unlocked = false;
        }

        states.push(if is_unlocked {
            LessonState::Unlocked
        } else {
            LessonState::Locked
        });
    }

    states
}

/// 基准键位（home row）左手区：左手小指→食指的归位起点，含食指向右延伸负责
/// 的 G。
///
/// 为什么包含 G：标准指法里左手食指同时负责 F 与 G，把 G 从这门课里拿掉会
/// 让"食指要覆盖两个键"这件事直到整行连打那一课才第一次出现；而上排左手区
/// （`TOP_ROW_LEFT`）本来就是把食指延伸的 T 一起教的（QWERT），基准键行不
/// 一起教 G 反而与上排的分组方式不一致。
const HOME_ROW_LEFT: &[KeyCode] = &[
    KeyCode::A,
    KeyCode::S,
    KeyCode::D,
    KeyCode::F,
    KeyCode::G,
];

/// 基准键位右手区：右手食指→小指的归位起点，含食指向左延伸负责的 H。
///
/// 与 `HOME_ROW_LEFT` 含 G 是同一条理由、左右对称：标准指法里右手食指同时
/// 负责 J 与 H，正如左手食指同时负责 F 与 G。两只手的基准键课程因此都是
/// 5 个键（`asdfg` / `hjkl;`），"食指要覆盖两个键"这件事在左右手的第一课
/// 就同时建立起来，而不是等到整行连打那一课才第一次出现。
const HOME_ROW_RIGHT: &[KeyCode] = &[
    KeyCode::H,
    KeyCode::J,
    KeyCode::K,
    KeyCode::L,
    KeyCode::Semicolon,
    KeyCode::Quote,
];

/// 完整基准键位行：左手区与右手区的并集。
///
/// 由于 `HOME_ROW_LEFT`/`HOME_ROW_RIGHT` 已各自包含本手食指负责的 G/H，这门课
/// **不再引入任何新键位**，它的意义是把此前分左右手练过的 10 个键放到一起做
/// 整行连打（双手交替与换手节奏）。下方
/// `home_row_full_is_exactly_the_union_of_both_hands` 会校验这一并集关系，
/// 避免改了单手常量却忘了同步这里、导致某个键只在整行课里出现或干脆漏掉。
const HOME_ROW_FULL: &[KeyCode] = &[
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
];

/// 上排左手区（左手小指→食指，含食指延伸的 T）。
const TOP_ROW_LEFT: &[KeyCode] = &[KeyCode::Q, KeyCode::W, KeyCode::E, KeyCode::R, KeyCode::T];

/// 上排右手区（右手食指→小指的字母键）。
///
/// 本行由右手小指负责的 `[`/`]`/`\` **不在**这门课里，而是单列为
/// [`RIGHT_PINKY_SYMBOLS`] 一门后置课程：这三个键要求小指离开基准位向右上方
/// 大幅伸展，是全键盘对初学者最不友好的一簇，与同一行的 YUIOP 放在一课里会让
/// 这门课的难度明显高出相邻课程。拆出去之后它们仍然被课程序列覆盖（由
/// `row_hand_groups_and_symbol_lesson_together_cover_all_three_rows` 校验），
/// 只是被推到三行都练熟之后。
const TOP_ROW_RIGHT: &[KeyCode] = &[KeyCode::Y, KeyCode::U, KeyCode::I, KeyCode::O, KeyCode::P];

/// 右手小指负责的上排符号簇：`[`/`]`/`\`。
///
/// 单列一课的理由见 [`TOP_ROW_RIGHT`]。三个键同属右手小指分区，正好构成一门
/// "同一根手指的伸展练习"。
const RIGHT_PINKY_SYMBOLS: &[KeyCode] = &[
    KeyCode::BracketLeft,
    KeyCode::BracketRight,
    KeyCode::Backslash,
];

/// 下排左手区（左手小指→食指，含食指延伸的 B）。
const BOTTOM_ROW_LEFT: &[KeyCode] = &[KeyCode::Z, KeyCode::X, KeyCode::C, KeyCode::V, KeyCode::B];

/// 下排右手区（右手食指→无名指，含常用标点逗号/句号）。
const BOTTOM_ROW_RIGHT: &[KeyCode] = &[
    KeyCode::N,
    KeyCode::M,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::Slash,
];

/// 26 个字母键（三行字母键位全集），供复习/词语/句子课程使用。
const ALL_LETTER_KEYS: &[KeyCode] = &[
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
    KeyCode::A,
    KeyCode::S,
    KeyCode::D,
    KeyCode::F,
    KeyCode::G,
    KeyCode::H,
    KeyCode::J,
    KeyCode::K,
    KeyCode::L,
    KeyCode::Z,
    KeyCode::X,
    KeyCode::C,
    KeyCode::V,
    KeyCode::B,
    KeyCode::N,
    KeyCode::M,
];

/// 按顺序拼接多个键位分组，得到某一课程的 `target_keys`。
/// 课程序列覆盖的全部键位：26 个字母 + 三个字母行里沿途学过的全部标点
/// （`;` `'` `,` `.` `/` `[` `]` `\`）。
///
/// 用于"全键位大复习"一课：既然前面各行都把本手负责的标点一起教了，复习课就
/// 不应该只回顾字母、把刚学会的标点丢在一边。
const ALL_PRACTICE_KEYS: &[KeyCode] = &[
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
];

/// Shift 组合符号课程涉及的键位：数字行（Shift 后为 `~!@#$%^&*()_+`）与
/// 常用标点键（Shift 后为 `:"<>?`）。
///
/// 选取依据：与你贴的 Apple Magic Keyboard 87 键布局一一对应，覆盖该键盘上
/// 除字母大写（已通过 `ALL_LETTER_KEYS` 的基础字符课程间接练到，大写只是
/// 同一物理键位的 Shift 取值，不需要单列课程）之外的全部 Shift 组合符号。
/// 不包含 `[`/`]`/`\`（Shift 后为 `{`/`}`/`|`）：这三个键已经是
/// [`RIGHT_PINKY_SYMBOLS`] 里公认的高难度键，把它们的 Shift 变体也塞进符号
/// 课程会让这门课的难度显著超出同一阶段的其他课程；留给后续单独扩展。
const SHIFT_SYMBOL_KEYS: &[KeyCode] = &[
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
    KeyCode::Semicolon,
    KeyCode::Quote,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::Slash,
];

fn keys(groups: &[&[KeyCode]]) -> Vec<KeyCode> {
    groups.iter().flat_map(|g| g.iter().copied()).collect()
}

/// 构造一条课程定义的内部辅助函数（`requires_shift` 恒为 `false`，即使用
/// 目标键位的基础字符；需要 Shift 组合字符的课程改用 [`shift_lesson`]）。
fn lesson(
    id: &str,
    title: &str,
    goal: LessonGoal,
    target_keys: Vec<KeyCode>,
    min_accuracy: Option<f32>,
    max_duration_secs: Option<u32>,
    min_attempts: Option<u32>,
) -> Lesson {
    Lesson {
        id: LessonId(id.to_string()),
        title: title.to_string(),
        goal,
        target_keys,
        requires_shift: false,
        unlock_criteria: UnlockCriteria {
            min_accuracy,
            max_duration_secs,
            min_attempts,
        },
    }
}

/// 构造一条"使用 Shift 组合字符"的课程定义（`requires_shift: true`）。
/// 其余语义与 [`lesson`] 完全一致，只是练习内容取 `target_keys` 对应的
/// Shift 组合字符（大写字母 / `~!@#$%^&*()_+{}|:"<>?`）而非基础字符。
fn shift_lesson(
    id: &str,
    title: &str,
    goal: LessonGoal,
    target_keys: Vec<KeyCode>,
    min_accuracy: Option<f32>,
    max_duration_secs: Option<u32>,
    min_attempts: Option<u32>,
) -> Lesson {
    Lesson {
        id: LessonId(id.to_string()),
        title: title.to_string(),
        goal,
        target_keys,
        requires_shift: true,
        unlock_criteria: UnlockCriteria {
            min_accuracy,
            max_duration_secs,
            min_attempts,
        },
    }
}

/// 应用内置的真实课程序列（Req 1.1, 1.3）。
///
/// # 学习路径设计
///
/// 13 个课程构成"单键位认识 → Shift 符号 → 词语 → 句子"的完整递进路径
/// （Req 1.1）：
///
/// 1. **基准键位阶段**（第 1–3 课）：先左手 ASDF、再右手 JKL;，然后合并整行
///    （补齐左右食指延伸负责的 G/H），建立归位手感。
/// 2. **上排阶段**（第 4–6 课）：左手 QWERT、右手 YUIOP，随后一节上排+基准行
///    的巩固课。
/// 3. **下排阶段**（第 7–9 课）：左手 ZXCVB、右手 NM 与常用标点 `,` `.`，
///    随后一节覆盖 26 个字母键的综合复习课。
/// 4. **Shift 组合符号阶段**（第 10 课）：教大写字母与
///    `~!@#$%^&*()_+:"<>?` 这些需要同时按住 Shift 才能输入的符号——物理键位
///    在阶段二、三已经练过，这门课新增的是"按住 Shift 变成另一个字符"这一
///    修饰维度本身。排在词语/句子阶段之前：与其余单键课程一样，本课的学习
///    目标仍是单键熟悉（`LessonGoal::SingleKey`），需满足"全部单键课程排在
///    词语/句子课程之前"的课程序列结构性约束。
/// 5. **词语阶段**（第 11–12 课）：先限定在基准键位范围内的词语练习（引入
///    空格作为词间分隔），再扩展到全部字母键。
/// 6. **句子阶段**（第 13 课）：以全部字母键 + 空格 + 常用标点做句子练习，
///    作为序列终点。
///
/// # `target_keys` 的语义
///
/// `target_keys` 表示"本课程涉及的键位"：既用于虚拟键盘高亮与手指分区提示
/// （Req 2.2），也参与练习文本的字符采样。练习文本实际使用的键位集合是
/// **所有已解锁课程 `target_keys` 的并集**（参见
/// `AppController::unlocked_keys_for_current_profile`），因此新阶段课程只需
/// 列出本阶段引入的键位即可自动获得累积效果；复习/词语/句子类课程则显式
/// 列出其涉及的全部键位（并集去重由调用方语义保证，重复列出无副作用），
/// 以便虚拟键盘正确高亮该课程的练习范围，并让
/// `validate_lesson_config` 能独立判定该课程的可行性。
///
/// # 课程标题（`title`）
///
/// 每个课程都携带一个面向儿童学习者的简体中文标题，作为课程卡片的主标题
/// 展示（Req 1.2）。命名约定：
/// - 侧重"本课的键位范围/学习内容"（如"上排左手 QWERT"），不重复 UI 侧
///   由 `goal` 推导的副标题（"单键练习"/"词语练习"/"句子练习"）；
/// - 长度控制在 6–12 个汉字量级，保证在卡片单行标题（16px 粗体，右侧还需
///   留出锁形标识的空间）内完整可读；
/// - 与 kebab-case 的 `id` 严格区分：`id` 是稳定的内部标识（参与持久化与
///   学习进度关联），`title` 只用于展示，可独立调整文案。
///
/// # 解锁门槛设计
///
/// 面向儿童学习者，门槛随阶段推进温和提高：入门课 80% 正确率，中段 84–88%，
/// 综合复习与词语/句子阶段 88–90%；阶段性关键课程附加 `min_attempts`
/// 让孩子多练几遍以形成肌肉记忆。`max_duration_secs` 仅在倒数第二课使用，
/// 且取宽松值（600 秒，远超一段 20–60 字符文本的正常用时），以避免给孩子
/// 制造时间压力。序列最后一课不设解锁条件（其后没有需要解锁的课程）。
pub fn builtin_curriculum() -> Curriculum {
    Curriculum {
        lessons: vec![
            // ——— 阶段一：基准键位 ———
            lesson(
                "home-row-left-asdfg",
                "左手基准键 ASDFG",
                LessonGoal::SingleKey,
                keys(&[HOME_ROW_LEFT]),
                Some(80.0),
                None,
                Some(2),
            ),
            lesson(
                "home-row-right-hjkl-semicolon",
                "右手基准键 HJKL;'",
                LessonGoal::SingleKey,
                keys(&[HOME_ROW_RIGHT]),
                Some(80.0),
                None,
                Some(2),
            ),
            lesson(
                "home-row-full",
                "基准键整行连打",
                LessonGoal::SingleKey,
                keys(&[HOME_ROW_FULL]),
                Some(82.0),
                None,
                Some(2),
            ),
            // ——— 阶段二：上排 ———
            lesson(
                "top-row-left-qwert",
                "上排左手 QWERT",
                LessonGoal::SingleKey,
                keys(&[TOP_ROW_LEFT]),
                Some(84.0),
                None,
                None,
            ),
            lesson(
                "top-row-right-yuiop",
                "上排右手 YUIOP",
                LessonGoal::SingleKey,
                keys(&[TOP_ROW_RIGHT]),
                Some(84.0),
                None,
                None,
            ),
            lesson(
                "top-row-and-home-row-review",
                "上排加基准键复习",
                LessonGoal::SingleKey,
                keys(&[HOME_ROW_FULL, TOP_ROW_LEFT, TOP_ROW_RIGHT]),
                Some(86.0),
                None,
                Some(2),
            ),
            // ——— 阶段三：下排与字母全集 ———
            lesson(
                "bottom-row-left-zxcvb",
                "下排左手 ZXCVB",
                LessonGoal::SingleKey,
                keys(&[BOTTOM_ROW_LEFT]),
                Some(86.0),
                None,
                None,
            ),
            lesson(
                "bottom-row-right-nm-punctuation",
                "下排右手 NM 和标点 ,./",
                LessonGoal::SingleKey,
                keys(&[BOTTOM_ROW_RIGHT]),
                Some(88.0),
                None,
                None,
            ),
            lesson(
                "right-pinky-symbols",
                "右手小指符号 []\\",
                LessonGoal::SingleKey,
                keys(&[RIGHT_PINKY_SYMBOLS]),
                Some(88.0),
                None,
                None,
            ),
            lesson(
                "all-keys-review",
                "全键位大复习",
                LessonGoal::SingleKey,
                keys(&[ALL_PRACTICE_KEYS]),
                Some(90.0),
                None,
                Some(2),
            ),
            // ——— 阶段四：Shift 组合符号 ———
            //
            // 教大写字母与 `~!@#$%^&*()_+:"<>?` 这些必须按住 Shift 才能输入的
            // 符号。物理键位早在阶段二、三就练过了（数字行、`;'` `,./`），
            // 这门课新增的是"同一个键、按住 Shift、变成另一个字符"这件事本身，
            // 因此单列一课而不是分散补进前面几课，避免把"认识新键位"与
            // "认识 Shift 修饰"两件不同的事混在同一门课里。
            //
            // 排在词语/句子阶段之前（而不是之后）：与其余单键课程一样，本课
            // 的目标是单键熟悉（`LessonGoal::SingleKey`），课程序列的结构性
            // 约束要求全部单键课程排在词语/句子课程之前
            // （`builtin_curriculum_goal_progression_is_single_key_then_word_then_sentence`）。
            shift_lesson(
                "shift-symbols",
                "Shift 组合符号 !@#$%",
                LessonGoal::SingleKey,
                keys(&[SHIFT_SYMBOL_KEYS]),
                Some(86.0),
                None,
                Some(2),
            ),
            // ——— 阶段五：词语 ———
            lesson(
                "word-home-row",
                "用基准键拼词语",
                LessonGoal::Word,
                keys(&[HOME_ROW_FULL, &[KeyCode::Space]]),
                Some(88.0),
                None,
                Some(2),
            ),
            lesson(
                "word-all-letters",
                "全字母拼词语",
                LessonGoal::Word,
                keys(&[ALL_LETTER_KEYS, &[KeyCode::Space]]),
                Some(90.0),
                Some(600),
                None,
            ),
            // ——— 阶段六：句子（序列终点，无需解锁条件） ———
            lesson(
                "sentence-all-letters",
                "完整句子大挑战",
                LessonGoal::Sentence,
                keys(&[
                    ALL_LETTER_KEYS,
                    &[KeyCode::Space, KeyCode::Comma, KeyCode::Period],
                ]),
                None,
                None,
                None,
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result(accuracy: f32, duration_ms: u64) -> PracticeResult {
        PracticeResult {
            lesson_id: LessonId("lesson-1".to_string()),
            accuracy,
            wpm: 0.0,
            error_count: 0,
            duration_ms,
            score: 0,
        }
    }

    #[test]
    fn all_none_criteria_always_satisfied() {
        let criteria = UnlockCriteria {
            min_accuracy: None,
            max_duration_secs: None,
            min_attempts: None,
        };
        let result = sample_result(0.0, 999_999);
        assert!(meets_unlock_criteria(&result, &criteria, 0));
    }

    #[test]
    fn min_accuracy_threshold_boundary() {
        let criteria = UnlockCriteria {
            min_accuracy: Some(90.0),
            max_duration_secs: None,
            min_attempts: None,
        };
        assert!(meets_unlock_criteria(&sample_result(90.0, 0), &criteria, 0));
        assert!(meets_unlock_criteria(&sample_result(95.0, 0), &criteria, 0));
        assert!(!meets_unlock_criteria(
            &sample_result(89.9, 0),
            &criteria,
            0
        ));
    }

    #[test]
    fn max_duration_secs_threshold_boundary() {
        let criteria = UnlockCriteria {
            min_accuracy: None,
            max_duration_secs: Some(60),
            min_attempts: None,
        };
        assert!(meets_unlock_criteria(
            &sample_result(0.0, 60_000),
            &criteria,
            0
        ));
        assert!(meets_unlock_criteria(
            &sample_result(0.0, 30_000),
            &criteria,
            0
        ));
        assert!(!meets_unlock_criteria(
            &sample_result(0.0, 61_000),
            &criteria,
            0
        ));
    }

    #[test]
    fn min_attempts_threshold_boundary() {
        let criteria = UnlockCriteria {
            min_accuracy: None,
            max_duration_secs: None,
            min_attempts: Some(3),
        };
        assert!(meets_unlock_criteria(&sample_result(0.0, 0), &criteria, 3));
        assert!(meets_unlock_criteria(&sample_result(0.0, 0), &criteria, 4));
        assert!(!meets_unlock_criteria(&sample_result(0.0, 0), &criteria, 2));
    }

    #[test]
    fn all_thresholds_must_hold_simultaneously() {
        let criteria = UnlockCriteria {
            min_accuracy: Some(90.0),
            max_duration_secs: Some(60),
            min_attempts: Some(1),
        };
        // 全部达标
        assert!(meets_unlock_criteria(
            &sample_result(95.0, 50_000),
            &criteria,
            1
        ));
        // 仅正确率未达标
        assert!(!meets_unlock_criteria(
            &sample_result(80.0, 50_000),
            &criteria,
            1
        ));
        // 仅用时未达标
        assert!(!meets_unlock_criteria(
            &sample_result(95.0, 70_000),
            &criteria,
            1
        ));
        // 仅练习次数未达标
        assert!(!meets_unlock_criteria(
            &sample_result(95.0, 50_000),
            &criteria,
            0
        ));
    }
}

#[cfg(test)]
mod compute_lesson_states_tests {
    use super::*;

    fn lesson(id: &str, min_accuracy: Option<f32>) -> Lesson {
        Lesson {
            id: LessonId(id.to_string()),
            title: format!("课程 {id}"),
            goal: LessonGoal::SingleKey,
            target_keys: vec![KeyCode::A],
            requires_shift: false,
            unlock_criteria: UnlockCriteria {
                min_accuracy,
                max_duration_secs: None,
                min_attempts: None,
            },
        }
    }

    fn result(lesson_id: &str, accuracy: f32) -> PracticeResult {
        PracticeResult {
            lesson_id: LessonId(lesson_id.to_string()),
            accuracy,
            wpm: 0.0,
            error_count: 0,
            duration_ms: 0,
            score: 0,
        }
    }

    fn sample_curriculum() -> Curriculum {
        Curriculum {
            lessons: vec![
                lesson("lesson-1", Some(90.0)),
                lesson("lesson-2", Some(90.0)),
                lesson("lesson-3", Some(90.0)),
            ],
        }
    }

    #[test]
    fn empty_progress_only_unlocks_first_lesson() {
        let curriculum = sample_curriculum();
        let progress = Learning_Progress::default();

        let states = compute_lesson_states(&curriculum, &progress);

        assert_eq!(
            states,
            vec![
                LessonState::Unlocked,
                LessonState::Locked,
                LessonState::Locked
            ]
        );
    }

    #[test]
    fn single_lesson_curriculum_first_lesson_unlocked() {
        let curriculum = Curriculum {
            lessons: vec![lesson("only-lesson", Some(90.0))],
        };
        let progress = Learning_Progress::default();

        let states = compute_lesson_states(&curriculum, &progress);

        assert_eq!(states, vec![LessonState::Unlocked]);
    }

    #[test]
    fn meeting_unlock_criteria_advances_unlock_boundary() {
        let curriculum = sample_curriculum();
        let mut progress = Learning_Progress::default();
        progress
            .lesson_records
            .insert(LessonId("lesson-1".to_string()), result("lesson-1", 95.0));

        let states = compute_lesson_states(&curriculum, &progress);

        assert_eq!(
            states,
            vec![
                LessonState::Unlocked,
                LessonState::Unlocked,
                LessonState::Locked
            ]
        );
        // 达标课程的成绩记录应被保留（未被本函数清除）。
        assert!(
            progress
                .lesson_records
                .contains_key(&LessonId("lesson-1".to_string()))
        );
    }

    #[test]
    fn failing_unlock_criteria_keeps_next_lesson_locked_but_keeps_record() {
        let curriculum = sample_curriculum();
        let mut progress = Learning_Progress::default();
        progress
            .lesson_records
            .insert(LessonId("lesson-1".to_string()), result("lesson-1", 50.0));

        let states = compute_lesson_states(&curriculum, &progress);

        assert_eq!(
            states,
            vec![
                LessonState::Unlocked,
                LessonState::Locked,
                LessonState::Locked
            ]
        );
        // 未达标课程的成绩记录仍需保留，允许学员重新练习（Req 1.5）。
        let record = progress
            .lesson_records
            .get(&LessonId("lesson-1".to_string()))
            .expect("未达标课程的成绩记录应被保留");
        assert_eq!(record.accuracy, 50.0);
    }

    #[test]
    fn previously_unlocked_lesson_does_not_regress_to_locked() {
        let curriculum = sample_curriculum();
        let mut progress = Learning_Progress::default();
        // lesson-2 之前已被记录为解锁（例如课程配置调整后 unlock_criteria 变严格），
        // 但 lesson-1 目前没有任何达成记录。
        progress
            .unlocked_lesson_ids
            .insert(LessonId("lesson-2".to_string()));

        let states = compute_lesson_states(&curriculum, &progress);

        assert_eq!(states[0], LessonState::Unlocked);
        assert_eq!(states[1], LessonState::Unlocked);
        assert_eq!(states[2], LessonState::Locked);
    }

    #[test]
    fn chain_breaks_at_first_unmet_lesson_even_if_later_lesson_has_record() {
        let curriculum = sample_curriculum();
        let mut progress = Learning_Progress::default();
        // lesson-1 未达标，但 lesson-2 却存在一条达标记录（数据异常/回退场景）。
        // lesson-3 不应因 lesson-2 的记录而被解锁，因为 lesson-2 本身仍是 Locked。
        progress
            .lesson_records
            .insert(LessonId("lesson-1".to_string()), result("lesson-1", 10.0));
        progress
            .lesson_records
            .insert(LessonId("lesson-2".to_string()), result("lesson-2", 99.0));

        let states = compute_lesson_states(&curriculum, &progress);

        assert_eq!(
            states,
            vec![
                LessonState::Unlocked,
                LessonState::Locked,
                LessonState::Locked
            ]
        );
    }
}

#[cfg(test)]
mod compute_lesson_states_property_tests {
    use super::*;
    use proptest::prelude::*;

    /// 生成任意 `UnlockCriteria`：各门槛字段独立地为 `None` 或 `Some(合法值)`，
    /// 覆盖“单项门槛”“多项门槛组合”“无门槛（全 None）”等各种配置。
    fn arb_unlock_criteria() -> impl Strategy<Value = UnlockCriteria> {
        (
            proptest::option::of(0.0f32..=100.0f32),
            proptest::option::of(1u32..=600u32),
            proptest::option::of(1u32..=20u32),
        )
            .prop_map(
                |(min_accuracy, max_duration_secs, min_attempts)| UnlockCriteria {
                    min_accuracy,
                    max_duration_secs,
                    min_attempts,
                },
            )
    }

    /// 生成任意单个 `Lesson`：`id` 由索引推导（保证同一课程序列内唯一），
    /// `goal`/`target_keys` 对本属性无关紧要，固定为最简取值，
    /// `unlock_criteria` 任意生成。
    fn arb_lesson(index: usize) -> impl Strategy<Value = Lesson> {
        arb_unlock_criteria().prop_map(move |unlock_criteria| Lesson {
            id: LessonId(format!("lesson-{index}")),
            title: format!("课程 {index}"),
            goal: LessonGoal::SingleKey,
            target_keys: vec![KeyCode::A],
            requires_shift: false,
            unlock_criteria,
        })
    }

    /// 生成任意 `Curriculum`：课程数量 1..=10，每个课程的解锁条件独立随机生成。
    fn arb_curriculum() -> impl Strategy<Value = Curriculum> {
        (1usize..=10).prop_flat_map(|lesson_count| {
            let lessons: Vec<_> = (0..lesson_count).map(arb_lesson).collect();
            lessons.prop_map(|lessons| Curriculum { lessons })
        })
    }

    // Feature: typing-desktop-app, Property 1: 空进度初始解锁状态
    //
    // 对于任意课程序列（长度 ≥1）与任意新建/空的学习进度，计算出的课程解锁状态中，
    // 序列第一个课程状态为 Unlocked，其余课程状态均为 Locked。
    //
    // Validates: Requirements 1.2, 6.3
    proptest! {
        #[test]
        fn prop_empty_progress_unlocks_only_first_lesson(
            curriculum in arb_curriculum(),
        ) {
            let progress = Learning_Progress::default();

            let states = compute_lesson_states(&curriculum, &progress);

            prop_assert_eq!(states.len(), curriculum.lessons.len());
            prop_assert_eq!(states.first(), Some(&LessonState::Unlocked));
            for state in states.iter().skip(1) {
                prop_assert_eq!(state, &LessonState::Locked);
            }
        }
    }

    /// 生成任意 `PracticeResult`：`lesson_id` 由调用处覆盖为具体课程 id，
    /// `accuracy`/`duration_ms` 覆盖能够触发 `min_accuracy`/`max_duration_secs`
    /// 门槛两侧取值（含边界）的范围。
    fn arb_practice_result() -> impl Strategy<Value = PracticeResult> {
        (0.0f32..=100.0f32, 0u64..=700_000u64, 0u32..=50u32).prop_map(
            |(accuracy, duration_ms, error_count)| PracticeResult {
                lesson_id: LessonId("lesson-0".to_string()),
                accuracy,
                wpm: 0.0,
                error_count,
                duration_ms,
                score: 0,
            },
        )
    }

    // Feature: typing-desktop-app, Property 2: 解锁条件判定与状态推进的一致性
    //
    // 对于任意课程序列（≥2 个课程）与任意针对第一个课程记录的 `PracticeResult`：
    // `compute_lesson_states` 是否解锁第二个课程，与直接调用
    // `meets_unlock_criteria(result, 第一课.unlock_criteria, 1)` 的判定结果恒一致
    // （`compute_lesson_states` 对“存在达成记录”的课程视为至少 1 次练习，
    // 参见其函数文档），即状态推进逻辑不与解锁条件判定函数分离漂移；
    // 无论结果是否达标，第一个课程的成绩记录都会被保留在学习进度中。
    //
    // Validates: Requirements 1.4, 1.5
    proptest! {
        #[test]
        fn prop_unlock_state_progression_matches_criteria_judgement(
            curriculum in arb_curriculum().prop_filter(
                "至少需要 2 个课程才能观察“下一课程”的解锁推进",
                |c| c.lessons.len() >= 2,
            ),
            mut result in arb_practice_result(),
        ) {
            let first_lesson = curriculum.lessons[0].clone();
            result.lesson_id = first_lesson.id.clone();

            let expected_unlocked =
                meets_unlock_criteria(&result, &first_lesson.unlock_criteria, 1);

            let mut progress = Learning_Progress::default();
            progress.lesson_records.insert(first_lesson.id.clone(), result.clone());

            let states = compute_lesson_states(&curriculum, &progress);

            let second_lesson_state = &states[1];
            if expected_unlocked {
                prop_assert_eq!(second_lesson_state, &LessonState::Unlocked);
            } else {
                prop_assert_eq!(second_lesson_state, &LessonState::Locked);
            }

            // 无论解锁与否，第一个课程的成绩记录都必须被保留（Req 1.4, 1.5）。
            let preserved_record = progress
                .lesson_records
                .get(&first_lesson.id)
                .expect("第一个课程的成绩记录应始终被保留");
            prop_assert_eq!(preserved_record, &result);
        }
    }
}

/// Req 1.1, 1.3 结构校验测试：内置课程序列静态数据的一次性结构校验。
///
/// 本模块既校验设计文档 Testing Strategy 中约定的**结构规则**（数量 ≥1、
/// 每个需解锁课程的 `unlock_criteria` 字段完整/非空），也把这些规则应用到
/// 应用真实的内置课程序列 [`builtin_curriculum`] 上：
/// `assert_valid_curriculum` 的校验逻辑本身与具体课程内容无关，因此真实
/// 数据与示例数据共用同一套校验函数。
#[cfg(test)]
mod builtin_curriculum_structure_tests {
    use super::*;
    use crate::domain::practice_text::validate_lesson_config;
    use std::collections::HashSet;

    /// 校验任意课程序列是否满足 Req 1.1、1.3 规定的结构性约束：
    ///
    /// - Req 1.1：课程总数不少于 1 个。
    /// - Req 1.3：序列中每个"需要解锁的课程"（即除最后一课外的所有课程——
    ///   最后一课之后没有下一课需要被解锁，因此不强制要求其携带解锁条件）都
    ///   必须定义一个由准确率、完成时间、练习次数中一项或多项组成的可量化
    ///   解锁条件，即 `unlock_criteria` 的三个字段中至少一个为 `Some(..)`，
    ///   不允许三者全部为 `None`（全 `None` 等价于"无门槛"，不构成 Req 1.3
    ///   所要求的可量化解锁条件）。
    ///
    /// 校验失败时返回具体原因，便于测试断言与未来接入真实课程数据时快速定位。
    fn assert_valid_curriculum(curriculum: &Curriculum) -> Result<(), String> {
        if curriculum.lessons.is_empty() {
            return Err("课程序列不能为空：课程总数必须不少于1个（Req 1.1）".to_string());
        }

        let last_index = curriculum.lessons.len() - 1;
        for (index, lesson) in curriculum.lessons.iter().enumerate() {
            // 序列最后一课之后没有"下一课"需要解锁，不强制要求其携带解锁条件；
            // 其余所有课程都需要一个可量化解锁条件才能让后续课程解锁（Req 1.3）。
            if index == last_index {
                continue;
            }

            let criteria = &lesson.unlock_criteria;
            let has_quantifiable_criteria = criteria.min_accuracy.is_some()
                || criteria.max_duration_secs.is_some()
                || criteria.min_attempts.is_some();

            if !has_quantifiable_criteria {
                return Err(format!(
                    "课程 {:?} 缺少可量化解锁条件：min_accuracy/max_duration_secs/min_attempts \
                     不能全部为 None（Req 1.3）",
                    lesson.id
                ));
            }
        }

        Ok(())
    }

    fn lesson_with_criteria(id: &str, criteria: UnlockCriteria) -> Lesson {
        Lesson {
            id: LessonId(id.to_string()),
            title: format!("课程 {id}"),
            goal: LessonGoal::SingleKey,
            target_keys: vec![KeyCode::A],
            requires_shift: false,
            unlock_criteria: criteria,
        }
    }

    fn full_criteria() -> UnlockCriteria {
        UnlockCriteria {
            min_accuracy: Some(90.0),
            max_duration_secs: Some(120),
            min_attempts: Some(1),
        }
    }

    fn empty_criteria() -> UnlockCriteria {
        UnlockCriteria {
            min_accuracy: None,
            max_duration_secs: None,
            min_attempts: None,
        }
    }

    /// 代表性的示例课程序列：模拟一个从单键 → 词语 → 句子的完整学习路径
    /// （覆盖 Req 1.1"从单个键位认识到词语、句子输入的完整学习路径"的结构形态），
    /// 每个需解锁课程均携带一项可量化解锁条件，用于演示 `assert_valid_curriculum`
    /// 在结构合法的课程序列上校验通过。
    fn representative_sample_curriculum() -> Curriculum {
        Curriculum {
            lessons: vec![
                lesson_with_criteria(
                    "lesson-single-key-home-row",
                    UnlockCriteria {
                        min_accuracy: Some(85.0),
                        max_duration_secs: None,
                        min_attempts: None,
                    },
                ),
                lesson_with_criteria(
                    "lesson-word-practice",
                    UnlockCriteria {
                        min_accuracy: None,
                        max_duration_secs: None,
                        min_attempts: Some(3),
                    },
                ),
                // 序列最后一课：无需携带解锁条件（没有下一课需要被它解锁）。
                lesson_with_criteria("lesson-sentence-practice", empty_criteria()),
            ],
        }
    }

    #[test]
    fn representative_curriculum_has_at_least_one_lesson() {
        let curriculum = representative_sample_curriculum();
        assert!(
            !curriculum.lessons.is_empty(),
            "课程总数必须不少于1个（Req 1.1）"
        );
    }

    #[test]
    fn representative_curriculum_passes_structural_validation() {
        let curriculum = representative_sample_curriculum();
        assert_eq!(assert_valid_curriculum(&curriculum), Ok(()));
    }

    #[test]
    fn single_lesson_curriculum_is_structurally_valid() {
        // 单课程序列：唯一课程即为"最后一课"，不强制要求解锁条件，仍满足
        // Req 1.1（数量≥1）。
        let curriculum = Curriculum {
            lessons: vec![lesson_with_criteria("only-lesson", empty_criteria())],
        };
        assert_eq!(assert_valid_curriculum(&curriculum), Ok(()));
    }

    #[test]
    fn empty_curriculum_fails_minimum_count_check() {
        let curriculum = Curriculum { lessons: vec![] };
        let result = assert_valid_curriculum(&curriculum);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("不少于1个"));
    }

    #[test]
    fn non_final_lesson_missing_unlock_criteria_fails_validation() {
        // lesson-1 是非最后一课，但三个解锁条件字段全为 None——缺少可量化
        // 解锁条件（Req 1.3），应被判定为结构非法。
        let curriculum = Curriculum {
            lessons: vec![
                lesson_with_criteria("lesson-1", empty_criteria()),
                lesson_with_criteria("lesson-2", full_criteria()),
            ],
        };
        let result = assert_valid_curriculum(&curriculum);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("lesson-1"));
    }

    #[test]
    fn final_lesson_may_omit_unlock_criteria() {
        // lesson-2 是最后一课，即使不携带任何解锁条件也应校验通过。
        let curriculum = Curriculum {
            lessons: vec![
                lesson_with_criteria("lesson-1", full_criteria()),
                lesson_with_criteria("lesson-2", empty_criteria()),
            ],
        };
        assert_eq!(assert_valid_curriculum(&curriculum), Ok(()));
    }

    #[test]
    fn non_final_lesson_with_any_single_criterion_field_passes() {
        // Req 1.3："由准确率、完成时间、练习次数中一项或多项组成"——
        // 仅设置其中任意一项即应视为满足要求，逐一验证三个字段各自单独设置的情况。
        for criteria in [
            UnlockCriteria {
                min_accuracy: Some(80.0),
                max_duration_secs: None,
                min_attempts: None,
            },
            UnlockCriteria {
                min_accuracy: None,
                max_duration_secs: Some(60),
                min_attempts: None,
            },
            UnlockCriteria {
                min_accuracy: None,
                max_duration_secs: None,
                min_attempts: Some(2),
            },
        ] {
            let curriculum = Curriculum {
                lessons: vec![
                    lesson_with_criteria("lesson-1", criteria),
                    lesson_with_criteria("lesson-2", empty_criteria()),
                ],
            };
            assert_eq!(
                assert_valid_curriculum(&curriculum),
                Ok(()),
                "解锁条件字段 {criteria:?} 应被视为合法的可量化解锁条件"
            );
        }
    }

    // ——— 真实内置课程序列（`builtin_curriculum`）的校验 ———

    #[test]
    fn builtin_curriculum_passes_structural_validation() {
        // Req 1.1（课程数 ≥1）、Req 1.3（除最后一课外每课都有可量化解锁条件）。
        let curriculum = builtin_curriculum();
        assert_eq!(assert_valid_curriculum(&curriculum), Ok(()));
    }

    #[test]
    fn builtin_curriculum_lesson_count_is_within_designed_range() {
        let curriculum = builtin_curriculum();
        assert!(
            !curriculum.lessons.is_empty(),
            "课程总数必须不少于1个（Req 1.1）"
        );
        // 课程序列需要形成有梯度的完整学习路径，单键→词语→句子至少需要若干阶段。
        assert!(
            curriculum.lessons.len() >= 8,
            "内置课程序列应至少包含 8 个课程以形成合理的难度梯度，实际 {}",
            curriculum.lessons.len()
        );
    }

    #[test]
    fn builtin_curriculum_lesson_ids_are_unique() {
        let curriculum = builtin_curriculum();
        let unique: HashSet<&LessonId> = curriculum.lessons.iter().map(|l| &l.id).collect();
        assert_eq!(
            unique.len(),
            curriculum.lessons.len(),
            "课程 id 必须互不重复（解锁状态与成绩记录以 id 为键）"
        );
    }

    /// Req 1.2 回归保护：课程卡片主标题直接取 `Lesson::title`，因此内置课程
    /// 序列中的标题必须是**面向儿童的人类可读文案**，不能为空、不能重复、
    /// 更不能是 kebab-case 的课程 id（后者对儿童完全不可读）。
    #[test]
    fn every_builtin_lesson_has_non_empty_human_readable_title() {
        let curriculum = builtin_curriculum();
        for lesson in &curriculum.lessons {
            assert!(
                !lesson.title.trim().is_empty(),
                "课程 {:?} 缺少人类可读标题（Req 1.2）",
                lesson.id
            );
            assert_ne!(
                lesson.title, lesson.id.0,
                "课程 {:?} 的标题不能直接使用 kebab-case 的课程 id——id 是内部标识，\
                 对儿童不可读（Req 1.2）",
                lesson.id
            );
        }
    }

    #[test]
    fn builtin_curriculum_lesson_titles_are_unique() {
        let curriculum = builtin_curriculum();
        let unique: HashSet<&str> = curriculum
            .lessons
            .iter()
            .map(|l| l.title.as_str())
            .collect();
        assert_eq!(
            unique.len(),
            curriculum.lessons.len(),
            "课程标题必须互不重复，否则学员无法从课程卡片区分不同课程（Req 1.2）"
        );
    }

    /// 最关键的回归保护：真实内置课程序列中的每一课都必须能通过
    /// `validate_lesson_config`，否则 `CurriculumState` 会把它标记为
    /// `LessonState::Unavailable`，该课程将永久不可进入（Req 1.8, 8.4）。
    #[test]
    fn every_builtin_lesson_passes_validate_lesson_config() {
        let curriculum = builtin_curriculum();
        for lesson in &curriculum.lessons {
            assert_eq!(
                validate_lesson_config(lesson),
                Ok(()),
                "课程 {:?} 无法生成满足其学习目标的练习内容，会被标记为 Unavailable",
                lesson.id
            );
        }
    }

    #[test]
    fn every_builtin_lesson_has_non_empty_target_keys() {
        let curriculum = builtin_curriculum();
        for lesson in &curriculum.lessons {
            assert!(
                !lesson.target_keys.is_empty(),
                "课程 {:?} 必须至少指定一个目标键位（Req 1.8）",
                lesson.id
            );
        }
    }

    #[test]
    fn builtin_curriculum_goal_progression_is_single_key_then_word_then_sentence() {
        let curriculum = builtin_curriculum();
        let goals: Vec<LessonGoal> = curriculum.lessons.iter().map(|l| l.goal).collect();

        // Req 1.1：完整学习路径必须同时覆盖单键、词语、句子三类目标。
        assert!(
            goals.contains(&LessonGoal::SingleKey),
            "课程序列必须包含单键认识课程"
        );
        assert!(
            goals.contains(&LessonGoal::Word),
            "课程序列必须包含词语练习课程"
        );
        assert!(
            goals.contains(&LessonGoal::Sentence),
            "课程序列必须包含句子练习课程"
        );

        // 递进性：所有单键课程都排在词语/句子课程之前；所有词语课程都排在
        // 句子课程之前。
        let last_single_key = goals
            .iter()
            .rposition(|&g| g == LessonGoal::SingleKey)
            .expect("已断言存在单键课程");
        let first_word = goals
            .iter()
            .position(|&g| g == LessonGoal::Word)
            .expect("已断言存在词语课程");
        let last_word = goals
            .iter()
            .rposition(|&g| g == LessonGoal::Word)
            .expect("已断言存在词语课程");
        let first_sentence = goals
            .iter()
            .position(|&g| g == LessonGoal::Sentence)
            .expect("已断言存在句子课程");

        assert!(
            last_single_key < first_word,
            "全部单键课程应排在词语课程之前：last_single_key={last_single_key}, first_word={first_word}"
        );
        assert!(
            last_word < first_sentence,
            "全部词语课程应排在句子课程之前：last_word={last_word}, first_sentence={first_sentence}"
        );
    }

    #[test]
    fn builtin_curriculum_new_profile_unlocks_only_first_lesson() {
        // Req 1.2, 6.3：新建档案（空进度）下只有第一课可进入。
        let curriculum = builtin_curriculum();
        let states = compute_lesson_states(&curriculum, &Learning_Progress::default());

        assert_eq!(states.len(), curriculum.lessons.len());
        assert_eq!(states[0], LessonState::Unlocked);
        for (index, state) in states.iter().enumerate().skip(1) {
            assert_eq!(
                state,
                &LessonState::Locked,
                "课程 {:?}（索引 {index}）在空进度下应为 Locked",
                curriculum.lessons[index].id
            );
        }
    }
}

#[cfg(test)]
mod builtin_lesson_scope_tests {
    use super::*;
    use crate::domain::keyboard_layout::{key_to_char, shifted_char};
    use crate::domain::practice_text::generate_practice_text;

    /// 基准键行的分组：左手 `asdfg`、右手 `hjkl;`，各含本手食指负责的两个键
    /// （左食指 F+G、右食指 J+H），左右完全对称。这两组是学员接触键盘的第一
    /// 课，分组一旦改动会直接改变整条学习路径，因此固定下来。
    #[test]
    fn home_row_lessons_are_grouped_as_asdfg_and_hjkl_semicolon() {
        let curriculum = builtin_curriculum();

        let left = &curriculum.lessons[0];
        let left_chars: String = left.target_keys.iter().map(|&k| key_to_char(k)).collect();
        assert_eq!(left_chars, "asdfg");

        let right = &curriculum.lessons[1];
        let right_chars: String = right.target_keys.iter().map(|&k| key_to_char(k)).collect();
        assert_eq!(right_chars, "hjkl;'");
    }

    /// 三个字母行的物理键位顺序（与虚拟键盘的渲染顺序一致）。
    const TOP_ROW: &[KeyCode] = &[
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
    ];
    const HOME_ROW: &[KeyCode] = &[
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
    ];
    const BOTTOM_ROW: &[KeyCode] = &[
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
    ];

    fn is_left_hand(key: KeyCode) -> bool {
        use crate::domain::keyboard_layout::{FingerZone, finger_zone_of};
        matches!(
            finger_zone_of(key),
            FingerZone::LeftPinky
                | FingerZone::LeftRing
                | FingerZone::LeftMiddle
                | FingerZone::LeftIndex
                | FingerZone::LeftThumb
        )
    }

    fn sorted_chars(keys: &[KeyCode]) -> Vec<char> {
        let mut chars: Vec<char> = keys.iter().map(|&k| key_to_char(k)).collect();
        chars.sort_unstable();
        chars
    }

    /// 「某一行 + 某一只手」的课程，其键位必须全部属于该物理行、且全部由这只手
    /// 负责——由 `finger_zone_of` 判定，不允许把另一只手的键混进来。
    ///
    /// 这条不变式把"按手指操作分组"从口头约定变成可自动验证的约束。此前三门
    /// 右手课程都恰好漏掉了右手小指负责的键（基准键行漏 `'`、上排漏 `[`/`]`/`\`、
    /// 下排漏 `/`），左手课程则一度漏掉左手食指延伸的 `G`——这类遗漏的共同后果
    /// 是某根手指的键位被拆散或干脆整套课程都不覆盖，而它们既不会导致编译错误，
    /// 也不会让任何既有测试失败。
    #[test]
    fn each_row_hand_lesson_stays_within_its_row_and_hand() {
        let cases: [(&str, &[KeyCode], &[KeyCode], bool); 6] = [
            ("基准键左手", HOME_ROW_LEFT, HOME_ROW, true),
            ("基准键右手", HOME_ROW_RIGHT, HOME_ROW, false),
            ("上排左手", TOP_ROW_LEFT, TOP_ROW, true),
            ("上排右手", TOP_ROW_RIGHT, TOP_ROW, false),
            ("下排左手", BOTTOM_ROW_LEFT, BOTTOM_ROW, true),
            ("下排右手", BOTTOM_ROW_RIGHT, BOTTOM_ROW, false),
        ];

        for (name, group, row, left_hand) in cases {
            for &key in group {
                assert!(
                    row.contains(&key),
                    "{name} 分组里的 {:?} 不属于这一行",
                    key_to_char(key)
                );
                assert_eq!(
                    is_left_hand(key),
                    left_hand,
                    "{name} 分组里的 {:?} 由另一只手负责",
                    key_to_char(key)
                );
            }
        }
    }

    /// 六个「行 + 手」分组加上右手小指符号课，必须**不重不漏**地覆盖三个字母行
    /// 的全部键位。
    ///
    /// 「不漏」保证没有任何键位从课程序列里掉出去（把 `[`/`]`/`\` 从上排右手课
    /// 拆成单独一课时，最容易犯的错就是拆出去却忘了加课）；「不重」保证同一个
    /// 键位不会在两门课里都作为新键位引入。
    #[test]
    fn row_hand_groups_and_symbol_lesson_together_cover_all_three_rows() {
        let mut covered: Vec<KeyCode> = Vec::new();
        for group in [
            HOME_ROW_LEFT,
            HOME_ROW_RIGHT,
            TOP_ROW_LEFT,
            TOP_ROW_RIGHT,
            BOTTOM_ROW_LEFT,
            BOTTOM_ROW_RIGHT,
            RIGHT_PINKY_SYMBOLS,
        ] {
            covered.extend_from_slice(group);
        }

        // 不重：任何键位只被引入一次。
        let mut seen = std::collections::HashSet::new();
        for &key in &covered {
            assert!(
                seen.insert(key),
                "键位 {:?} 在多个分组里被重复引入",
                key_to_char(key)
            );
        }

        // 不漏：并集恰好等于三行的全部键位。
        let mut all_rows: Vec<KeyCode> = Vec::new();
        all_rows.extend_from_slice(TOP_ROW);
        all_rows.extend_from_slice(HOME_ROW);
        all_rows.extend_from_slice(BOTTOM_ROW);
        assert_eq!(sorted_chars(&covered), sorted_chars(&all_rows));

        // 且课程序列里确实存在承载符号簇的那门课。
        let curriculum = builtin_curriculum();
        let symbol_lesson = curriculum
            .lessons
            .iter()
            .find(|lesson| lesson.id == LessonId("right-pinky-symbols".to_string()))
            .expect("拆出的符号簇必须有一门课程承载，否则这些键位无人教");
        assert_eq!(
            sorted_chars(&symbol_lesson.target_keys),
            sorted_chars(RIGHT_PINKY_SYMBOLS)
        );
    }

    /// 「全键位大复习」必须复习到沿途教过的**所有**键位（字母 + 标点），而不是
    /// 只回顾 26 个字母、把刚学会的标点丢在一边。
    #[test]
    fn final_review_lesson_covers_every_key_taught_before_it() {
        let curriculum = builtin_curriculum();
        let review_index = curriculum
            .lessons
            .iter()
            .position(|lesson| lesson.id == LessonId("all-keys-review".to_string()))
            .expect("课程序列应包含全键位大复习");

        // 之前所有单键课程引入过的键位。
        let mut taught: Vec<KeyCode> = Vec::new();
        for lesson in &curriculum.lessons[..review_index] {
            taught.extend_from_slice(&lesson.target_keys);
        }
        let review = &curriculum.lessons[review_index];

        for &key in &taught {
            assert!(
                review.target_keys.contains(&key),
                "复习课漏掉了此前教过的键位 {:?}",
                key_to_char(key)
            );
        }
    }

    /// 整行连打课的键位集合必须恰好是左右手两课的并集：既不多出只在整行课里
    /// 才第一次出现的键（那样学员会在"复习"课上遇到没学过的键），也不遗漏任何
    /// 已学过的键。
    #[test]
    fn home_row_full_is_exactly_the_union_of_both_hands() {
        let curriculum = builtin_curriculum();

        let mut union: Vec<KeyCode> = curriculum.lessons[0]
            .target_keys
            .iter()
            .chain(curriculum.lessons[1].target_keys.iter())
            .copied()
            .collect();
        let mut full = curriculum.lessons[2].target_keys.clone();

        let mut union_chars: Vec<char> = union.drain(..).map(key_to_char).collect();
        let mut full_chars: Vec<char> = full.drain(..).map(key_to_char).collect();
        union_chars.sort_unstable();
        full_chars.sort_unstable();

        assert_eq!(full_chars, union_chars);
    }

    /// Req 8.1 在**真实内置课程**上的端到端校验：对每一门单键熟悉课程，即使
    /// 把整套课程的键位都当作"已解锁"，生成的练习文本也只能由该课自己的目标
    /// 键位组成。
    #[test]
    fn every_single_key_lesson_generates_text_within_its_own_keys() {
        let curriculum = builtin_curriculum();
        let all_keys: Vec<KeyCode> = curriculum
            .lessons
            .iter()
            .flat_map(|lesson| lesson.target_keys.iter().copied())
            .collect();

        for lesson in &curriculum.lessons {
            if lesson.goal != LessonGoal::SingleKey {
                continue;
            }
            let allowed: Vec<char> = lesson
                .target_keys
                .iter()
                .map(|&k| {
                    if lesson.requires_shift {
                        shifted_char(k).unwrap_or_else(|| key_to_char(k))
                    } else {
                        key_to_char(k)
                    }
                })
                .collect();
            for seed in 0..8u64 {
                let text = generate_practice_text(
                    lesson.goal,
                    &lesson.target_keys,
                    &all_keys,
                    lesson.requires_shift,
                    seed,
                )
                .expect("单键文本生成应成功");
                for ch in text.chars() {
                    assert!(
                        allowed.contains(&ch),
                        "课程 {:?} (seed={seed}) 的练习文本出现了本课之外的字符 {ch:?}",
                        lesson.id
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod highlight_and_zone_tests {
    use super::*;
    // 仅测试使用：课程定义本身不需要手指分区（`Lesson` 只持有 `KeyCode`），
    // 但"课程涉及键位的分区归属"这条跨模块一致性断言需要它们。
    use crate::domain::keyboard_layout::{FingerZone, finger_zone_of};

    /// Req 2.2：学员进入以键位认识为目标的课程时，虚拟键盘需要同时具备
    /// “高亮显示当前课程涉及的按键”（键位集合，来自 `Lesson::target_keys`）与
    /// “显示每个按键所属的手指分区标识”（分区标签，来自 `finger_zone_of`）
    /// 两类数据。这两类数据均可直接从既有的 `Lesson`/`keyboard_layout` 数据中
    /// 派生，无需额外的领域层结构体。
    ///
    /// 本测试验证：给定一个以单键认识为目标的示例课程，其 `target_keys`
    /// 与逐一映射得到的 `FingerZone` 可以同时获取，且覆盖该课程涉及的
    /// 每一个按键（无遗漏映射）。
    fn sample_single_key_lesson() -> Lesson {
        Lesson {
            id: LessonId("lesson-home-row-left".to_string()),
            title: "左手基准键 ASDF".to_string(),
            goal: LessonGoal::SingleKey,
            target_keys: vec![KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F],
            requires_shift: false,
            unlock_criteria: UnlockCriteria {
                min_accuracy: Some(90.0),
                max_duration_secs: None,
                min_attempts: None,
            },
        }
    }

    #[test]
    fn lesson_target_keys_and_finger_zones_are_simultaneously_derivable() {
        let lesson = sample_single_key_lesson();

        // 键位高亮所需的数据：课程涉及的按键集合，直接来自 target_keys。
        let highlighted_keys: Vec<KeyCode> = lesson.target_keys.clone();
        assert_eq!(
            highlighted_keys,
            vec![KeyCode::A, KeyCode::S, KeyCode::D, KeyCode::F]
        );

        // 分区标识所需的数据：对每个高亮键位求其手指分区，与 target_keys
        // 一一对应，构成 (KeyCode, FingerZone) 对，可同时供虚拟键盘渲染
        // 高亮与分区着色。
        let key_zone_pairs: Vec<(KeyCode, FingerZone)> = lesson
            .target_keys
            .iter()
            .map(|&key| (key, finger_zone_of(key)))
            .collect();

        assert_eq!(
            key_zone_pairs,
            vec![
                (KeyCode::A, FingerZone::LeftPinky),
                (KeyCode::S, FingerZone::LeftRing),
                (KeyCode::D, FingerZone::LeftMiddle),
                (KeyCode::F, FingerZone::LeftIndex),
            ]
        );

        // 覆盖性断言：课程涉及的每一个按键都能解析出唯一的手指分区，
        // 不存在遗漏映射（键位集合与分区集合数量一致）。
        assert_eq!(key_zone_pairs.len(), lesson.target_keys.len());
        for (key, _zone) in &key_zone_pairs {
            assert!(lesson.target_keys.contains(key));
        }
    }

    #[test]
    fn lesson_with_single_target_key_still_yields_key_and_zone() {
        let lesson = Lesson {
            id: LessonId("lesson-single-key-j".to_string()),
            title: "认识 J 键".to_string(),
            goal: LessonGoal::SingleKey,
            target_keys: vec![KeyCode::J],
            requires_shift: false,
            unlock_criteria: UnlockCriteria {
                min_accuracy: None,
                max_duration_secs: None,
                min_attempts: Some(1),
            },
        };

        let key_zone_pairs: Vec<(KeyCode, FingerZone)> = lesson
            .target_keys
            .iter()
            .map(|&key| (key, finger_zone_of(key)))
            .collect();

        assert_eq!(key_zone_pairs, vec![(KeyCode::J, FingerZone::RightIndex)]);
    }
}


