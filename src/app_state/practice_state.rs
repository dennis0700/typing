//! PracticeEngine：练习环节运行时状态（计时、输入位置）。
//!
//! `PracticeState` 持有当前练习环节的 `TypingMatchState`（字符匹配状态机）
//! 与练习开始时刻 `started_at`，并记录本次练习针对的课程 `lesson_id`。
//!
//! - Req 3.1：练习环节开始时展示练习文本（即 `typing_state.text`）并将第一个
//!   字符标记为下一个应输入字符（即 `typing_state.cursor == 0`）——由
//!   `PracticeState::new` 通过 `TypingMatchState::new` 保证。
//! - Req 3.7：练习环节进行中持续显示已用时长——由 `elapsed` 提供，供 UI 层
//!   `Timer` 周期性读取刷新显示，正式计时结束点为最后字符正确输入的时刻
//!   （即 `typing_state.is_complete()` 变为 `true` 时，调用方应停止继续读取
//!   `elapsed` 用于展示，而不是本结构体自动停止计时）。
//! - Req 7.5：学员主动退出练习环节——由 [`PracticeState::exit`] 处理，
//!   将会话级进度保存为 [`SuspendedPracticeState`]，不生成正式
//!   `storage::PracticeResult`，并返回导航到课程序列界面的命令意图。

use std::time::{Duration, Instant};

use crate::domain::{LessonId, TypingMatchState, calc_accuracy, calc_wpm};
use crate::storage;

/// 练习环节运行时状态：当前练习目标（课程）、练习文本对应的字符匹配状态机，
/// 以及练习开始时刻。
#[derive(Debug, Clone)]
pub struct PracticeState {
    /// 当前练习环节针对的课程 id（练习目标）。
    pub lesson_id: LessonId,
    /// 字符匹配状态机：持有练习文本、输入位置与错误统计。
    pub typing_state: TypingMatchState,
    /// 练习环节开始的时刻，用于计算已用时长（Req 3.7）。
    pub started_at: Instant,
}

impl PracticeState {
    /// 基于课程 id 与练习文本创建初始练习状态：
    /// - `typing_state` 通过 `TypingMatchState::new(text)` 初始化，`cursor = 0`
    ///   （第一个字符即为下一个应输入字符，Req 3.1）；
    /// - `started_at` 置为创建时刻 `Instant::now()`，作为已用时长的计时起点。
    pub fn new(lesson_id: LessonId, text: Vec<char>) -> Self {
        Self {
            lesson_id,
            typing_state: TypingMatchState::new(text),
            started_at: Instant::now(),
        }
    }

