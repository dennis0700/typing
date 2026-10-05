//! 键位 → 手指分区映射表。

/// 标准键盘键位（覆盖字母、数字、常用符号与功能键）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    // 数字行
    Backquote,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Digit0,
    Minus,
    Equal,
    // 字母行 1（QWERTY 上行）
    Q,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,
    BracketLeft,
    BracketRight,
    Backslash,
    // 字母行 2（中行）
    A,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,
    Semicolon,
    Quote,
    // 字母行 3（下行）
    Z,
    X,
    C,
    V,
    B,
    N,
    M,
    Comma,
    Period,
    Slash,
    // 拇指区
    Space,
}

/// 手指分区：按照标准指法规则，将键盘按键划分给特定手指负责的区域。
///
/// 十个分区是 Req 2.1 规定的完整模型（"10 个分区使用 10 种互不相同、可区分
/// 的颜色"），因此即使 `finger_zone_of` 当前不会返回某个分区，该变体也必须
/// 存在：空格被统一归给左拇指（键盘上只有一个空格键，必须选定一只手），
/// 于是 `RightThumb` 不会被构造出来，但它是模型完整性的一部分，且
/// `zone_color_token`/`.slint` 侧配色都按 10 个分区一一对应。
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FingerZone {
    LeftPinky,
    LeftRing,
    LeftMiddle,
    LeftIndex,
    LeftThumb,
    RightThumb,
    RightIndex,
    RightMiddle,
    RightRing,
    RightPinky,
}

/// 编译期常量映射：每个标准键位 → 唯一手指分区。
///
/// 这是一个全量覆盖的纯函数（无默认分支兜底缺失键位），保证每个 `KeyCode`
/// 变体都被显式指派到一个 `FingerZone`。
pub const fn finger_zone_of(key: KeyCode) -> FingerZone {
    match key {
        // 左手小指
        KeyCode::Backquote | KeyCode::Digit1 | KeyCode::Q | KeyCode::A | KeyCode::Z => {
            FingerZone::LeftPinky
        }

        // 左手无名指
        KeyCode::Digit2 | KeyCode::W | KeyCode::S | KeyCode::X => FingerZone::LeftRing,

        // 左手中指
        KeyCode::Digit3 | KeyCode::E | KeyCode::D | KeyCode::C => FingerZone::LeftMiddle,

        // 左手食指（含标准指法中由左手食指延伸负责的 T/G/B、4/5 键）
        KeyCode::Digit4
        | KeyCode::Digit5
        | KeyCode::R
        | KeyCode::T
        | KeyCode::F
        | KeyCode::G
        | KeyCode::V
        | KeyCode::B => FingerZone::LeftIndex,

        // 左手拇指
        // Space 在标准指法中由左右拇指共同负责，此处归入左手拇指分区
        // 以保证 finger_zone_of 为全量单值映射。
        KeyCode::Space => FingerZone::LeftThumb,

        // 右手食指（含 6/7、Y/U、H/J、N/M）
        KeyCode::Digit6
        | KeyCode::Digit7
        | KeyCode::Y
        | KeyCode::U
        | KeyCode::H
        | KeyCode::J
        | KeyCode::N
        | KeyCode::M => FingerZone::RightIndex,

        // 右手中指
        KeyCode::Digit8 | KeyCode::I | KeyCode::K | KeyCode::Comma => FingerZone::RightMiddle,

        // 右手无名指
        KeyCode::Digit9 | KeyCode::O | KeyCode::L | KeyCode::Period => FingerZone::RightRing,

        // 右手小指
        KeyCode::Digit0
        | KeyCode::Minus
        | KeyCode::Equal
        | KeyCode::P
        | KeyCode::BracketLeft
        | KeyCode::BracketRight
        | KeyCode::Backslash
        | KeyCode::Semicolon
        | KeyCode::Quote
        | KeyCode::Slash => FingerZone::RightPinky,
    }
}

