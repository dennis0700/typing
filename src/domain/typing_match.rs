//! 字符匹配校验状态机。

use crate::domain::keyboard_layout::{KeyCode, char_to_key};

/// 字符匹配状态机的状态：记录练习文本、当前输入位置与错误统计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingMatchState {
    /// 练习文本，按字符拆分。
    pub text: Vec<char>,
    /// 下一个待输入字符的位置。
    pub cursor: usize,
    /// 本次练习环节累计的错误输入次数。
    pub error_count: u32,
    /// 当前 `cursor` 指向的字符是否处于错误标记状态。
    pub current_char_has_error: bool,
}

impl TypingMatchState {
    /// 基于练习文本创建初始状态：`cursor = 0`，无错误标记，`error_count = 0`。
    pub fn new(text: Vec<char>) -> Self {
        Self {
            text,
            cursor: 0,
            error_count: 0,
            current_char_has_error: false,
        }
    }

    /// 判定当前状态是否已完成：`cursor` 达到文本长度，即所有字符均已正确输入。
    ///
    /// **Validates: Requirements 2.6, 3.5**
    pub fn is_complete(&self) -> bool {
        self.cursor >= self.text.len()
    }
}

/// 一次 `apply_input` 调用的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOutcome {
    /// 输入字符与待输入字符一致，`cursor` 前进到 `advanced_to`。
    Correct { advanced_to: usize },
    /// 输入字符与待输入字符不一致。
    Incorrect,
    /// 状态已处于完成状态，本次输入不产生任何效果。
    AlreadyComplete,
}

/// 纯函数：输入一个字符，返回新状态与本次输入结果。
///
/// - 若状态已完成（`cursor` 达到文本长度），返回原状态的拷贝与
///   `MatchOutcome::AlreadyComplete`，不改变任何字段。
/// - 若输入字符与 `cursor` 指向的待输入字符一致，返回 `cursor + 1`、
///   `current_char_has_error` 被清除的新状态，与
///   `MatchOutcome::Correct { advanced_to: cursor + 1 }`。
/// - 若输入字符与待输入字符不一致，返回 `cursor` 不变、`error_count + 1`、
///   `current_char_has_error = true` 的新状态，与 `MatchOutcome::Incorrect`。
///   该分支覆盖"已处于错误标记状态下再次输入错误"的情况：每次错误都会使
///   `error_count` 递增且 `cursor` 保持不变。
///
/// **Validates: Requirements 2.4, 2.5, 3.2, 3.3, 3.6**
pub fn apply_input(state: &TypingMatchState, input: char) -> (TypingMatchState, MatchOutcome) {
    if state.is_complete() {
        return (state.clone(), MatchOutcome::AlreadyComplete);
    }

    // `is_complete` 为 false 时 `cursor < text.len()`，可安全索引。
    let expected = state.text[state.cursor];

    if input == expected {
        let advanced_to = state.cursor + 1;
        let new_state = TypingMatchState {
            text: state.text.clone(),
            cursor: advanced_to,
            error_count: state.error_count,
            current_char_has_error: false,
        };
        (new_state, MatchOutcome::Correct { advanced_to })
    } else {
        let new_state = TypingMatchState {
            text: state.text.clone(),
            cursor: state.cursor,
            error_count: state.error_count + 1,
            current_char_has_error: true,
        };
        (new_state, MatchOutcome::Incorrect)
    }
}

/// 单个字符位置的显示状态：待输入 / 已正确输入 / 当前标记为错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharState {
    /// 尚未输入（位于 `cursor` 之后，或位于 `cursor` 且当前未处于错误标记状态）。
    Pending,
    /// 已正确输入（位于 `cursor` 之前）。
    Correct,
    /// 位于 `cursor` 且当前处于错误标记状态。
    Error,
}