    /// 已用时长：自 `started_at` 至当前调用时刻经过的时间（Req 3.7）。
    ///
    /// 供 UI 层 `Timer` 周期性调用以刷新"已用时长"显示；调用本方法不会
    /// 改变 `started_at`，多次调用返回单调不减的结果。
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// 练习文本（只读），委托给 `typing_state.text`。
    ///
    /// UI 桥接层直接读 `practice_state.typing_state`（它需要的是整个匹配状态
    /// 机来算每个字符的 `CharState`），因此没有经由这两个便捷访问器；它们是
    /// 本层公开 API 且被单测大量使用（构造输入序列时需要取练习文本/光标），
    /// 故保留。`cfg_attr` 只压制非测试构建的 dead_code 提示。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn text(&self) -> &[char] {
        &self.typing_state.text
    }

    /// 当前练习环节是否已完成（练习文本的最后一个字符已被正确输入），
    /// 委托给 `typing_state.is_complete()`。
    pub fn is_complete(&self) -> bool {
        self.typing_state.is_complete()
    }

    /// 下一个待输入字符的位置，委托给 `typing_state.cursor`。
    ///
    /// 未被 UI 直接调用的原因同 [`PracticeState::text`]。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn cursor(&self) -> usize {
        self.typing_state.cursor
    }

    /// 练习完成后生成本次成绩 `storage::PracticeResult`（Req 4.3, 5.1）。
    ///
    /// 仅当 `self.is_complete()` 为 `true`（练习文本的最后一个字符已被正确
    /// 输入，Req 3.5 的完成判定）时才产出结果；否则返回 `None`，调用方据此
    /// 判断"练习环节完成前主动退出"的情况不应生成正式成绩记录（Req 4.3）
    /// ——`ExitPractice` 处理流程（任务 14.4）应改用其他路径保存会话级进度，
    /// 不应调用本方法。
    ///
    /// 各字段计算方式：
    /// - `accuracy`：通过 `calc_accuracy(correct_inputs, total_inputs)` 计算。
    ///   `correct_inputs` 恒等于练习文本长度（完成状态下每个字符都已被正确
    ///   输入过一次，即 `cursor == text.len()`）；`total_inputs` 为总输入
    ///   次数 = 正确输入次数 + 错误输入次数（`correct_inputs + error_count`），
    ///   因为 `TypingMatchState` 中的 `error_count` 统计的正是"额外的"错误
    ///   按键次数，不会重复计入最终成功的那一次正确输入。
    /// - `wpm`：通过 `calc_wpm(total_chars, self.elapsed())` 计算，
    ///   `total_chars` 取练习文本长度（衡量"有效产出"的字符数，与
    ///   `calc_accuracy` 中 `correct_inputs` 的含义一致）。
    /// - `error_count`：直接取 `self.typing_state.error_count`。
    /// - `duration_ms`：取 `self.elapsed().as_millis()`。
    /// - `completed_at`：取调用时刻的 `chrono::Utc::now()`。
    /// - `score`：设计文档未定义具体计分公式，此处采用
    ///   `accuracy.round() as u32`（即正确率的整数近似值，范围
    ///   `[0, 100]`）作为占位实现——这是一个尚待产品/设计确认的假设，
    ///   而非设计文档规定的公式。
    /// - `lesson_id`：取 `self.lesson_id`，转换为 `storage::LessonId`。
    pub fn to_practice_result(&self) -> Option<storage::PracticeResult> {
        if !self.is_complete() {
            return None;
        }

        let correct_inputs = self.typing_state.text.len() as u32;
        let error_count = self.typing_state.error_count;
        let total_inputs = correct_inputs + error_count;

        let accuracy = calc_accuracy(correct_inputs, total_inputs);
        let wpm = calc_wpm(correct_inputs, self.elapsed());
        let duration_ms = self.elapsed().as_millis() as u64;
        // score 计分公式未在设计文档中定义，暂以正确率的整数近似值作为占位实现。
        let score = accuracy.round().clamp(0.0, 100.0) as u32;

        Some(storage::PracticeResult {
            lesson_id: storage::LessonId(self.lesson_id.0.clone()),
            accuracy,
            wpm,
            error_count,
            duration_ms,
            score,
            completed_at: chrono::Utc::now(),
        })
    }

    /// 处理学员主动退出练习环节（`AppEvent::ExitPractice`，Req 7.5）。
    ///
    /// 与 [`Self::to_practice_result`] 不同，本方法**不会**调用
    /// `to_practice_result`，也不产出 `storage::PracticeResult`——无论当前
    /// 练习是否已经完成，退出流程都不生成该次练习环节的正式成绩记录
    /// （Req 4.3：练习环节完成前主动退出不生成正式成绩记录）。调用方
    /// （未来的 `AppController::dispatch`）不应在处理 `ExitPractice` 事件时
    /// 另行调用 `to_practice_result`/`CurriculumState::record_practice_result`。
    ///
    /// 本方法消费 `self`，将当前练习环节的会话级进度（练习目标课程、字符
    /// 匹配状态机、已用时长）保存为 [`SuspendedPracticeState`]——一个可用于
    /// 恢复的快照，而非持久化的学习进度记录。是否/何时使用该快照恢复练习
    /// （例如学员再次进入同一课程时是否从此处继续）超出本任务范围，留给
    /// 后续任务（`AppController`/`CurriculumState` 的路由逻辑）决定；本方法
    /// 仅保证退出时进度不会被静默丢弃。
    ///
    /// 返回的 [`ExitPracticeOutcome`] 同时携带一个"导航到课程序列界面"的
    /// 命令意图（`navigate_to`），供调用方在返回的 `UiCommand` 列表
    /// （任务 16，当前尚不存在）中包含对应的导航命令（Req 7.5）。
    pub fn exit(self) -> ExitPracticeOutcome {
        ExitPracticeOutcome {
            suspended: SuspendedPracticeState {
                lesson_id: self.lesson_id,
                typing_state: self.typing_state,
                started_at: self.started_at,
            },
            navigate_to: NavigationTarget::CurriculumSequence,
        }
    }
}