/// 每个标准键位 → 其对应的可打印字符（全量覆盖的纯函数）。
///
/// 用于练习文本生成（`domain::practice_text`）等需要将 `KeyCode` 转换为
/// 实际展示/输入字符的场景，避免在多个模块中重复定义该映射。
///
/// 说明：`Space` 映射为空格字符 `' '`；其余键位映射为其在标准 QWERTY
/// 布局下不带 Shift 修饰的小写/基础字符。
pub const fn key_to_char(key: KeyCode) -> char {
    match key {
        KeyCode::Backquote => '`',
        KeyCode::Digit1 => '1',
        KeyCode::Digit2 => '2',
        KeyCode::Digit3 => '3',
        KeyCode::Digit4 => '4',
        KeyCode::Digit5 => '5',
        KeyCode::Digit6 => '6',
        KeyCode::Digit7 => '7',
        KeyCode::Digit8 => '8',
        KeyCode::Digit9 => '9',
        KeyCode::Digit0 => '0',
        KeyCode::Minus => '-',
        KeyCode::Equal => '=',
        KeyCode::Q => 'q',
        KeyCode::W => 'w',
        KeyCode::E => 'e',
        KeyCode::R => 'r',
        KeyCode::T => 't',
        KeyCode::Y => 'y',
        KeyCode::U => 'u',
        KeyCode::I => 'i',
        KeyCode::O => 'o',
        KeyCode::P => 'p',
        KeyCode::BracketLeft => '[',
        KeyCode::BracketRight => ']',
        KeyCode::Backslash => '\\',
        KeyCode::A => 'a',
        KeyCode::S => 's',
        KeyCode::D => 'd',
        KeyCode::F => 'f',
        KeyCode::G => 'g',
        KeyCode::H => 'h',
        KeyCode::J => 'j',
        KeyCode::K => 'k',
        KeyCode::L => 'l',
        KeyCode::Semicolon => ';',
        KeyCode::Quote => '\'',
        KeyCode::Z => 'z',
        KeyCode::X => 'x',
        KeyCode::C => 'c',
        KeyCode::V => 'v',
        KeyCode::B => 'b',
        KeyCode::N => 'n',
        KeyCode::M => 'm',
        KeyCode::Comma => ',',
        KeyCode::Period => '.',
        KeyCode::Slash => '/',
        KeyCode::Space => ' ',
    }
}

/// 每个标准键位 → 其 Shift 组合字符（全量覆盖的纯函数，`Space` 除外无 Shift 变体）。
///
/// 与 `key_to_char` 是同一把物理键位在两种修饰状态下的两个取值：`key_to_char`
/// 给出不按 Shift 时的字符，`shifted_char` 给出同一按键**按住 Shift**时的字符。
/// 二者共同构成对物理键盘（Apple Magic Keyboard 87 键 / 标准 ANSI 布局）的
/// 完整字符覆盖——`KeyCode` 仍然只表示物理键位本身，Shift 是叠加在其上的
/// 独立维度，不会让 `KeyCode` 变体数量翻倍。
///
/// - 字母键（`Q`-`P`、`A`-`L`、`Z`-`M`）：Shift 组合是其大写字母。
/// - 数字行与符号键：Shift 组合是对应的符号（如 `Digit1` -> `!`、
///   `Semicolon` -> `:`、`Slash` -> `?`），与你贴的键盘图片一一对应。
/// - `Space`：没有 Shift 变体，返回 `None`（空格在任何修饰状态下都是空格，
///   由调用方直接使用 `key_to_char` 处理，不通过本函数）。
pub const fn shifted_char(key: KeyCode) -> Option<char> {
    match key {
        KeyCode::Backquote => Some('~'),
        KeyCode::Digit1 => Some('!'),
        KeyCode::Digit2 => Some('@'),
        KeyCode::Digit3 => Some('#'),
        KeyCode::Digit4 => Some('$'),
        KeyCode::Digit5 => Some('%'),
        KeyCode::Digit6 => Some('^'),
        KeyCode::Digit7 => Some('&'),
        KeyCode::Digit8 => Some('*'),
        KeyCode::Digit9 => Some('('),
        KeyCode::Digit0 => Some(')'),
        KeyCode::Minus => Some('_'),
        KeyCode::Equal => Some('+'),
        KeyCode::Q => Some('Q'),
        KeyCode::W => Some('W'),
        KeyCode::E => Some('E'),
        KeyCode::R => Some('R'),
        KeyCode::T => Some('T'),
        KeyCode::Y => Some('Y'),
        KeyCode::U => Some('U'),
        KeyCode::I => Some('I'),
        KeyCode::O => Some('O'),
        KeyCode::P => Some('P'),
        KeyCode::BracketLeft => Some('{'),
        KeyCode::BracketRight => Some('}'),
        KeyCode::Backslash => Some('|'),
        KeyCode::A => Some('A'),
        KeyCode::S => Some('S'),
        KeyCode::D => Some('D'),
        KeyCode::F => Some('F'),
        KeyCode::G => Some('G'),
        KeyCode::H => Some('H'),
        KeyCode::J => Some('J'),
        KeyCode::K => Some('K'),
        KeyCode::L => Some('L'),
        KeyCode::Semicolon => Some(':'),
        KeyCode::Quote => Some('"'),
        KeyCode::Z => Some('Z'),
        KeyCode::X => Some('X'),
        KeyCode::C => Some('C'),
        KeyCode::V => Some('V'),
        KeyCode::B => Some('B'),
        KeyCode::N => Some('N'),
        KeyCode::M => Some('M'),
        KeyCode::Comma => Some('<'),
        KeyCode::Period => Some('>'),
        KeyCode::Slash => Some('?'),
        KeyCode::Space => None,
    }
}