/// 纯函数：给定 `TypingMatchState` 与字符位置 `index`，返回该位置的显示状态。
///
/// - `index < cursor`：字符已通过正确输入，状态为 `Correct`。
/// - `index == cursor`：字符是当前待输入字符；若 `current_char_has_error` 为
///   `true`，状态为 `Error`，否则为 `Pending`。
/// - `index > cursor`：字符尚未到达，状态为 `Pending`。
///
/// **Validates: Requirements 3.4**
pub fn char_state_at(state: &TypingMatchState, index: usize) -> CharState {
    if index < state.cursor {
        CharState::Correct
    } else if index == state.cursor && state.current_char_has_error {
        CharState::Error
    } else {
        CharState::Pending
    }
}

/// 纯函数：将 `CharState` 映射为与 slintcn 主题色变量对应的视觉样式 token。
///
/// 全量覆盖：3 个状态对应 3 个互不相同的 token 字符串常量，保证 `Correct`
/// 与 `Error` 的视觉样式恒不相同（Req 3.4）。
///
/// **Validates: Requirements 3.4**
///
/// 渲染路径上没有调用方：字符格的三态样式直接由 `practice_view.slint` 的
/// `PracticeCharCell` 按 `PracticeCharState` 分支定义（与虚拟键盘配色同样的
/// 取舍——`.slint` 无法调用 Rust 函数解析 token）。本函数保留的价值在于它的
/// 单测：以"3 个 token 两两互不相同"的形式把 Req 3.4 固定在可自动验证的位置。
#[cfg_attr(not(test), allow(dead_code))]
pub const fn char_state_style_token(state: CharState) -> &'static str {
    match state {
        CharState::Pending => "char-state-pending",
        CharState::Correct => "char-state-correct",
        CharState::Error => "char-state-error",
    }
}