/// 练习环节因学员主动退出而保存的会话级进度快照（Req 7.5）。
///
/// 与 `storage::PracticeResult`/`LessonRecord.history` 中的记录不同，本结构
/// 不是正式成绩记录，不会被写入学员档案的持久化学习进度中；它仅在当前
/// 应用会话内存中保留，供同一会话内"继续练习"场景使用（设计文档 Error
/// Handling 一节："仅将会话级进度……暂存于内存，供同一会话内'继续'使用
/// （不持久化为正式成绩）"）。
///
/// 三个字段目前都没有读取方：「继续练习」这一入口尚未在 UI 上实现，退出
/// 练习后 `AppController` 只用到 `ExitPracticeOutcome.navigate_to`。快照
/// 本身的正确性（退出时确实保留了课程 id/匹配状态/开始时刻，且不产生正式
/// 成绩）由单测覆盖，是 Req 7.5 的落点，故保留字段而不删除。
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct SuspendedPracticeState {
    /// 被中断的练习环节所针对的课程 id。
    pub lesson_id: LessonId,
    /// 中断时刻的字符匹配状态机（练习文本、输入位置、错误统计）。
    pub typing_state: TypingMatchState,
    /// 原练习环节的开始时刻（保留以便未来"继续练习"场景下延续已用时长）。
    pub started_at: Instant,
}

/// `PracticeState::exit` 返回的导航命令意图：当前尚无 `UiCommand`/
/// `AppController`（任务 16），此处以最小化的枚举表达"应导航到哪个界面"，
/// 供未来的 `AppController::dispatch` 在构造 `UiCommand::NavigateTo(..)`
/// 时直接映射使用，而不必在本任务中提前定义完整的 `UiCommand` 体系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationTarget {
    /// 课程序列界面（Req 1.6：选择/新建档案后进入；Req 7.5：退出练习后
    /// 返回课程序列界面）。
    CurriculumSequence,
    /// 练习环节界面（Req 1.6：选择已解锁课程后 2 秒内进入对应练习环节）。
    PracticeSession,
    /// 学员档案选择界面（Req 6.2）。用于"返回上一层"这条反向导航：从课程
    /// 序列界面退回档案选择界面以切换学员或管理档案（Req 6.8-6.14 的管理
    /// 入口就在该界面上，没有这条返回路径的话，选定档案后整个会话就再也回
    /// 不到管理界面）。
    ProfileSelect,
}

/// `PracticeState::exit` 的返回结果：会话级进度快照 + 导航命令意图。
///
/// 明确不包含任何 `storage::PracticeResult` 字段——调用方无法从本结构中
/// 获得一个可供落盘的正式成绩，这是有意为之的设计约束（Req 4.3）。
#[derive(Debug, Clone)]
pub struct ExitPracticeOutcome {
    /// 保存下来的会话级练习进度，供同一会话内可能的"继续"场景使用。
    /// 目前无读取方，原因见 [`SuspendedPracticeState`]。
    #[cfg_attr(not(test), allow(dead_code))]
    pub suspended: SuspendedPracticeState,
    /// 应导航到的界面。
    pub navigate_to: NavigationTarget,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lesson_id() -> LessonId {
        LessonId("lesson-1".to_string())
    }

    #[test]
    fn new_initializes_typing_state_with_given_text_and_zero_cursor() {
        let text: Vec<char> = "abc".chars().collect();
        let state = PracticeState::new(sample_lesson_id(), text.clone());

        assert_eq!(state.lesson_id, sample_lesson_id());
        assert_eq!(state.typing_state.text, text);
        assert_eq!(state.cursor(), 0);
        assert_eq!(state.typing_state.error_count, 0);
        assert!(!state.typing_state.current_char_has_error);
        assert!(!state.is_complete());
        assert_eq!(state.text(), text.as_slice());
    }