/// 纯函数：判定一个字符在标准键盘上是否需要按住 Shift 才能输入。
///
/// 仅对存在物理键位映射的字符有意义（即 `char_to_key` 返回 `Some` 的字符）：
/// - 大写字母 `'A'`-`'Z'`、Shift 组合符号（`~!@#$%^&*()_+{}|:"<>?`）返回 `true`。
/// - 小写字母、数字、基础符号、空格返回 `false`。
/// - 不在键盘映射范围内的字符（如中文字符）返回 `false`——它们本身就没有
///   对应的物理键位，"是否需要 Shift"这一问题对它们不适用，调用方通常会先
///   用 `char_to_key` 判空再决定是否需要这一信息。
pub fn needs_shift(c: char) -> bool {
    if c.is_ascii_uppercase() {
        return true;
    }
    matches!(
        c,
        '~' | '!' | '@' | '#' | '$' | '%' | '^' | '&' | '*' | '(' | ')' | '_' | '+' | '{' | '}' | '|' | ':' | '"' | '<' | '>' | '?'
    )
}

/// 字符 → 键位的反向映射（`key_to_char`/`shifted_char` 的联合逆函数）。
///
/// 说明：`key_to_char` 与 `shifted_char` 共同构成对标准键盘可打印字符的完整
/// 覆盖（基础字符 + Shift 组合字符），因此该反向映射能覆盖两者的并集：
/// - 基础字符（数字行符号、`a`-`z` 小写字母、空格）：返回对应的 `KeyCode`。
/// - Shift 组合字符（大写字母 `'A'`-`'Z'`、`~!@#$%^&*()_+{}|:"<>?`）：返回其
///   物理键位对应的同一个 `KeyCode`（与不按 Shift 时相同的物理键，因为
///   `KeyCode` 只表示物理键位，不区分修饰状态）——调用方若需要知道"这个字符
///   是否需要按 Shift"，应额外调用 [`needs_shift`]。
/// - 其余字符（未映射的符号、任意非 ASCII 字符）：返回 `None`，表示该字符
///   没有对应的单一按键位置提示（调用方应据此不展示键位提示，而不是 panic）。
pub fn char_to_key(c: char) -> Option<KeyCode> {
    match c {
        '`' | '~' => Some(KeyCode::Backquote),
        '1' | '!' => Some(KeyCode::Digit1),
        '2' | '@' => Some(KeyCode::Digit2),
        '3' | '#' => Some(KeyCode::Digit3),
        '4' | '$' => Some(KeyCode::Digit4),
        '5' | '%' => Some(KeyCode::Digit5),
        '6' | '^' => Some(KeyCode::Digit6),
        '7' | '&' => Some(KeyCode::Digit7),
        '8' | '*' => Some(KeyCode::Digit8),
        '9' | '(' => Some(KeyCode::Digit9),
        '0' | ')' => Some(KeyCode::Digit0),
        '-' | '_' => Some(KeyCode::Minus),
        '=' | '+' => Some(KeyCode::Equal),
        'q' | 'Q' => Some(KeyCode::Q),
        'w' | 'W' => Some(KeyCode::W),
        'e' | 'E' => Some(KeyCode::E),
        'r' | 'R' => Some(KeyCode::R),
        't' | 'T' => Some(KeyCode::T),
        'y' | 'Y' => Some(KeyCode::Y),
        'u' | 'U' => Some(KeyCode::U),
        'i' | 'I' => Some(KeyCode::I),
        'o' | 'O' => Some(KeyCode::O),
        'p' | 'P' => Some(KeyCode::P),
        '[' | '{' => Some(KeyCode::BracketLeft),
        ']' | '}' => Some(KeyCode::BracketRight),
        '\\' | '|' => Some(KeyCode::Backslash),
        'a' | 'A' => Some(KeyCode::A),
        's' | 'S' => Some(KeyCode::S),
        'd' | 'D' => Some(KeyCode::D),
        'f' | 'F' => Some(KeyCode::F),
        'g' | 'G' => Some(KeyCode::G),
        'h' | 'H' => Some(KeyCode::H),
        'j' | 'J' => Some(KeyCode::J),
        'k' | 'K' => Some(KeyCode::K),
        'l' | 'L' => Some(KeyCode::L),
        ';' | ':' => Some(KeyCode::Semicolon),
        '\'' | '"' => Some(KeyCode::Quote),
        'z' | 'Z' => Some(KeyCode::Z),
        'x' | 'X' => Some(KeyCode::X),
        'c' | 'C' => Some(KeyCode::C),
        'v' | 'V' => Some(KeyCode::V),
        'b' | 'B' => Some(KeyCode::B),
        'n' | 'N' => Some(KeyCode::N),
        'm' | 'M' => Some(KeyCode::M),
        ',' | '<' => Some(KeyCode::Comma),
        '.' | '>' => Some(KeyCode::Period),
        '/' | '?' => Some(KeyCode::Slash),
        ' ' => Some(KeyCode::Space),
        _ => None,
    }
}