/// 纯函数：给定当前 `TypingMatchState`，返回学员下一步应按下的按键提示。
///
/// - 若状态已完成（`is_complete() == true`），不存在下一个待按键位，返回 `None`。
/// - 否则，取 `cursor` 指向的字符并通过 `char_to_key` 反向映射为 `KeyCode`：
///   - 若该字符存在对应的物理 `KeyCode`（小写字母、数字、基础符号，以及
///     大写字母、Shift 组合符号——它们与对应的基础字符共享同一物理键位），
///     返回 `Some(key_code)`。该 `KeyCode` 只表示"按哪个键"，不含是否需要
///     按住 Shift 的信息；调用方需要该信息时应额外调用
///     [`crate::domain::keyboard_layout::needs_shift`]。
///   - 若该字符没有对应的 `KeyCode`（如未映射的中文标点等 `char_to_key`
///     返回 `None` 的情况），本函数同样返回 `None`——即"当前字符无可提示的
///     键位"，调用方应据此不在虚拟键盘上展示任何高亮提示，而不是 panic。
///
/// 同一时刻至多返回一个 `KeyCode`，不存在返回多个候选键位的情况
/// （Property 5：键位提示的唯一性）。
///
/// **Validates: Requirements 2.3**
pub fn current_key_hint(state: &TypingMatchState) -> Option<KeyCode> {
    if state.is_complete() {
        return None;
    }

    // is_complete() 为 false 时 cursor < text.len()，可安全索引。
    let expected = state.text[state.cursor];
    char_to_key(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_of(text: &str) -> TypingMatchState {
        TypingMatchState::new(text.chars().collect())
    }

    #[test]
    fn initial_state_has_zero_cursor_and_no_error() {
        let state = state_of("abc");
        assert_eq!(state.cursor, 0);
        assert_eq!(state.error_count, 0);
        assert!(!state.current_char_has_error);
        assert!(!state.is_complete());
    }

    #[test]
    fn initial_state_for_single_character_text_has_zero_cursor_and_no_error() {
        // 边界文本：长度为 1（Req 3.1 规定的最小长度）。
        let state = state_of("a");
        assert_eq!(state.text.len(), 1);
        assert_eq!(state.cursor, 0);
        assert_eq!(state.error_count, 0);
        assert!(!state.current_char_has_error);
        assert!(!state.is_complete());

        // 单字符文本在一次正确输入后应立即转为已完成状态。
        let (state, outcome) = apply_input(&state, 'a');
        assert_eq!(outcome, MatchOutcome::Correct { advanced_to: 1 });
        assert_eq!(state.cursor, 1);
        assert_eq!(state.error_count, 0);
        assert!(!state.current_char_has_error);
        assert!(state.is_complete());
    }

    #[test]
    fn initial_state_for_max_length_text_has_zero_cursor_and_no_error() {
        // 边界文本：长度为 500（Req 3.1 规定的最大长度）。
        let text: String = "a".repeat(500);
        let state = state_of(&text);

        assert_eq!(state.text.len(), 500);
        assert_eq!(state.cursor, 0);
        assert_eq!(state.error_count, 0);
        assert!(!state.current_char_has_error);
        assert!(!state.is_complete());
    }

    #[test]
    fn correct_input_advances_cursor_and_clears_error_flag() {
        let mut state = state_of("ab");
        state.current_char_has_error = true; // 模拟先前一次错误标记
        let (new_state, outcome) = apply_input(&state, 'a');

        assert_eq!(outcome, MatchOutcome::Correct { advanced_to: 1 });
        assert_eq!(new_state.cursor, 1);
        assert!(!new_state.current_char_has_error);
        assert_eq!(new_state.error_count, state.error_count);
    }

    #[test]
    fn incorrect_input_keeps_cursor_and_increments_error_count() {
        let state = state_of("ab");
        let (new_state, outcome) = apply_input(&state, 'x');

        assert_eq!(outcome, MatchOutcome::Incorrect);
        assert_eq!(new_state.cursor, 0);
        assert_eq!(new_state.error_count, 1);
        assert!(new_state.current_char_has_error);
    }

    #[test]
    fn repeated_incorrect_input_keeps_incrementing_without_advancing() {
        let state = state_of("ab");
        let (state, outcome1) = apply_input(&state, 'x');
        assert_eq!(outcome1, MatchOutcome::Incorrect);
        let (state, outcome2) = apply_input(&state, 'y');
        assert_eq!(outcome2, MatchOutcome::Incorrect);

        assert_eq!(state.cursor, 0);
        assert_eq!(state.error_count, 2);
        assert!(state.current_char_has_error);
    }

    #[test]
    fn is_complete_is_false_at_last_char_boundary() {
        // cursor 指向最后一个字符（尚未输入），此时应仍判定为未完成。
        let state = state_of("abc");
        let state = TypingMatchState {
            cursor: state.text.len() - 1,
            ..state
        };
        assert!(!state.is_complete());
        assert!(state.cursor < state.text.len());
    }

    #[test]
    fn is_complete_is_true_once_last_char_is_correctly_input() {
        let state = state_of("abc");
        let (state, _) = apply_input(&state, 'a');
        assert!(!state.is_complete());
        let (state, _) = apply_input(&state, 'b');
        assert!(!state.is_complete());
        let (state, outcome) = apply_input(&state, 'c');
        assert_eq!(outcome, MatchOutcome::Correct { advanced_to: 3 });
        assert!(state.is_complete());
        assert_eq!(state.cursor, state.text.len());
    }

    #[test]
    fn input_after_completion_returns_already_complete_and_does_not_mutate() {
        let state = state_of("a");
        let (state, outcome) = apply_input(&state, 'a');
        assert_eq!(outcome, MatchOutcome::Correct { advanced_to: 1 });
        assert!(state.is_complete());

        let (state2, outcome2) = apply_input(&state, 'a');
        assert_eq!(outcome2, MatchOutcome::AlreadyComplete);
        assert_eq!(state2, state);
    }

    #[test]
    fn current_key_hint_is_none_when_state_is_complete() {
        let state = state_of("a");
        let (state, _) = apply_input(&state, 'a');
        assert!(state.is_complete());
        assert_eq!(current_key_hint(&state), None);
    }

    #[test]
    fn current_key_hint_returns_expected_key_code_for_known_character() {
        let state = state_of("ab");
        assert_eq!(current_key_hint(&state), Some(KeyCode::A));

        let (state, _) = apply_input(&state, 'a');
        assert_eq!(current_key_hint(&state), Some(KeyCode::B));
    }

    #[test]
    fn current_key_hint_is_none_for_unmappable_character_without_panicking() {
        // 中文字符等不在键盘物理映射范围内的字符，char_to_key 返回 None，
        // current_key_hint 应同样返回 None 而不是 panic。
        let state = state_of("中");
        assert!(!state.is_complete());
        assert_eq!(current_key_hint(&state), None);
    }

    #[test]
    fn current_key_hint_returns_physical_key_for_uppercase_and_shift_symbols() {
        // 大写字母/Shift 组合符号是已有物理键位的 Shift 取值，current_key_hint
        // 应返回该物理键位（提示"按哪个键"，不含是否需要 Shift 的信息——
        // 是否需要 Shift 由调用方通过 needs_shift 单独查询）。
        let state = state_of("A:");
        assert_eq!(current_key_hint(&state), Some(KeyCode::A));

        let (state, _) = apply_input(&state, 'A');
        assert_eq!(current_key_hint(&state), Some(KeyCode::Semicolon));
    }

    #[test]
    fn char_state_at_is_correct_before_cursor() {
        let state = state_of("abc");
        let (state, _) = apply_input(&state, 'a');
        assert_eq!(state.cursor, 1);
        assert_eq!(char_state_at(&state, 0), CharState::Correct);
    }

    #[test]
    fn char_state_at_is_error_when_cursor_position_has_error_flag() {
        let mut state = state_of("abc");
        state.current_char_has_error = true;
        assert_eq!(char_state_at(&state, state.cursor), CharState::Error);
    }

    #[test]
    fn char_state_at_is_pending_when_cursor_position_has_no_error_flag() {
        let state = state_of("abc");
        assert!(!state.current_char_has_error);
        assert_eq!(char_state_at(&state, state.cursor), CharState::Pending);
    }

    #[test]
    fn char_state_at_is_pending_after_cursor() {
        let state = state_of("abc");
        assert_eq!(char_state_at(&state, state.cursor + 1), CharState::Pending);
        assert_eq!(char_state_at(&state, state.cursor + 2), CharState::Pending);
    }

    #[test]
    fn char_state_style_token_produces_three_distinct_tokens() {
        let tokens = [
            char_state_style_token(CharState::Pending),
            char_state_style_token(CharState::Correct),
            char_state_style_token(CharState::Error),
        ];
        let unique: std::collections::HashSet<&&str> = tokens.iter().collect();
        assert_eq!(
            unique.len(),
            3,
            "Pending/Correct/Error 三种状态必须映射到 3 个互不相同的视觉样式 token"
        );
    }

    #[test]
    fn char_state_style_token_correct_and_error_are_distinct() {
        // 直接对应 Req 3.4："该视觉样式与表示正确输入的视觉样式不同"。
        assert_ne!(
            char_state_style_token(CharState::Correct),
            char_state_style_token(CharState::Error)
        );
    }

    /// 全部 `CharState` 变体的枚举值列表，供属性测试中生成任意 `CharState` 使用。
    const ALL_CHAR_STATES: &[CharState] = &[CharState::Pending, CharState::Correct, CharState::Error];

    // Feature: typing-desktop-app, Property 7: 字符状态到视觉样式映射的可区分性
    //
    // 对于 CharState 的任意两个取值：它们映射到相同的视觉样式 token，
    // 当且仅当它们是同一个变体；不同变体恒映射到不同的 token
    // （即 char_state_style_token 是单射）。
    //
    // Validates: Requirements 3.4
    mod char_state_style_token_property {
        use super::*;
        use proptest::prelude::*;

        /// 生成任意 `CharState` 的策略（`CharState` 未派生 `Arbitrary`，
        /// 通过枚举全量变体实现，与 `keyboard_layout.rs` 中 `arb_key_code` 同构）。
        fn arb_char_state() -> impl Strategy<Value = CharState> {
            prop::sample::select(ALL_CHAR_STATES)
        }

        proptest! {
            #[test]
            fn prop_char_state_style_token_is_injective(
                s1 in arb_char_state(),
                s2 in arb_char_state(),
            ) {
                if s1 == s2 {
                    prop_assert_eq!(char_state_style_token(s1), char_state_style_token(s2));
                } else {
                    prop_assert_ne!(char_state_style_token(s1), char_state_style_token(s2));
                }
            }
        }
    }

    // Feature: typing-desktop-app, Property 3: 字符匹配状态机的输入一致性
    //
    // 对于任意 TypingMatchState 与任意输入字符 c：若 c 等于 cursor 指向的待输入
    // 字符，则新状态的 cursor 前进一位且 current_char_has_error 被清除；若 c
    // 不等于该字符，则新状态的 cursor 保持不变、error_count 增加 1、
    // current_char_has_error 被置位。该性质对连续多次错误输入同样成立。
    //
    // Validates: Requirements 2.4, 2.5, 3.2, 3.3, 3.6
    #[cfg(test)]
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn prop_apply_input_consistency(
                text in proptest::collection::vec(proptest::char::range('a', 'z'), 1..30),
                cursor_ratio in 0.0f64..1.0,
                input in proptest::char::range('a', 'z'),
                error_count in 0u32..1000,
            ) {
                let cursor = ((text.len() as f64) * cursor_ratio) as usize;
                let cursor = cursor.min(text.len() - 1); // 保证未完成状态

                let state = TypingMatchState {
                    text: text.clone(),
                    cursor,
                    error_count,
                    current_char_has_error: false,
                };

                let expected_char = text[cursor];
                let (new_state, outcome) = apply_input(&state, input);

                if input == expected_char {
                    prop_assert_eq!(outcome, MatchOutcome::Correct { advanced_to: cursor + 1 });
                    prop_assert_eq!(new_state.cursor, cursor + 1);
                    prop_assert!(!new_state.current_char_has_error);
                    prop_assert_eq!(new_state.error_count, error_count);
                } else {
                    prop_assert_eq!(outcome, MatchOutcome::Incorrect);
                    prop_assert_eq!(new_state.cursor, cursor);
                    prop_assert_eq!(new_state.error_count, error_count + 1);
                    prop_assert!(new_state.current_char_has_error);
                }
            }

            #[test]
            fn prop_repeated_incorrect_inputs_keep_incrementing(
                text in proptest::collection::vec(proptest::char::range('a', 'z'), 1..30),
                wrong_inputs in proptest::collection::vec(proptest::char::range('A', 'Z'), 1..10),
            ) {
                // 'A'..'Z' 与目标文本字符集 'a'..'z' 不相交，保证每次输入均为错误输入。
                let mut state = TypingMatchState::new(text);
                let initial_cursor = state.cursor;

                for (i, &c) in wrong_inputs.iter().enumerate() {
                    let (new_state, outcome) = apply_input(&state, c);
                    prop_assert_eq!(outcome, MatchOutcome::Incorrect);
                    prop_assert_eq!(new_state.cursor, initial_cursor);
                    prop_assert_eq!(new_state.error_count, (i + 1) as u32);
                    prop_assert!(new_state.current_char_has_error);
                    state = new_state;
                }
            }

            // Feature: typing-desktop-app, Property 4: 全部正确输入的终止条件充要性
            //
            // 对于任意长度的目标字符序列，反复对 TypingMatchState 应用 apply_input：
            // 状态被标记为已完成，当且仅当序列中的每个字符都已通过正确输入使 cursor
            // 前进过。未完成状态下 cursor 必然小于序列长度。
            //
            // Validates: Requirements 2.6, 3.5
            #[test]
            fn prop_is_complete_iff_all_chars_correctly_input(
                text in proptest::collection::vec(proptest::char::range('a', 'z'), 1..30),
            ) {
                let mut state = TypingMatchState::new(text.clone());

                // 初始状态：尚未输入任何字符，必然未完成。
                prop_assert!(!state.is_complete());
                prop_assert!(state.cursor < state.text.len());

                for (i, &c) in text.iter().enumerate() {
                    let (new_state, outcome) = apply_input(&state, c);
                    prop_assert_eq!(outcome, MatchOutcome::Correct { advanced_to: i + 1 });
                    state = new_state;

                    if i + 1 < text.len() {
                        // 尚未输入完最后一个字符：is_complete 必然为 false，
                        // 且 cursor 必然小于文本长度（充分性方向的反证：未全部
                        // 正确输入则不应被判定为完成）。
                        prop_assert!(!state.is_complete());
                        prop_assert!(state.cursor < state.text.len());
                    } else {
                        // 已正确输入最后一个字符：is_complete 必然为 true
                        // （必要性方向：全部正确输入后必须判定完成）。
                        prop_assert!(state.is_complete());
                        prop_assert_eq!(state.cursor, state.text.len());
                    }
                }

                // 完成后继续输入（无论正确与否）不应改变已完成状态，
                // 验证"充要性"在完成之后保持稳定。
                let (after_more_input, outcome) = apply_input(&state, 'z');
                prop_assert_eq!(outcome, MatchOutcome::AlreadyComplete);
                prop_assert!(after_more_input.is_complete());
            }

            // Feature: typing-desktop-app, Property 5: 键位提示的唯一性
            //
            // 对于任意未完成的 TypingMatchState，其对应的"当前待按键位提示"
            // 恒为至多一个 KeyCode（由 Option<KeyCode> 返回类型本身保证），
            // 且 current_key_hint 是确定性的：同一状态多次调用返回完全相同的
            // 结果；结果恒等于 cursor 指向字符经 char_to_key 映射得到的值。
            // 该性质在任意合法的 cursor 位置（文本中间、首位、末位）下均成立。
            //
            // Validates: Requirements 2.3
            #[test]
            fn prop_current_key_hint_is_deterministic_and_unique(
                text in proptest::collection::vec(proptest::char::any(), 1..30),
                cursor_ratio in 0.0f64..1.0,
                error_count in 0u32..1000,
                current_char_has_error in proptest::bool::ANY,
            ) {
                let cursor = ((text.len() as f64) * cursor_ratio) as usize;
                let cursor = cursor.min(text.len() - 1); // 保证未完成状态

                let state = TypingMatchState {
                    text: text.clone(),
                    cursor,
                    error_count,
                    current_char_has_error,
                };
                prop_assert!(!state.is_complete());

                // 确定性：同一状态多次调用 current_key_hint 返回完全相同的结果。
                let hint1 = current_key_hint(&state);
                let hint2 = current_key_hint(&state);
                let hint3 = current_key_hint(&state);
                prop_assert_eq!(hint1, hint2);
                prop_assert_eq!(hint2, hint3);

                // 唯一性：结果恒等于 cursor 指向字符的唯一映射结果，
                // Option<KeyCode> 的类型本身保证了"至多一个"提示键位
                // ——不存在返回集合或多个候选键位的情况。
                let expected_char = text[cursor];
                prop_assert_eq!(hint1, char_to_key(expected_char));

                // error_count 与 current_char_has_error 的变化不影响
                // current_key_hint 的结果：它只依赖 text 与 cursor。
                let state_with_different_error_flags = TypingMatchState {
                    text: text.clone(),
                    cursor,
                    error_count: error_count.wrapping_add(1),
                    current_char_has_error: !current_char_has_error,
                };
                prop_assert_eq!(
                    current_key_hint(&state_with_different_error_flags),
                    hint1
                );
            }

            // Feature: typing-desktop-app, Property 5: 键位提示的唯一性
            //
            // 对于任意已完成的 TypingMatchState（cursor 达到文本长度），
            // current_key_hint 恒返回 None——即"不存在提示键位"，而不是
            // 返回某个残留的候选键位，同样满足"至多一个"的约束（0 个）。
            //
            // Validates: Requirements 2.3
            #[test]
            fn prop_current_key_hint_is_none_when_complete(
                text in proptest::collection::vec(proptest::char::any(), 1..30),
                error_count in 0u32..1000,
            ) {
                let state = TypingMatchState {
                    text: text.clone(),
                    cursor: text.len(),
                    error_count,
                    current_char_has_error: false,
                };
                prop_assert!(state.is_complete());
                prop_assert_eq!(current_key_hint(&state), None);
                // 确定性同样适用于已完成状态。
                prop_assert_eq!(current_key_hint(&state), current_key_hint(&state));
            }
        }
    }
}