    #[test]
    fn elapsed_is_non_negative_immediately_after_construction() {
        let state = PracticeState::new(sample_lesson_id(), "a".chars().collect());
        // Duration 类型本身恒非负；此处验证调用不会 panic 且可正常读取。
        let elapsed = state.elapsed();
        assert!(elapsed >= Duration::ZERO);
    }

    #[test]
    fn elapsed_is_monotonically_non_decreasing_over_time() {
        let state = PracticeState::new(sample_lesson_id(), "a".chars().collect());
        let first = state.elapsed();
        std::thread::sleep(Duration::from_millis(5));
        let second = state.elapsed();

        assert!(second >= first);
    }

    /// 集成测试（Req 3.7）："已用时长"属性随真实时间推移持续刷新。
    ///
    /// 模拟 UI 层 `Timer` 周期性调用 `elapsed()` 刷新显示的场景：在真实
    /// `std::thread::sleep` 间隔之后多次采样 `elapsed()`，验证：
    /// 1) 每次采样结果相较上一次单调不减（不会出现时间倒退）；
    /// 2) 采样值与真实经过的墙钟时间保持在合理误差范围内（既不会明显滞后
    ///    也不会明显超前），确认 `elapsed()` 确实反映了真实经过的时长，而
    ///    不是一个恒定值/未被驱动更新的值。
    #[test]
    fn timer_driven_polling_refreshes_elapsed_duration_over_real_time() {
        let state = PracticeState::new(sample_lesson_id(), "abc".chars().collect());

        let sleep_interval = Duration::from_millis(50);
        let sample_count = 4;
        let mut samples = Vec::with_capacity(sample_count);

        for _ in 0..sample_count {
            std::thread::sleep(sleep_interval);
            samples.push(state.elapsed());
        }

        // 1) 属性刷新：每次“Timer 触发”（此处以真实 sleep 模拟）后采样值
        //    单调不减，反映已用时长在持续推进。
        for i in 1..samples.len() {
            assert!(
                samples[i] >= samples[i - 1],
                "elapsed() should not decrease across successive timer ticks: {:?} then {:?}",
                samples[i - 1],
                samples[i]
            );
        }

        // 2) 采样值与真实经过时间大致吻合：给予充裕误差余量（500ms）以避免
        //    因测试执行环境调度抖动导致假失败，同时仍能验证 elapsed() 不是
        //    一个未被驱动的固定值。
        let expected_min = sleep_interval * (sample_count as u32);
        let last = *samples.last().expect("at least one sample recorded");
        assert!(
            last >= expected_min,
            "expected elapsed() to reflect at least {:?} of real sleep time, got {:?}",
            expected_min,
            last
        );
        assert!(
            last < expected_min + Duration::from_millis(500),
            "expected elapsed() to stay within a generous tolerance of real sleep time, got {:?} (expected around {:?})",
            last,
            expected_min
        );
    }

    #[test]
    fn is_complete_reflects_underlying_typing_state() {
        use crate::domain::apply_input;

        let text: Vec<char> = "a".chars().collect();
        let mut state = PracticeState::new(sample_lesson_id(), text);
        assert!(!state.is_complete());

        let (new_typing_state, _) = apply_input(&state.typing_state, 'a');
        state.typing_state = new_typing_state;

        assert!(state.is_complete());
    }

    #[test]
    fn to_practice_result_returns_none_when_not_complete() {
        let state = PracticeState::new(sample_lesson_id(), "abc".chars().collect());
        assert!(!state.is_complete());
        assert_eq!(state.to_practice_result(), None);
    }