/// 每个手指分区 → 唯一颜色 token（与 slintcn 主题色变量对应）。
///
/// 全量覆盖的纯函数：10 个分区对应 10 个互不相同的颜色 token 字符串常量。
///
/// 渲染路径上没有调用方：`.slint` 无法调用任意 Rust 函数做 token -> color
/// 的解析，因此虚拟键盘的实际配色由 `virtual_keyboard.slint` 的
/// `FingerZoneColors` global 直接定义（该文件顶部详细记录了这一取舍）。
/// 本函数保留的价值在于它的属性测试：以"10 个 token 两两互不相同"的形式
/// 把 Req 2.1 的可区分性约束固定在 Rust 侧可自动验证的位置。
#[cfg_attr(not(test), allow(dead_code))]
pub const fn zone_color_token(zone: FingerZone) -> &'static str {
    match zone {
        FingerZone::LeftPinky => "finger-zone-left-pinky",
        FingerZone::LeftRing => "finger-zone-left-ring",
        FingerZone::LeftMiddle => "finger-zone-left-middle",
        FingerZone::LeftIndex => "finger-zone-left-index",
        FingerZone::LeftThumb => "finger-zone-left-thumb",
        FingerZone::RightThumb => "finger-zone-right-thumb",
        FingerZone::RightIndex => "finger-zone-right-index",
        FingerZone::RightMiddle => "finger-zone-right-middle",
        FingerZone::RightRing => "finger-zone-right-ring",
        FingerZone::RightPinky => "finger-zone-right-pinky",
    }
}

/// 全部标准键位的枚举值列表，供测试中生成任意 `KeyCode` 使用。
#[cfg(test)]
const ALL_KEY_CODES: &[KeyCode] = &[
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
];