    #[test]
    fn to_practice_result_all_correct_input_yields_full_accuracy() {
        use crate::domain::apply_input;

        let text: Vec<char> = "abc".chars().collect();
        let mut state = PracticeState::new(sample_lesson_id(), text.clone());

        for &c in &text {
            let (new_typing_state, _) = apply_input(&state.typing_state, c);
            state.typing_state = new_typing_state;
        }
        assert!(state.is_complete());

        let result = state
            .to_practice_result()
            .expect("completed practice session should produce a PracticeResult");

        assert_eq!(result.lesson_id, storage::LessonId("lesson-1".to_string()));
        assert_eq!(result.accuracy, 100.0);
        assert_eq!(result.error_count, 0);
        assert_eq!(result.score, 100);
    }

    #[test]
    fn to_practice_result_with_errors_yields_reduced_accuracy() {
        use crate::domain::apply_input;

        // 文本 "ab"：先输入两次错误字符，再正确完成整段文本。
        // correct_inputs = 2（文本长度），error_count = 2，
        // total_inputs = 4，accuracy = 2/4*100 = 50.0。
        let text: Vec<char> = "ab".chars().collect();
        let mut state = PracticeState::new(sample_lesson_id(), text.clone());

        let (s, _) = apply_input(&state.typing_state, 'x'); // 错误输入
        state.typing_state = s;
        let (s, _) = apply_input(&state.typing_state, 'y'); // 错误输入
        state.typing_state = s;
        let (s, _) = apply_input(&state.typing_state, 'a'); // 正确
        state.typing_state = s;
        let (s, _) = apply_input(&state.typing_state, 'b'); // 正确，完成
        state.typing_state = s;
        assert!(state.is_complete());

        let result = state
            .to_practice_result()
            .expect("completed practice session should produce a PracticeResult");

        assert_eq!(result.error_count, 2);
        assert_eq!(result.accuracy, 50.0);
        assert_eq!(result.score, 50);
    }

    #[test]
    fn to_practice_result_wpm_reflects_elapsed_duration() {
        use crate::domain::apply_input;

        // 构造一个已完成的状态，但手动设置 started_at 使得 elapsed() 是
        // 一个确定的、可用于验证公式的时长，避免依赖真实 Instant 的抖动。
        let text: Vec<char> = "abcde".chars().collect(); // 5 个字符
        let mut state = PracticeState::new(sample_lesson_id(), text.clone());
        for &c in &text {
            let (new_typing_state, _) = apply_input(&state.typing_state, c);
            state.typing_state = new_typing_state;
        }
        assert!(state.is_complete());

        // 将开始时刻回拨 60 秒，使 elapsed() ≈ 60s。
        state.started_at = Instant::now() - Duration::from_secs(60);

        let result = state
            .to_practice_result()
            .expect("completed practice session should produce a PracticeResult");

        // wpm = (correct_inputs / 5) / duration_minutes = (5/5) / 1 = 1.0
        // 允许因测试执行耗时带来的微小误差。
        assert!(
            (result.wpm - 1.0).abs() < 0.1,
            "expected wpm close to 1.0, got {}",
            result.wpm
        );
        assert!(result.duration_ms >= 60_000);
    }

    #[test]
    fn to_practice_result_uses_correct_lesson_id() {
        use crate::domain::apply_input;

        let lesson_id = LessonId("custom-lesson".to_string());
        let text: Vec<char> = "a".chars().collect();
        let mut state = PracticeState::new(lesson_id, text);
        let (new_typing_state, _) = apply_input(&state.typing_state, 'a');
        state.typing_state = new_typing_state;

        let result = state.to_practice_result().expect("should be complete");
        assert_eq!(result.lesson_id, storage::LessonId("custom-lesson".to_string()));
    }

    // --- Req 7.5：退出练习环节（ExitPractice）相关测试 ---

    #[test]
    fn exit_mid_practice_does_not_produce_a_practice_result() {
        use crate::domain::apply_input;

        // 会话中已应用了部分正确/错误输入，但尚未完成整段练习文本。
        let text: Vec<char> = "abcde".chars().collect();
        let mut state = PracticeState::new(sample_lesson_id(), text);

        let (s, _) = apply_input(&state.typing_state, 'x'); // 错误输入
        state.typing_state = s;
        let (s, _) = apply_input(&state.typing_state, 'a'); // 正确输入
        state.typing_state = s;
        assert!(!state.is_complete());

        // 退出前：确认在未完成状态下 to_practice_result 本身已经返回 None
        // （14.2 已保证的行为，此处围绕"退出中途练习"场景重新断言，用于
        // 回归测试/文档目的）。
        assert_eq!(state.to_practice_result(), None);

        let outcome = state.exit();

        // exit() 不产出任何 storage::PracticeResult ——ExitPracticeOutcome
        // 结构本身没有该字段，因此这一约束在类型层面即成立；这里进一步
        // 验证会话进度被完整保留在 suspended 快照中。
        assert_eq!(outcome.suspended.lesson_id, sample_lesson_id());
        assert_eq!(outcome.suspended.typing_state.cursor, 1);
        assert_eq!(outcome.suspended.typing_state.error_count, 1);
        assert_eq!(outcome.navigate_to, NavigationTarget::CurriculumSequence);
    }

    #[test]
    fn exit_returns_navigation_command_to_curriculum_sequence() {
        let state = PracticeState::new(sample_lesson_id(), "abc".chars().collect());
        let outcome = state.exit();

        assert_eq!(outcome.navigate_to, NavigationTarget::CurriculumSequence);
    }

    #[test]
    fn exit_preserves_session_progress_for_resumption() {
        use crate::domain::apply_input;

        let text: Vec<char> = "hello".chars().collect();
        let mut state = PracticeState::new(sample_lesson_id(), text.clone());

        let (s, _) = apply_input(&state.typing_state, 'h');
        state.typing_state = s;
        let (s, _) = apply_input(&state.typing_state, 'e');
        state.typing_state = s;

        let started_at = state.started_at;
        let outcome = state.exit();

        // 会话级进度（练习文本、光标位置、开始时刻）被原样保存，供未来
        // "继续练习"场景使用，而不是被丢弃。
        assert_eq!(outcome.suspended.typing_state.text, text);
        assert_eq!(outcome.suspended.typing_state.cursor, 2);
        assert_eq!(outcome.suspended.started_at, started_at);
    }

    #[test]
    fn exit_even_on_completed_session_does_not_yield_a_practice_result() {
        use crate::domain::apply_input;

        // 边界场景：即便练习已经完成才触发退出（例如 UI 层竞态），exit()
        // 的返回类型上也不包含 PracticeResult——退出路径与"完成后生成
        // 成绩"路径（to_practice_result）是两条独立、互不隐式调用的路径。
        let text: Vec<char> = "a".chars().collect();
        let mut state = PracticeState::new(sample_lesson_id(), text);
        let (s, _) = apply_input(&state.typing_state, 'a');
        state.typing_state = s;
        assert!(state.is_complete());

        let outcome = state.exit();
        assert!(outcome.suspended.typing_state.is_complete());
        assert_eq!(outcome.navigate_to, NavigationTarget::CurriculumSequence);
    }

    // Feature: typing-desktop-app, Property 19: 退出练习的状态转移
    //
    // 对于任意进行中的练习状态，触发 ExitPractice 事件后：返回的命令集合
    // 包含导航到课程序列界面的命令；当前练习环节的进度数据被保存到可恢复
    // 的会话状态中（lesson_id、typing_state、started_at 均被原样保留，
    // 不发生数据丢失或篡改）；且不生成正式的 PracticeResult 历史记录。
    // 该性质无论会话在退出时是否已完成均成立。
    //
    // Validates: Requirements 7.5
    mod exit_state_transition_property {
        use super::*;
        use crate::domain::apply_input;
        use proptest::prelude::*;

        /// 单次输入动作：正确输入（推进 cursor）或错误输入（保持 cursor，
        /// 递增 error_count）。用于构造"任意序列的正确/错误输入"场景，
        /// 覆盖退出前的任意会话历史。
        #[derive(Debug, Clone, Copy)]
        enum InputAction {
            Correct,
            Incorrect,
        }

        fn arb_input_action() -> impl Strategy<Value = InputAction> {
            prop_oneof![Just(InputAction::Correct), Just(InputAction::Incorrect)]
        }