/// 全部手指分区的枚举值列表，供测试中生成任意 `FingerZone` 使用。
#[cfg(test)]
const ALL_FINGER_ZONES: &[FingerZone] = &[
    FingerZone::LeftPinky,
    FingerZone::LeftRing,
    FingerZone::LeftMiddle,
    FingerZone::LeftIndex,
    FingerZone::LeftThumb,
    FingerZone::RightThumb,
    FingerZone::RightIndex,
    FingerZone::RightMiddle,
    FingerZone::RightRing,
    FingerZone::RightPinky,
];

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 生成任意 `KeyCode` 的策略（`KeyCode` 未派生 `Arbitrary`，通过枚举全量变体实现）。
    fn arb_key_code() -> impl Strategy<Value = KeyCode> {
        prop::sample::select(ALL_KEY_CODES)
    }

    /// 生成任意 `FingerZone` 的策略（同上，通过枚举全量变体实现）。
    fn arb_finger_zone() -> impl Strategy<Value = FingerZone> {
        prop::sample::select(ALL_FINGER_ZONES)
    }

    // Feature: typing-desktop-app, Property 6: 手指分区颜色映射的一致性与唯一性
    proptest! {
        #[test]
        fn prop_finger_zone_color_consistency_and_uniqueness(
            k1 in arb_key_code(),
            k2 in arb_key_code(),
        ) {
            let z1 = finger_zone_of(k1);
            let z2 = finger_zone_of(k2);

            // 一致性：同一手指分区（即使来自不同键位）映射到相同颜色 token。
            if z1 == z2 {
                prop_assert_eq!(zone_color_token(z1), zone_color_token(z2));
            } else {
                // 唯一性：不同手指分区映射到不同颜色 token。
                prop_assert_ne!(zone_color_token(z1), zone_color_token(z2));
            }
        }
    }

    // Feature: typing-desktop-app, Property 6: 手指分区颜色映射的一致性与唯一性
    proptest! {
        #[test]
        fn prop_zone_color_token_pairwise_uniqueness_across_all_zones(
            z1 in arb_finger_zone(),
            z2 in arb_finger_zone(),
        ) {
            // 覆盖全部 10 个分区两两组合：分区相同 <=> 颜色 token 相同。
            if z1 == z2 {
                prop_assert_eq!(zone_color_token(z1), zone_color_token(z2));
            } else {
                prop_assert_ne!(zone_color_token(z1), zone_color_token(z2));
            }
        }
    }

    // Feature: typing-desktop-app, Property 6: 手指分区颜色映射的一致性与唯一性
    #[test]
    fn prop_finger_zone_of_is_total_and_deterministic() {
        // finger_zone_of 对每个 KeyCode 变体都有定义（全量覆盖），
        // 且对同一输入多次调用返回相同结果（确定性）。
        for &key in ALL_KEY_CODES {
            let z1 = finger_zone_of(key);
            let z2 = finger_zone_of(key);
            assert_eq!(z1, z2, "finger_zone_of 对相同输入应返回相同结果: {key:?}");
        }
    }

    #[test]
    fn key_to_char_is_total_and_deterministic() {
        // key_to_char 对每个 KeyCode 变体都有定义（全量覆盖），
        // 且对同一输入多次调用返回相同结果（确定性）。
        for &key in ALL_KEY_CODES {
            let c1 = key_to_char(key);
            let c2 = key_to_char(key);
            assert_eq!(c1, c2, "key_to_char 对相同输入应返回相同结果: {key:?}");
        }
    }

    #[test]
    fn key_to_char_space_maps_to_space_character() {
        assert_eq!(key_to_char(KeyCode::Space), ' ');
    }

    #[test]
    fn char_to_key_is_inverse_of_key_to_char_for_all_key_codes() {
        // key_to_char 的值域内的每个字符都能通过 char_to_key 反向映射回
        // 唯一对应的 KeyCode（往返一致性）。
        for &key in ALL_KEY_CODES {
            let c = key_to_char(key);
            assert_eq!(
                char_to_key(c),
                Some(key),
                "char_to_key(key_to_char({key:?})) 应等于 Some({key:?})"
            );
        }
    }

    #[test]
    fn char_to_key_resolves_uppercase_letters_to_their_physical_key() {
        // 大写字母是同一物理键位在 Shift 修饰下的取值，char_to_key 应返回
        // 与对应小写字母相同的 KeyCode（物理键位不因修饰状态而改变）。
        for c in 'A'..='Z' {
            let lower = c.to_ascii_lowercase();
            assert_eq!(
                char_to_key(c),
                char_to_key(lower),
                "大写字母 {c:?} 应与小写字母 {lower:?} 映射到同一物理键位"
            );
            assert!(char_to_key(c).is_some(), "大写字母 {c:?} 应有对应的 KeyCode");
        }
    }

    #[test]
    fn char_to_key_resolves_shift_symbols_to_their_physical_key() {
        // Shift 组合符号同样是已有物理键位的 Shift 取值，不是未映射字符。
        for c in ['@', '#', '$', '%', '^', '&', '*', '_', '+', '~', '!', '?', ':', '"', '<', '>', '{', '}', '|'] {
            assert!(char_to_key(c).is_some(), "Shift 组合符号 {c:?} 应有对应的 KeyCode");
        }
    }

    #[test]
    fn char_to_key_returns_none_for_unmapped_characters() {
        // 任意非 ASCII 字符（键盘上没有对应物理键位）应返回 None。
        assert_eq!(char_to_key('中'), None);
    }

    #[test]
    fn shifted_char_is_inverse_consistent_with_char_to_key_for_all_key_codes() {
        // shifted_char 的值域内的每个字符都能通过 char_to_key 反向映射回
        // 同一个 KeyCode（往返一致性），与 key_to_char 的性质对称。
        for &key in ALL_KEY_CODES {
            if let Some(c) = shifted_char(key) {
                assert_eq!(
                    char_to_key(c),
                    Some(key),
                    "char_to_key(shifted_char({key:?})) 应等于 Some({key:?})"
                );
            }
        }
    }

    #[test]
    fn shifted_char_returns_none_only_for_space() {
        for &key in ALL_KEY_CODES {
            if key == KeyCode::Space {
                assert_eq!(shifted_char(key), None);
            } else {
                assert!(shifted_char(key).is_some(), "{key:?} 应有 Shift 组合字符");
            }
        }
    }

    #[test]
    fn needs_shift_is_true_for_uppercase_and_shift_symbols_false_otherwise() {
        for c in 'A'..='Z' {
            assert!(needs_shift(c), "大写字母 {c:?} 应需要 Shift");
        }
        for c in 'a'..='z' {
            assert!(!needs_shift(c), "小写字母 {c:?} 不应需要 Shift");
        }
        for c in "~!@#$%^&*()_+{}|:\"<>?".chars() {
            assert!(needs_shift(c), "Shift 组合符号 {c:?} 应需要 Shift");
        }
        for c in "`1234567890-=[]\\;',./".chars() {
            assert!(!needs_shift(c), "基础符号 {c:?} 不应需要 Shift");
        }
        assert!(!needs_shift(' '));
        assert!(!needs_shift('中'));
    }

    // Feature: typing-desktop-app, Property 5: 键位提示的唯一性
    proptest! {
        #[test]
        fn prop_char_to_key_is_deterministic_and_at_most_one_result(c in proptest::char::any()) {
            // char_to_key 对同一输入始终返回相同结果（确定性），且返回类型
            // Option<KeyCode> 本身保证了"至多一个"结果（不存在多值映射）。
            let r1 = char_to_key(c);
            let r2 = char_to_key(c);
            prop_assert_eq!(r1, r2);
        }
    }

    #[test]
    fn all_ten_zones_have_distinct_color_tokens() {
        // 补充示例：显式验证 10 个分区的颜色 token 两两不同（唯一性的直接检验）。
        let tokens: Vec<&'static str> = ALL_FINGER_ZONES
            .iter()
            .map(|&zone| zone_color_token(zone))
            .collect();
        let unique: std::collections::HashSet<&&str> = tokens.iter().collect();
        assert_eq!(
            unique.len(),
            ALL_FINGER_ZONES.len(),
            "10 个手指分区必须映射到 10 个互不相同的颜色 token"
        );
    }
}