        proptest! {
            #[test]
            fn prop_exit_state_transition(
                text in proptest::collection::vec(proptest::char::range('a', 'z'), 1..30),
                actions in proptest::collection::vec(arb_input_action(), 0..40),
                started_at_offset_secs in 0u64..1000,
            ) {
                let lesson_id = LessonId("prop-exit-lesson".to_string());
                let mut state = PracticeState::new(lesson_id.clone(), text.clone());
                // 使 started_at 取任意确定的历史时刻，验证其在退出后被原样保留。
                state.started_at = Instant::now() - Duration::from_secs(started_at_offset_secs);
                let started_at_before_exit = state.started_at;

                // 应用任意序列的正确/错误输入（一旦已完成，"正确输入"动作会
                // 变为 AlreadyComplete 且不改变状态，"错误输入"同理不产生
                // 实际效果——apply_input 本身保证了这一点）。
                for action in &actions {
                    let input = match action {
                        InputAction::Correct => {
                            if state.typing_state.is_complete() {
                                'z' // 任意字符，已完成状态下不会产生效果
                            } else {
                                state.typing_state.text[state.typing_state.cursor]
                            }
                        }
                        InputAction::Incorrect => {
                            if state.typing_state.is_complete() {
                                'z'
                            } else {
                                let expected = state.typing_state.text[state.typing_state.cursor];
                                // 选取一个必然不等于 expected 的字符：'a'..'z' 与
                                // 用于错误输入的非目标字符集不相交时退化为同一字符，
                                // 因此在相等时改用一个明确越界于生成器字符集的字符。
                                if expected == 'Z' { 'Y' } else { 'Z' }
                            }
                        }
                    };
                    let (new_state, _outcome) = apply_input(&state.typing_state, input);
                    state.typing_state = new_state;
                }

                // 记录退出前的完整快照，用于逐字段比对。
                let lesson_id_before_exit = state.lesson_id.clone();
                let typing_state_before_exit = state.typing_state.clone();
                let was_complete_before_exit = state.typing_state.is_complete();

                let outcome = state.exit();

                // 1) 恒导航到课程序列界面。
                prop_assert_eq!(outcome.navigate_to, NavigationTarget::CurriculumSequence);

                // 2) 不产生任何 storage::PracticeResult：ExitPracticeOutcome
                //    结构本身没有该字段（类型层面即成立），进一步验证
                //    suspended 快照中也没有隐藏地携带一份成绩记录——
                //    suspended 的字段集合恰好是 lesson_id/typing_state/
                //    started_at 三者，不包含 accuracy/wpm/score 等成绩字段。
                let SuspendedPracticeState {
                    lesson_id: suspended_lesson_id,
                    typing_state: suspended_typing_state,
                    started_at: suspended_started_at,
                } = outcome.suspended;

                // 3) suspended 快照精确保留退出前的 lesson_id、typing_state
                //    （text、cursor、error_count、current_char_has_error）
                //    与 started_at，不发生数据丢失或篡改。
                prop_assert_eq!(suspended_lesson_id, lesson_id_before_exit);
                prop_assert_eq!(
                    suspended_typing_state.cursor,
                    typing_state_before_exit.cursor
                );
                prop_assert_eq!(
                    suspended_typing_state.error_count,
                    typing_state_before_exit.error_count
                );
                prop_assert_eq!(
                    suspended_typing_state.current_char_has_error,
                    typing_state_before_exit.current_char_has_error
                );
                prop_assert_eq!(suspended_started_at, started_at_before_exit);

                // 4) 该性质无论会话在退出时是否已完成均成立——此处针对两种
                //    情形都已被前面的断言覆盖，此处仅确认完成状态本身也被
                //    如实保留（既不会把未完成的会话标记为完成，也不会把
                //    已完成的会话重置为未完成）。
                prop_assert_eq!(suspended_typing_state.is_complete(), was_complete_before_exit);

                // text 字段的比对放在最后（消费 suspended_typing_state），
                // 避免与上面按值使用的字段产生借用冲突。
                prop_assert_eq!(suspended_typing_state.text, typing_state_before_exit.text);
            }
        }
    }
}
