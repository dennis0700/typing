//! Slint 组件加载与属性/回调绑定桥接层（对 .slint 生成代码的封装）。
//!
//! 本文件实现任务 18.8：将 `AppWindow` 暴露的 Slint 回调转换为
//! `app_state::AppEvent`，派发给 `AppController::dispatch`，并将返回的
//! `Vec<UiCommand>` 应用到 `AppWindow` 的属性/导航状态；同时在每次
//! `dispatch` 之后，把 `AppController` 内部最新的只读展示数据（档案列表、
//! 课程卡片、练习字符/按键状态等）重新计算并写回对应的 Slint 属性。
//!
//! 架构总览：
//! - `run_app()` 是新的应用入口：构造 `AppController`、创建 `AppWindow`，
//!   用 `Rc<RefCell<AppController>>` 包裹控制器以便在多个回调闭包间共享
//!   可变状态（Slint 官方推荐的模式：闭包持有 `Weak<AppWindow>` +
//!   `Rc<RefCell<..>>`，避免闭包与窗口之间出现循环引用）。
//! - 每个 Slint 回调（`select-profile`/`create-profile`/`lesson-selected`/
//!   `locked-lesson-tapped`/`practice-poll-elapsed`/`practice-exit-requested`/
//!   `practice-char-input`/`toast-dismissed`/`reward-dismissed`/
//!   `result-dismissed`/`answer-hint-continue`）
//!   都注册一个 Rust 闭包：转换参数为 `AppEvent` -> `dispatch` -> 应用
//!   `UiCommand` -> 刷新当前页面的展示数据。
//! - `create_main_window()` 保留，供既有测试与其他可能的调用方使用。
//!
//! ## 物理键盘输入的派发：为什么用 `CharInput` 而不是 `KeyPressed`
//!
//! `AppController::dispatch` 对两个变体的处理是：
//! `KeyPressed(kc) => handle_char_input(key_to_char(kc))`、
//! `CharInput(c) => handle_char_input(c)`——即二者走同一个状态机，唯一区别
//! 是 `KeyPressed` 先把 `KeyCode` 映射成字符。
//!
//! 物理键盘事件从 Slint 传来的是 `KeyEvent.text`（一个字符串），本身已经
//! 是字符。若改走 `KeyPressed`，桥接层必须先 `char_to_key(c)` 把字符映射回
//! `KeyCode`、再由 `dispatch` 内部 `key_to_char` 映射回字符，这一来一回是
//! **有损**的：`char_to_key` 只保留"按哪个物理键"这一信息，丢弃了"是否按住
//! Shift"——大写字母 `'A'` 与小写 `'a'` 都映射到 `KeyCode::A`，但
//! `key_to_char(KeyCode::A)` 恒返回小写 `'a'`，往返后大写会被错误地还原成
//! 小写；对没有物理键位的字符（中文标点等），`char_to_key` 返回 `None`，
//! 那些输入会在桥接层被静默丢弃，而 `apply_input` 本来可以正确地把它们
//! 判定为错误输入（Req 3.3）。因此这里直接派发 `AppEvent::CharInput(c)`，
//! 不做无谓的往返映射。
//!
//! `AppEvent::KeyPressed` 目前没有任何派发点——它是留给虚拟键盘点击输入的
//! 入口（`virtual_keyboard.slint` 的 `KeyButton` 当前是纯展示元素，不暴露
//! 点击回调），本次接线不涉及。
//!
//! ## `UiCommand` 变体的 UI 接线情况
//!
//! | 变体 | 接线状态 |
//! |---|---|
//! | `NavigateTo` | 完整接线：写入 `current-page`，并顺带清空 `nickname-error`（离开档案选择界面后该内联错误提示不应残留）。 |
//! | `ShowToast` | 完整接线：按当前页面路由到两个展示位之一——档案选择界面写入 `nickname-error`（内联展示在昵称输入框旁，满足 Req 6.5/6.7"指明具体原因 + 保留用户输入"），其他页面写入全局 Toast 浮层（`toast.slint` 的 `Toast`）。路由理由见 [`toast_target_for_page`] 的文档注释。 |
//! | `ShowEncouragement` | 完整接线：写入全局 Toast 浮层（`toast-message`），短暂展示后由 `toast-dismissed()` 回调清空（Req 7.2）。 |
//! | `ShowCorrectAnswer` | 完整接线：写入 `AnswerHint` 浮层（`answer-hint-message` + `answer-hint-answer`，后者是当前待输入字符即"正确答案"）。该浮层带一个 ≥44×44px 的"继续下一题"按钮，点击后派发 `AppEvent::SkipCurrentItem`（Req 7.6、7.1）。 |
//! | `ShowRewardFeedback` | 完整接线：`RewardFeedback` 各字段写入 `reward-*` 属性并置 `reward-active: true`，`RewardOverlay` 的 `Timer` 到时通过 `reward-dismissed()` 回调关闭（Req 7.3）。 |
//! | `ShowPracticeResult` | 完整接线：写入 `result-*` 属性并调用 `domain::compare_with_best` 计算与历史最佳的比较结果（Req 4.1, 4.2, 4.4, 4.5）。与奖励反馈的展示顺序由 [`PendingOverlays`] 串联，见其文档注释。 |
//!
//! ### 仍存在的局限
//!
//! - 「历史最佳成绩」的取法受 `CurriculumState::record_practice_result` 的
//!   时序限制，需要从 `history` 中剔除本次成绩后再取最佳，详见
//!   [`previous_best_excluding_current`] 的文档注释。
//! - `AppEvent::SkipCurrentItem` 的业务语义本身仍是有限的：
//!   `AppController::handle_skip_current_item` 只重置连续错误计数、不切换
//!   题目（其文档注释说明了原因——设计文档未定义"题目"与"练习文本"的拆分
//!   关系）。桥接层只负责正确派发该事件，不改变其业务语义。
//! - 各浮层的 root 均为 `Rectangle`（不是 `TouchArea`），因此展示时并不会
//!   真正拦截下方页面的点击事件——即孩子在结果统计/奖励浮层可见时仍能点到
//!   下方课程序列界面的卡片。这与 `reward_overlay.slint` 既有的行为一致，
//!   若未来需要真正的模态遮罩，应在各浮层 root 内补一个全屏 `TouchArea`。
//! - 同理，浮层可见时**物理键盘输入仍会被接受**：`AnswerHint`（Req 7.6 的
//!   "展示正确答案"）与 `Toast`/`RewardOverlay` 都不含 `FocusScope`，键盘
//!   焦点始终留在 `practice_view.slint` 的 `key-scope` 上，因此
//!   `practice-char-input` 会继续派发 `AppEvent::CharInput`，孩子可以在
//!   看着正确答案的同时直接把它敲出来。这是当前有意保留的行为（Req 7.6
//!   要求的是"允许学员选择继续下一题目"，并未要求在展示答案时禁止输入）。
//!   若未来需要在浮层可见时屏蔽输入，应在浮层内自建 `FocusScope` 抢占焦点，
//!   而不是在桥接层按浮层可见性做条件过滤。
//!
//! ## 内置课程数据
//!
//! 真实的内置课程序列由 `domain::builtin_curriculum()` 提供（12 个课程，
//! 覆盖"基准键位 → 上排 → 下排 → 字母全集复习 → 词语 → 句子"的完整
//! 学习路径，Req 1.1、1.3），`run_app()` 直接使用该数据构造
//! `AppController`/`CurriculumState`。此前用于让应用可编译/可启动的占位
//! 课程序列（`placeholder_curriculum()`）已被移除。

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;

use slint::{Model, ModelRc, VecModel};

use crate::app_state::{AppController, AppEvent, NavigationTarget, UiCommand};
use crate::audio::{ClickSound, KeyClickPlayer};
use crate::domain::{
    self, CharState, ComparisonResult, Curriculum, KeyCode, LessonGoal,
    LessonState as DomainLessonState, char_state_at, char_to_key, compare_with_best,
    current_key_hint, finger_zone_of, key_to_char,
};
use crate::storage::{self, ProfileId};
use crate::trace;

/// 创建应用顶层窗口组件实例。
pub fn create_main_window() -> Result<AppWindow, slint::PlatformError> {
    AppWindow::new()
}

/// 应用入口：构造 `AppController`、创建 `AppWindow`，注册全部回调桥接，
/// 并启动 Slint 事件循环。
///
/// 必须在主线程调用（Slint 在 macOS 上通过 `winit` 要求窗口在主线程创建/
/// 运行事件循环）。
///
/// # 开发期调试开关（**仅开发期调试用，非产品功能**）
///
/// 以下两个环境变量都不是需求文档定义的功能，只为定位"创建档案后课程序列
/// 界面显示不出来"这类必须在真实窗口里才能复现的渲染问题而存在；相关代码
/// 应在问题定位完成后连同 [`crate::trace`] 模块一并移除。
///
/// - `TYPING_DEV_STORE_DIR`：覆盖 [`storage::ProfileStore`] 的基础目录。设置
///   后档案读写全部落在该目录，而不是
///   `~/Library/Application Support/typing/profiles`——**避免开发期反复
///   autostart 造成的调试档案污染用户真实的学员数据**。
/// - `TYPING_DEV_AUTOSTART`：设为 `1`/`true` 时，在 `window.run()` **之前**
///   自动走一遍"创建档案"流程（见 [`dev_autostart`]），使窗口一显示就已经
///   处于课程序列界面，从而无需鼠标点击即可复现该页面的渲染状态。
///
/// 两个开关都只在 `run_app()` 的启动路径上生效，不影响任何业务逻辑：
/// autostart 走的是与用户真实点击"创建"完全相同的
/// `dispatch` -> `apply_commands` 路径，没有任何捷径。
pub fn run_app() -> Result<(), Box<dyn std::error::Error>> {
    let curriculum = domain::builtin_curriculum();
    let controller = Rc::new(RefCell::new(build_controller(curriculum)?));
    let pending = Rc::new(RefCell::new(PendingOverlays::default()));

    // 按键音效播放器。设备打开失败或 `TYPING_MUTE=1` 时为 `None`，此后所有
    // 播放请求都被静默跳过——没有声音不影响任何功能。
    let clicks = Rc::new(KeyClickPlayer::new());

    let window = create_main_window()?;

    register_callbacks(&window, &controller, &pending, &clicks);

    // 启动时立即刷新一次档案列表，使档案选择界面在首次展示时即为最新状态
    // （对应 Req 6.2：启动时列出所有已创建档案及其昵称）。
    refresh_profiles(&window, &controller.borrow());

    crate::trace!(
        "run_app: 窗口已创建，current-page={:?}",
        window.get_current_page()
    );

    if trace::env_flag_enabled(DEV_AUTOSTART_ENV) {
        dev_autostart(&window, &controller, &pending);
    }

    window.run()?;

    Ok(())
}

/// 覆盖档案存储目录的开发期环境变量名（**仅开发期调试用**，见
/// [`run_app`] 的文档注释）。
const DEV_STORE_DIR_ENV: &str = "TYPING_DEV_STORE_DIR";

/// 开启启动时自动进入课程序列界面的开发期环境变量名（**仅开发期调试用**，
/// 见 [`run_app`] 与 [`dev_autostart`] 的文档注释）。
const DEV_AUTOSTART_ENV: &str = "TYPING_DEV_AUTOSTART";

/// [`dev_autostart`] 创建调试档案时使用的固定昵称（**仅开发期调试用**）。
///
/// 取固定值而不是随机值，使得反复 autostart 时不会在调试用的 store 目录里
/// 堆积无数档案；第二次及以后的 autostart 会因"昵称已被使用"而创建失败，
/// 此时 [`dev_autostart`] 自动退化为选择已存在的档案。
const DEV_AUTOSTART_NICKNAME: &str = "调试档案";

/// 构造 `AppController`：默认使用 macOS 标准档案目录，但当开发期环境变量
/// `TYPING_DEV_STORE_DIR` 被设置时改用该目录（**仅开发期调试用**，见
/// [`run_app`] 的文档注释）。
fn build_controller(curriculum: Curriculum) -> std::io::Result<AppController> {
    match std::env::var_os(DEV_STORE_DIR_ENV) {
        Some(dir) if !dir.is_empty() => {
            crate::trace!(
                "build_controller: 使用开发期 store 目录 {DEV_STORE_DIR_ENV}={dir:?}（不触碰用户真实档案）"
            );
            let store = storage::ProfileStore::new(std::path::PathBuf::from(dir));
            Ok(AppController::new(store, curriculum))
        }
        _ => {
            crate::trace!("build_controller: 使用默认 store 目录");
            AppController::new_with_default_store(curriculum)
        }
    }
}

/// **仅开发期调试用，非产品功能**：在 `window.run()` 之前免交互地把应用推进
/// 到课程序列界面，用于复现"创建档案后课程序列界面显示不出来"的渲染问题。
///
/// 之所以需要这个开关：到达课程序列界面的唯一真实路径是用鼠标点击"创建"
/// 或某个已有档案，而 Slint 窗口在 macOS 上只能在主线程运行、`cargo test`
/// 里根本无法实例化 `AppWindow`，因此该页面的渲染状态既无法自动化测试、
/// 也无法在无人交互的环境下复现。
///
/// 路径选择（两条都经由 `dispatch` -> [`apply_commands`]，与用户真实点击
/// 完全同一条代码路径，不另开捷径）：
/// 1. 先派发 `AppEvent::CreateProfile`；若成功（页面被切到 `CurriculumMap`），
///    到此结束。
/// 2. 若创建失败（昵称已被上一次 autostart 占用，`AppController` 返回的是
///    `ShowToast`），退化为对已存在的第一个档案派发
///    `AppEvent::SelectProfile`，保证 autostart 总能到达课程序列界面。
///
/// 实际走的是哪条路径会被 trace 出来（`路径=CreateProfile` /
/// `路径=SelectProfile` / `路径=失败`）。
fn dev_autostart(
    window: &AppWindow,
    controller: &Rc<RefCell<AppController>>,
    pending: &Rc<RefCell<PendingOverlays>>,
) {
    crate::trace!("dev_autostart: 开始（昵称={DEV_AUTOSTART_NICKNAME}）");

    let commands = dispatch_traced(
        controller,
        AppEvent::CreateProfile {
            nickname: DEV_AUTOSTART_NICKNAME.to_string(),
        },
    );
    apply_commands(
        window,
        &controller.borrow(),
        &mut pending.borrow_mut(),
        commands,
    );

    if window.get_current_page() == AppPage::CurriculumMap {
        crate::trace!("dev_autostart: 路径=CreateProfile（新建档案成功并已进入课程序列界面）");
        return;
    }

    // 创建失败（多半是昵称已被上一次 autostart 占用）：退化为选择第一个
    // 已存在的档案。
    let first_profile = controller
        .borrow()
        .profile_manager()
        .list_profiles()
        .first()
        .map(|summary| summary.profile_id);

    let Some(profile_id) = first_profile else {
        crate::trace!(
            "dev_autostart: 路径=失败（新建档案未导航且 store 中没有任何已存在档案，无法到达课程序列界面）"
        );
        return;
    };

    crate::trace!(
        "dev_autostart: 新建档案未导航，退化为选择已存在档案 {}",
        format_profile_id(profile_id)
    );
    let commands = dispatch_traced(controller, AppEvent::SelectProfile(profile_id));
    apply_commands(
        window,
        &controller.borrow(),
        &mut pending.borrow_mut(),
        commands,
    );
    crate::trace!(
        "dev_autostart: 路径=SelectProfile 完成，current-page={:?}",
        window.get_current_page()
    );
}

/// 带 trace 的 `AppController::dispatch` 包装：在派发前后各输出一行 trace
/// （派发的 `AppEvent` 与返回的 `Vec<UiCommand>`），供开发期定位"事件到底
/// 派发出去了没、控制器回了什么命令"。
///
/// 除 trace 之外与直接调用 `controller.borrow_mut().dispatch(event)` 完全
/// 等价；`RefCell` 的可变借用在函数返回前即结束，因此调用方随后仍可安全地
/// `controller.borrow()`。
fn dispatch_traced(controller: &Rc<RefCell<AppController>>, event: AppEvent) -> Vec<UiCommand> {
    crate::trace!("dispatch -> {event:?}");
    let commands = controller.borrow_mut().dispatch(event);
    crate::trace!("dispatch <- {commands:?}");
    commands
}

/// 浮层展示顺序的暂存状态：解决「奖励反馈」与「结果统计」同时被要求展示时
/// 互相遮挡的问题。
///
/// 背景：`AppController::handle_practice_completion()` 在同一批命令里同时
/// 返回 `ShowPracticeResult(..)` 与 `ShowRewardFeedback(..)`（前者在命令
/// 序列中更靠前）。若两者都立刻置为 `active`，两个全屏浮层会叠在一起，
/// 孩子既看不清庆祝动画也看不清成绩。
///
/// 因此桥接层采用「先庆祝、后看成绩」的串联顺序：
/// 1. 应用 `ShowPracticeResult` 时**不**立即展示，只把算好的
///    [`ResultSummaryView`] 暂存到 `pending_result`。
/// 2. 应用 `ShowRewardFeedback` 时立即展示 `RewardOverlay`。
/// 3. 一批命令全部应用完毕后（[`apply_commands`] 末尾）：若奖励浮层并未被
///    激活（例如某个只返回 `ShowPracticeResult` 而不带奖励反馈的未来调用
///    路径），则立刻把暂存的结果统计展示出来，避免成绩数据被静默丢弃。
/// 4. 若奖励浮层已激活，则等它的 `Timer` 到时触发 `reward-dismissed()`，
///    在该回调里关闭奖励浮层并把暂存的结果统计置为 active。
/// 5. 孩子点"继续"关闭结果统计（`result-dismissed()`），回到下方已经由
///    `NavigateTo(CurriculumSequence)` 切换好的课程序列页。
#[derive(Default)]
struct PendingOverlays {
    /// 已算好但尚未展示的结果统计数据；`None` 表示当前没有待展示的成绩。
    pending_result: Option<ResultSummaryView>,
}

/// 结果统计浮层所需的全部展示数据（`ResultSummary` 组件属性的 Rust 侧镜像）。
///
/// 单独抽出这个结构体是为了让「`PracticeResult` + 历史最佳 -> 展示数据」的
/// 转换成为一个可单测的纯函数（[`build_result_summary_view`]），而不必实例化
/// Slint 窗口（Slint 窗口要求主线程，`cargo test` 无法实例化）。
#[derive(Debug, Clone, PartialEq)]
struct ResultSummaryView {
    duration_ms: i32,
    error_count: i32,
    accuracy: f32,
    wpm: f32,
    comparison_direction: ComparisonDirection,
    accuracy_delta: f32,
    wpm_delta: f32,
}

/// 奖励反馈浮层所需的全部展示数据（`RewardOverlay` 组件属性的 Rust 侧镜像）。
///
/// 与 [`ResultSummaryView`] 同理，抽出结构体使
/// `RewardFeedback -> Slint 属性` 的转换成为可单测的纯函数
/// （[`build_reward_overlay_view`]）。
#[derive(Debug, Clone, PartialEq)]
struct RewardOverlayView {
    show_animation: bool,
    show_badge: bool,
    encouragement_text: String,
    display_duration_ms: i32,
}

/// 注册 `AppWindow` 暴露的全部回调：将 Slint 侧事件转换为 `AppEvent`，
/// 派发给 `AppController::dispatch`，应用返回的 `UiCommand`，并刷新受影响
/// 页面的展示数据。
fn register_callbacks(
    window: &AppWindow,
    controller: &Rc<RefCell<AppController>>,
    pending: &Rc<RefCell<PendingOverlays>>,
    clicks: &Rc<Option<KeyClickPlayer>>,
) {
    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_select_profile(move |profile_id_str| {
            crate::trace!("callback select-profile(profile_id={profile_id_str:?})");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback select-profile: window 已销毁，忽略");
                return;
            };
            let Some(profile_id) = parse_profile_id(&profile_id_str) else {
                crate::trace!("callback select-profile: profile_id 解析失败，忽略");
                window.set_nickname_error("无法识别所选档案".into());
                return;
            };
            let commands = dispatch_traced(&controller, AppEvent::SelectProfile(profile_id));
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_create_profile(move |nickname| {
            crate::trace!("callback create-profile(nickname={nickname:?})");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback create-profile: window 已销毁，忽略");
                return;
            };
            let commands = dispatch_traced(
                &controller,
                AppEvent::CreateProfile {
                    nickname: nickname.to_string(),
                },
            );
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    // 档案管理三个回调（Req 6.8-6.14）。三者都已在 `ProfileSelectPage` 的
    // 二次确认弹窗中被用户确认过（点"取消"根本不会触发回调，Req 6.10），
    // 因此这里不再追问，直接派发对应事件。
    //
    // `profile-id` 解析失败时只写内联错误提示、不派发事件：与
    // `select-profile` 的处理一致——这属于"UI 传来的标识无法识别"，不是一次
    // 合法的档案操作，不应让状态机去处理一个无意义的 id。
    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_delete_profile(move |profile_id_str| {
            crate::trace!("callback delete-profile(profile_id={profile_id_str:?})");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback delete-profile: window 已销毁，忽略");
                return;
            };
            let Some(profile_id) = parse_profile_id(&profile_id_str) else {
                crate::trace!("callback delete-profile: profile_id 解析失败，忽略");
                window.set_nickname_error("无法识别所选档案".into());
                return;
            };
            let commands = dispatch_traced(&controller, AppEvent::DeleteProfile(profile_id));
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_reset_profile_progress(move |profile_id_str| {
            crate::trace!("callback reset-profile-progress(profile_id={profile_id_str:?})");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback reset-profile-progress: window 已销毁，忽略");
                return;
            };
            let Some(profile_id) = parse_profile_id(&profile_id_str) else {
                crate::trace!("callback reset-profile-progress: profile_id 解析失败，忽略");
                window.set_nickname_error("无法识别所选档案".into());
                return;
            };
            let commands = dispatch_traced(&controller, AppEvent::ResetProfileProgress(profile_id));
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_rename_profile(move |profile_id_str, nickname| {
            crate::trace!(
                "callback rename-profile(profile_id={profile_id_str:?}, nickname={nickname:?})"
            );
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback rename-profile: window 已销毁，忽略");
                return;
            };
            let Some(profile_id) = parse_profile_id(&profile_id_str) else {
                crate::trace!("callback rename-profile: profile_id 解析失败，忽略");
                window.set_nickname_error("无法识别所选档案".into());
                return;
            };
            let commands = dispatch_traced(
                &controller,
                AppEvent::RenameProfile {
                    profile_id,
                    nickname: nickname.to_string(),
                },
            );
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    // 课程序列界面左上角"返回"：回到档案选择界面。这条反向导航是档案管理
    // 功能（Req 6.8-6.14）在启动之后仍可达的唯一路径。
    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_exit_to_profile_select(move || {
            crate::trace!("callback exit-to-profile-select()");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback exit-to-profile-select: window 已销毁，忽略");
                return;
            };
            let commands = dispatch_traced(&controller, AppEvent::ExitToProfileSelect);
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_lesson_selected(move |index| {
            crate::trace!("callback lesson-selected(index={index})");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback lesson-selected: window 已销毁，忽略");
                return;
            };
            let Some(lesson_id) = lesson_id_at(&controller.borrow(), index) else {
                crate::trace!("callback lesson-selected: index={index} 越界，忽略");
                return;
            };
            crate::trace!("callback lesson-selected: index={index} -> lesson_id={lesson_id:?}");
            let commands = dispatch_traced(&controller, AppEvent::SelectLesson(lesson_id));
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_locked_lesson_tapped(move |index| {
            crate::trace!("callback locked-lesson-tapped(index={index})");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback locked-lesson-tapped: window 已销毁，忽略");
                return;
            };
            // 与 `lesson-selected` 一致：仍通过 `AppEvent::SelectLesson` 派发，
            // 由 `AppController::dispatch` 内部的解锁判定（Req 1.7）决定
            // 是否阻止进入并返回解锁条件提示——`curriculum_map.slint` 已经
            // 把"是否已解锁"的分支判断做在 `.slint` 侧（对 Unlocked 发
            // `lesson-selected`，对 Locked/Unavailable 发
            // `locked-lesson-tapped`），但 Rust 侧的判定入口是唯一的
            // `dispatch`，不因回调来源不同而绕过該判定逻辑。
            let Some(lesson_id) = lesson_id_at(&controller.borrow(), index) else {
                crate::trace!("callback locked-lesson-tapped: index={index} 越界，忽略");
                return;
            };
            crate::trace!(
                "callback locked-lesson-tapped: index={index} -> lesson_id={lesson_id:?}"
            );
            let commands = dispatch_traced(&controller, AppEvent::SelectLesson(lesson_id));
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    {
        let controller = controller.clone();
        let window_weak = window.as_weak();
        window.on_practice_poll_elapsed(move || {
            // 该回调由 `practice_view.slint` 的 `Timer` 每秒触发一次，是唯一
            // 会周期性产生 trace 的埋点；只在练习环节界面上活跃，不会在
            // 档案选择/课程序列界面刷屏。
            crate::trace!("callback practice-poll-elapsed()");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback practice-poll-elapsed: window 已销毁，忽略");
                return;
            };
            refresh_practice_elapsed(&window, &controller.borrow());
        });
    }

    // 物理键盘输入（Req 3.2, 3.3, 3.6）：`practice_view.slint` 的
    // `key-scope := FocusScope` 把 `KeyEvent.text` 原样上抛到这里，由
    // [`practice_input_char`] 过滤/转换后派发 `AppEvent::CharInput`。
    // 这是 `AppEvent::CharInput` 唯一的派发点——在本次接线之前没有任何
    // Slint 回调会产生该事件，孩子实际上无法打字。
    {
        let controller = controller.clone();
        let pending = pending.clone();
        let clicks = clicks.clone();
        let window_weak = window.as_weak();
        window.on_practice_char_input(move |text| {
            // 同时打印码位：中文输入法会把 `;` 送成全角 `；`（U+FF1B）这类
            // 肉眼难以区分的变体，只打印字符本身无法定位（见
            // `normalize_ime_punctuation`）。
            crate::trace!(
                "callback practice-char-input(text={text:?}, codepoints={:?}, normalized={:?})",
                text.chars().map(|c| format!("U+{:04X}", c as u32)).collect::<Vec<_>>(),
                practice_input_char(&text)
            );
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback practice-char-input: window 已销毁，忽略");
                return;
            };
            // 不是有效的练习输入字符（控制键/方向键/输入法组合中间态等）时
            // 静默忽略：不派发事件、不产生提示，避免把"按了一下 Esc"记成
            // 一次输入错误（Req 3.3 的错误统计只应统计真实的字符输入）。
            let Some(ch) = practice_input_char(&text) else {
                crate::trace!("callback practice-char-input: text={text:?} 非练习输入字符，忽略");
                return;
            };
            // 敲击反馈：先点亮被按下的键位（键帽下沉动画），再派发事件。
            //
            // 顺序刻意放在 dispatch 之前：这一步是纯视觉反馈，不应该等状态机
            // 算完（练习完成时 dispatch 会连带触发结果统计与奖励浮层）。松手
            // 回弹由 `virtual_keyboard.slint` 里的计时器负责，这里只写下标。
            //
            // 输入的字符可能不对应任何物理键位（未能还原的中文标点等），
            // 此时 `layout_index_of_char` 返回 `None`，不做动画——但事件仍会
            // 照常派发并被判为输入错误（Req 3.3）。大写字母/Shift 组合符号
            // 会定位到与其基础字符相同的物理键位（Shift 不改变物理键位），
            // 因此仍会正常触发按键动画。
            if let Some(index) = layout_index_of_char(ch) {
                window.set_practice_pressed_key_index(index as i32);
            }

            // 听觉反馈：在派发之前先按"这一下打对了没有"选好音效。
            //
            // 这里只是**只读地看一眼**当前待输入字符，不重新实现匹配逻辑——
            // 是否正确的权威判定仍然只发生在 `domain::apply_input` 里（下一行
            // 的 dispatch）。之所以要提前取：dispatch 之后 `cursor` 已经前移、
            // 甚至练习已经完成、`practice_state` 被清空，就取不到"刚才该按哪个
            // 字符"了。
            if let Some(player) = clicks.as_ref() {
                let expected = controller
                    .borrow()
                    .practice_state()
                    .and_then(|state| {
                        let typing = &state.typing_state;
                        typing.text.get(typing.cursor).copied()
                    });
                player.play(if expected == Some(ch) {
                    ClickSound::Correct
                } else {
                    ClickSound::Error
                });
            }

            let commands = dispatch_traced(&controller, AppEvent::CharInput(ch));
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_practice_exit_requested(move || {
            crate::trace!("callback practice-exit-requested()");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback practice-exit-requested: window 已销毁，忽略");
                return;
            };
            let commands = dispatch_traced(&controller, AppEvent::ExitPractice);
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }

    // 全局 Toast 浮层的自动消失：`Toast` 组件的 `Timer` 到时只通知"该收起
    // 了"，由这里把文案置回空字符串（组件自身不修改可见状态，见
    // `toast.slint` 的分层约定）。
    {
        let window_weak = window.as_weak();
        window.on_toast_dismissed(move || {
            crate::trace!("callback toast-dismissed()");
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            window.set_toast_message("".into());
        });
    }

    // 奖励反馈浮层的展示时长到时：关闭奖励浮层，并把暂存的结果统计接着
    // 展示出来（「先庆祝、后看成绩」的串联顺序，见 `PendingOverlays`）。
    {
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_reward_dismissed(move || {
            crate::trace!("callback reward-dismissed()");
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            window.set_reward_active(false);
            flush_pending_result(&window, &mut pending.borrow_mut());
        });
    }

    // 结果统计浮层的"继续"按钮：仅关闭浮层。下方页面早已由
    // `NavigateTo(CurriculumSequence)` 切换为课程序列界面，因此这里不需要
    // 再做任何导航（Req 4.1-4.5 只要求展示成绩，不定义额外的跳转）。
    {
        let window_weak = window.as_weak();
        window.on_result_dismissed(move || {
            crate::trace!("callback result-dismissed()");
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            window.set_result_active(false);
        });
    }

    // 「展示正确答案」浮层的"继续下一题"按钮（Req 7.6）：清空提示文案后
    // 派发 `AppEvent::SkipCurrentItem`。
    //
    // 这是 `AppEvent::SkipCurrentItem` 唯一的派发点——在本次接线之前，
    // 没有任何 Slint 回调会产生该事件。注意 `handle_skip_current_item`
    // 目前的业务语义只是"重置连续错误计数"（其文档注释说明了原因），
    // 桥接层不改变这一语义。
    {
        let controller = controller.clone();
        let pending = pending.clone();
        let window_weak = window.as_weak();
        window.on_answer_hint_continue(move || {
            crate::trace!("callback answer-hint-continue()");
            let Some(window) = window_weak.upgrade() else {
                crate::trace!("callback answer-hint-continue: window 已销毁，忽略");
                return;
            };
            window.set_answer_hint_message("".into());
            window.set_answer_hint_answer("".into());
            let commands = dispatch_traced(&controller, AppEvent::SkipCurrentItem);
            apply_commands(
                &window,
                &controller.borrow(),
                &mut pending.borrow_mut(),
                commands,
            );
        });
    }
}

/// 查找当前课程序列视图中索引 `index` 对应的课程 id；索引越界时返回
/// `None`（静默忽略，不 panic——`curriculum_map.slint` 的索引来自其自身
/// 渲染的 `lessons` 数组，正常情况下不会越界，但 Rust 侧仍需防御式处理）。
fn lesson_id_at(controller: &AppController, index: i32) -> Option<domain::LessonId> {
    if index < 0 {
        return None;
    }
    controller
        .curriculum()
        .lessons
        .get(index as usize)
        .map(|lesson| lesson.id.clone())
}

/// Unicode 私用区（Private Use Area）的起止字符。
///
/// Slint 把没有对应可打印字符的按键（Esc、Backspace、Delete、方向键、
/// F1..F12、Home/End 等）映射到这一区间内的哨兵字符——`Key.Escape`、
/// `Key.Backspace` 之类的常量本身就是这些私用区字符。因此
/// [`practice_input_char`] 必须整段排除它们，否则孩子按一下方向键就会被
/// 当成一次练习输入并计入错误统计。
const PRIVATE_USE_AREA_START: char = '\u{E000}';
const PRIVATE_USE_AREA_END: char = '\u{F8FF}';

/// 全角 ASCII 区（U+FF01 `！` .. U+FF5E `～`）与半角 ASCII 区（U+0021 `!` ..
/// U+007E `~`）之间的固定偏移。
const FULLWIDTH_TO_ASCII_OFFSET: u32 = 0xFEE0;

/// 纯函数：把中文输入法产出的全角/中文标点还原为它在美式键盘上对应的
/// **物理按键字符**。
///
/// 为什么需要这一步：macOS 的简体拼音输入法即使处于英文模式
/// （`com.apple.inputmethod.SCIM.ITABC`），字母会原样输出，但标点会按输入法
/// 自己的标点集转换——按下 `;` 实际送达应用的是全角分号 `；`（U+FF1B），
/// 按下 `[`/`]` 送达的可能是 `【`/`】`，按下 `\` 送达的是 `、`。这些字符
/// 不在 `key_to_char` 的值域里，于是 `domain::apply_input` 只能判为输入错误：
/// 孩子明明按对了键位，屏幕上却记一次错。
///
/// 本应用教的是**键位**（`FingerZone`/`KeyCode` 是它的核心领域概念），
/// "按下了哪个物理键"才是要判定的对象；输入法把该键的字符换成了全角变体，
/// 属于输入通道的表示差异，不是孩子按错了键。因此在桥接层把它还原回去，
/// 而不是要求家长先去系统设置里切输入法——后者对一个给孩子用的打字应用
/// 是不可接受的前提条件。
///
/// 转换规则：
/// 1. 全角 ASCII 区整段按固定偏移还原（覆盖 `；`->`;`、`，`->`,`、`：`->`:`
///    等绝大多数情况）。
/// 2. 全角空格 `U+3000` -> 半角空格。
/// 3. 不落在全角 ASCII 区的中文标点（`。`、`、`、书名号、各类引号/括号、
///    间隔号、破折号）用显式表映射到产生它的那个按键的字符。
/// 4. 其余字符原样返回——包括本来就是半角的字符，以及任何本函数不认识的
///    Unicode 字符（它们仍会被 `apply_input` 判为错误输入，与本次修改前
///    的行为一致）。
fn normalize_ime_punctuation(ch: char) -> char {
    // 规则 1：全角 ASCII 区。
    if ('\u{FF01}'..='\u{FF5E}').contains(&ch)
        && let Some(ascii) = char::from_u32(ch as u32 - FULLWIDTH_TO_ASCII_OFFSET)
    {
        return ascii;
    }

    // 规则 2、3。表中每一项的右侧都是"在美式键盘上按哪个键会得到左侧这个
    // 中文标点"，因此左右两侧共享同一个物理键位。
    match ch {
        '\u{3000}' => ' ', // 全角空格
        '。' => '.',
        '、' => '\\',
        '·' => '`',
        '—' => '-',
        '‘' | '’' => '\'',
        '“' | '”' => '"',
        '《' => '<',
        '》' => '>',
        '「' | '【' => '[',
        '」' | '】' => ']',
        '『' => '{',
        '』' => '}',
        other => other,
    }
}

/// 纯函数：把 Slint `KeyEvent.text` 转换为一个可用于练习输入的字符
/// （Req 3.2, 3.3, 3.6）；不属于练习输入的按键返回 `None`（调用方静默忽略）。
///
/// 过滤规则（按顺序）：
/// 1. 空字符串 -> `None`（某些按键事件不携带任何文本）。
/// 2. 字符数不等于 1 -> `None`。练习的最小单元是"一个字符"
///    （`domain::apply_input` 的入参是 `char`），多字符的 `text` 只可能来自
///    输入法组合输入等场景，没有对应的单字符语义。
/// 3. `char::is_control()` 为真 -> `None`。回车/退格/Tab 等键在部分平台上
///    以控制字符（`'\n'`、`'\u{8}'`、`'\t'`）而不是私用区字符的形式出现。
/// 4. 落在 Unicode 私用区 `U+E000..=U+F8FF` -> `None`
///    （见 [`PRIVATE_USE_AREA_START`]）。
/// 5. 经 [`normalize_ime_punctuation`] 把中文输入法产出的全角/中文标点还原
///    为对应物理按键的字符（如 `；` -> `;`）。
/// 6. 其余一律返回 `Some(c)`——**包括**那些明显不可能出现在练习文本里的
///    字符（大写字母、未能还原的中文标点等）。这类输入应该被
///    `domain::apply_input` 判定为错误输入并计入错误统计（Req 3.3），而不是
///    在桥接层被吞掉。
fn practice_input_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if ch.is_control() {
        return None;
    }
    if (PRIVATE_USE_AREA_START..=PRIVATE_USE_AREA_END).contains(&ch) {
        return None;
    }
    Some(normalize_ime_punctuation(ch))
}

/// 将 Slint 侧传入的档案 id 字符串解析为 `ProfileId`（内部为 UUID）。
fn parse_profile_id(raw: &str) -> Option<ProfileId> {
    uuid::Uuid::parse_str(raw).ok().map(ProfileId)
}

/// 将 `ProfileId` 格式化为 Slint 侧使用的字符串形式（UUID 标准文本表示）。
fn format_profile_id(id: ProfileId) -> String {
    id.0.to_string()
}

/// 依次应用 `dispatch` 返回的全部 `UiCommand`，并在应用完毕后统一刷新受
/// 影响页面的展示数据（避免每条命令各自触发一次刷新造成重复计算）。
///
/// 末尾还负责浮层展示顺序的收尾判定：若本批命令产生了待展示的结果统计、
/// 但并没有同时激活奖励反馈浮层，则立刻把结果统计展示出来（详见
/// [`PendingOverlays`]）。
fn apply_commands(
    window: &AppWindow,
    controller: &AppController,
    pending: &mut PendingOverlays,
    commands: Vec<UiCommand>,
) {
    for command in commands {
        apply_command(window, controller, pending, command);
    }

    if !window.get_reward_active() {
        flush_pending_result(window, pending);
    }

    refresh_all_pages(window, controller);
}

/// 把暂存的结果统计数据写入 Slint 属性并展示；没有待展示数据时什么都不做。
fn flush_pending_result(window: &AppWindow, pending: &mut PendingOverlays) {
    let Some(view) = pending.pending_result.take() else {
        return;
    };
    apply_result_summary_view(window, &view);
    window.set_result_active(true);
}

/// 将 [`ResultSummaryView`] 写入 `AppWindow` 的 `result-*` 属性（不含
/// `result-active`——是否展示由调用方决定）。
fn apply_result_summary_view(window: &AppWindow, view: &ResultSummaryView) {
    window.set_result_duration_ms(view.duration_ms);
    window.set_result_error_count(view.error_count);
    window.set_result_accuracy(view.accuracy);
    window.set_result_wpm(view.wpm);
    window.set_result_comparison_direction(view.comparison_direction);
    window.set_result_accuracy_delta(view.accuracy_delta);
    window.set_result_wpm_delta(view.wpm_delta);
}

/// 将 [`RewardOverlayView`] 写入 `AppWindow` 的 `reward-*` 属性（不含
/// `reward-active`——是否展示由调用方决定）。
fn apply_reward_overlay_view(window: &AppWindow, view: &RewardOverlayView) {
    window.set_reward_show_animation(view.show_animation);
    window.set_reward_show_badge(view.show_badge);
    window.set_reward_encouragement_text(view.encouragement_text.clone().into());
    window.set_reward_display_duration_ms(view.display_duration_ms);
}

/// `ShowToast` 的两个可能展示位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToastTarget {
    /// 档案选择界面昵称输入框旁的内联错误提示（`nickname-error`）。
    NicknameError,
    /// 全局 Toast 浮层（`toast-message`）。
    GlobalToast,
}

/// 纯函数：依据当前页面决定 `UiCommand::ShowToast` 应写入哪个展示位。
///
/// # 为什么需要按页面路由
///
/// `AppController` 对「昵称校验错误」（Req 6.5, 6.7：昵称为空/超长/重复/
/// 档案数已达上限）与「一般提示」（Req 1.7 未解锁课程、Req 5.5 成绩保存
/// 失败等）返回的是同一个 `UiCommand::ShowToast(String)` 变体，桥接层无法
/// 从命令类型上区分二者。
///
/// 但 Req 6.5/6.7 明确要求昵称非法时"保留用户当前输入内容，并显示错误提示
/// 指明昵称不能为空/该昵称已被使用"——这类错误必须**内联展示在输入框旁**
/// 才能让用户对着自己的输入修改；用几秒后自动消失的全局 Toast 承载它并不
/// 满足需求意图。
///
/// # 采用的策略
///
/// 用「当前页面」作为区分依据：档案选择界面（`profile-select`）上出现的
/// `ShowToast` 一律视为昵称校验/档案选择相关的错误，写入 `nickname-error`；
/// 其他页面写入全局 Toast 浮层。
///
/// 这个映射在当前的 `AppController` 实现下是精确的：会在
/// `profile-select` 页面产生 `ShowToast` 的只有 `handle_select_profile`
/// 与 `handle_create_profile` 两条路径（其余 `ShowToast` 都发生在已经选定
/// 档案、页面已经切换之后），二者都属于"档案相关错误、应内联展示且需保留
/// 用户输入"的类别。
///
/// # 为什么不给 `UiCommand` 增加更精确的变体
///
/// 更"正确"的方案是新增 `UiCommand::ShowNicknameError(String)`，让类型自身
/// 携带这一区分。这里没有采用，理由是成本收益不划算：那会改动
/// `app_controller.rs` 的公开枚举，并需要同步修改其既有测试中对
/// `UiCommand::ShowToast(_)` 的多处断言（`dispatch_select_profile_*`/
/// `dispatch_create_profile_*` 等），而收益仅是消除一个在当前实现下已经
/// 精确的映射。若未来 `AppController` 出现"在档案选择界面上展示的非昵称类
/// 一般提示"，则应改为新增专用变体，而不是继续依赖页面判断。
fn toast_target_for_page(page: AppPage) -> ToastTarget {
    match page {
        AppPage::ProfileSelect => ToastTarget::NicknameError,
        AppPage::CurriculumMap | AppPage::PracticeView => ToastTarget::GlobalToast,
    }
}

/// 将单条 `UiCommand` 应用到 `AppWindow` 属性/导航状态。
fn apply_command(
    window: &AppWindow,
    controller: &AppController,
    pending: &mut PendingOverlays,
    command: UiCommand,
) {
    match command {
        UiCommand::NavigateTo(target) => {
            let page = navigation_target_to_app_page(target);
            // 打印写入前后的 `current-page`，确认属性真的被改了（而不是
            // 命令到了、导航却没生效）。
            crate::trace!(
                "apply_command NavigateTo({target:?}) -> AppPage::{page:?}；写入前 current-page={:?}",
                window.get_current_page()
            );
            window.set_current_page(page);
            crate::trace!(
                "apply_command NavigateTo: 写入后 current-page={:?}",
                window.get_current_page()
            );
            // 离开档案选择界面后，昵称输入框旁的内联错误提示不应残留到
            // 下一次回到该界面时（Req 6.5/6.7 只要求"创建失败时展示错误"，
            // 不要求错误提示跨会话保留）。
            window.set_nickname_error("".into());
        }
        UiCommand::ShowToast(message) => match toast_target_for_page(window.get_current_page()) {
            ToastTarget::NicknameError => {
                crate::trace!(
                    "apply_command ShowToast -> nickname-error（当前页 {:?}）：{message:?}",
                    window.get_current_page()
                );
                window.set_nickname_error(message.into())
            }
            ToastTarget::GlobalToast => {
                crate::trace!(
                    "apply_command ShowToast -> 全局 Toast（当前页 {:?}）：{message:?}",
                    window.get_current_page()
                );
                window.set_toast_message(message.into())
            }
        },
        UiCommand::ShowGlobalToast(message) => {
            // 与 `ShowToast` 不同：不经过 `toast_target_for_page` 的分页路由，
            // 一律落到全局 Toast 浮层。档案管理的结果反馈（含成功提示）不属于
            // 昵称输入框的内联错误，理由见 `UiCommand::ShowGlobalToast` 的文档。
            crate::trace!("apply_command ShowGlobalToast：{message:?}");
            window.set_toast_message(message.into());
        }
        UiCommand::ShowEncouragement(message) => {
            // Req 7.2：连续错误 3 次时的鼓励提示。用全局 Toast 浮层承载
            // （短暂展示后自动消失，不打断孩子继续尝试当前题目）。
            crate::trace!("apply_command ShowEncouragement -> 全局 Toast：{message:?}");
            window.set_toast_message(message.into());
        }
        UiCommand::ShowCorrectAnswer(message) => {
            // Req 7.6：连续错误 5 次时展示正确答案，并允许学员选择继续
            // 下一题目。`AnswerHint` 浮层同时展示提示文案与"正确答案"
            // （当前待输入字符），其"继续下一题"按钮派发
            // `AppEvent::SkipCurrentItem`。
            let answer = expected_char_display(controller);
            crate::trace!(
                "apply_command ShowCorrectAnswer -> AnswerHint：message={message:?} answer={answer:?}"
            );
            window.set_answer_hint_answer(answer.into());
            window.set_answer_hint_message(message.into());
        }
        UiCommand::ShowRewardFeedback(feedback) => {
            // Req 7.3：课程完成时的奖励反馈。立即展示；其 `Timer` 到时会
            // 通过 `reward-dismissed()` 回调关闭本浮层并接着展示暂存的
            // 结果统计（见 `PendingOverlays`）。
            let view = build_reward_overlay_view(&feedback);
            crate::trace!("apply_command ShowRewardFeedback -> reward-active=true，view={view:?}");
            apply_reward_overlay_view(window, &view);
            window.set_reward_active(true);
        }
        UiCommand::ShowPracticeResult(result) => {
            // Req 4.1, 4.2, 4.4, 4.5：结果统计。这里只计算并暂存，不立即
            // 展示——展示时机由 `PendingOverlays` 描述的串联顺序决定。
            let previous_best = previous_best_for_result(controller, &result);
            let view = build_result_summary_view(&result, previous_best.as_ref());
            crate::trace!("apply_command ShowPracticeResult -> 暂存待展示，view={view:?}");
            pending.pending_result = Some(view);
        }
    }
}

/// 当前待输入字符（即 Req 7.6 所说的"正确答案"）的展示文本。
///
/// 无进行中的练习环节、或练习文本已全部输入完毕（没有"下一个待输入字符"）
/// 时返回空字符串，`AnswerHint` 会据此不渲染答案行。
fn expected_char_display(controller: &AppController) -> String {
    let Some(practice_state) = controller.practice_state() else {
        return String::new();
    };
    expected_char_of(&practice_state.typing_state)
}

/// 纯函数版本的 [`expected_char_display`]：给定字符匹配状态，返回当前待输入
/// 字符的展示文本。空格用可见的"空格"二字表示（直接渲染 `' '` 孩子看不出
/// 来"正确答案"是什么）。
fn expected_char_of(typing_state: &domain::TypingMatchState) -> String {
    match typing_state.text.get(typing_state.cursor) {
        Some(&' ') => "空格".to_string(),
        Some(ch) => ch.to_string(),
        None => String::new(),
    }
}

/// 取出当前档案在该课程上的「本次之前的历史最佳成绩」，供
/// `compare_with_best` 使用（Req 4.4, 4.5）。
///
/// 未选择档案 / 该课程尚无任何记录时返回 `None`（对应
/// `ComparisonResult::FirstRecord`，Req 4.5）。
fn previous_best_for_result(
    controller: &AppController,
    current: &storage::PracticeResult,
) -> Option<storage::PracticeResult> {
    let record = controller
        .current_profile()?
        .progress
        .lesson_records
        .get(&current.lesson_id)?;
    previous_best_excluding_current(record, current)
}

/// 纯函数：从课程记录的 `history` 中剔除本次成绩后，取出历史最佳成绩。
///
/// # 为什么必须剔除本次成绩
///
/// `AppController::handle_practice_completion()` 的时序是：
/// 1. 先调用 `CurriculumState::record_practice_result`，其内部的
///    `upsert_lesson_record` 会把本次成绩 **push 进 `history` 末尾**，并按
///    "正确率优先、相同时比较 WPM"的规则把 `best_accuracy`/`best_wpm`/
///    `best_score` 更新为**含本次在内**的历史最优；
/// 2. 之后才把 `ShowPracticeResult(result)` 命令返回给桥接层。
///
/// 也就是说，桥接层拿到命令时，`LessonRecord::best_*` 已经把本次成绩算了
/// 进去。若直接用 `best_*` 比较，破纪录的那一次会被判成 `Equal`（自己和
/// 自己比），与 Req 4.4"明确指出本次成绩相较历史最佳是更高、更低还是相等"
/// 的意图不符。
///
/// # 采用的取法
///
/// `history` 是一个按时间追加的完整列表（`upsert_lesson_record` 只 push，
/// 从不删改），本次成绩恒是最后被 push 进去的那一条。因此这里从后向前找到
/// **第一条与 `current` 完全相等**的记录并剔除它，再在剩余记录上按同样的
/// "正确率优先、相同时比较 WPM"规则取最佳。这样得到的是精确的"本次之前的
/// 历史最佳"，不是近似值。
///
/// 用"值相等 + rposition"而不是无条件 `history.pop()`，是为了对
/// "`ShowPracticeResult` 由其他未经 `record_practice_result` 的路径产生"
/// 这一情况保持正确：此时 `history` 里不含 `current`，不会误删一条真实的
/// 历史记录。
///
/// 剩余记录为空（本次是该课程的首次练习）时返回 `None`，对应
/// `ComparisonResult::FirstRecord`（Req 4.5）。
fn previous_best_excluding_current(
    record: &storage::LessonRecord,
    current: &storage::PracticeResult,
) -> Option<storage::PracticeResult> {
    let mut history: Vec<&storage::PracticeResult> = record.history.iter().collect();
    if let Some(pos) = history.iter().rposition(|entry| *entry == current) {
        history.remove(pos);
    }
    best_of(&history).cloned()
}

/// 纯函数：按"正确率优先，正确率相同时比较打字速度"的规则取出最佳成绩。
///
/// 该规则与 `CurriculumState::upsert_lesson_record` 更新 `best_*` 字段时
/// 使用的规则完全一致，保证桥接层重算出的"历史最佳"与持久化层缓存的
/// `best_*` 语义相同（差别仅在于是否包含本次成绩）。
fn best_of<'a>(history: &[&'a storage::PracticeResult]) -> Option<&'a storage::PracticeResult> {
    history.iter().copied().reduce(|best, candidate| {
        let is_better = candidate.accuracy > best.accuracy
            || (candidate.accuracy == best.accuracy && candidate.wpm > best.wpm);
        if is_better { candidate } else { best }
    })
}

/// 纯函数：把本次成绩与（本次之前的）历史最佳成绩转换为结果统计浮层所需的
/// 全部展示数据（Req 4.1, 4.2, 4.4, 4.5）。
///
/// 比较判定本身完全委托给领域层的 `domain::compare_with_best`，本函数只做
/// `ComparisonResult -> (ComparisonDirection, accuracy_delta, wpm_delta)`
/// 的形状转换与数值类型转换（`u64`/`u32` -> Slint 的 `i32`）。
fn build_result_summary_view(
    current: &storage::PracticeResult,
    previous_best: Option<&storage::PracticeResult>,
) -> ResultSummaryView {
    let comparison = compare_with_best(
        &to_domain_practice_result(current),
        previous_best.map(to_domain_practice_result).as_ref(),
    );
    let (comparison_direction, accuracy_delta, wpm_delta) =
        comparison_result_to_slint(comparison);

    ResultSummaryView {
        // Slint 的整数属性是 `i32`：用 `try_into` 而非 `as` 转换，超出范围
        // 时退化为 `i32::MAX`（练习用时/错误数不可能真的溢出 i32，这里只是
        // 避免 `as` 的静默回绕产生负数展示值）。
        duration_ms: current.duration_ms.try_into().unwrap_or(i32::MAX),
        error_count: current.error_count.try_into().unwrap_or(i32::MAX),
        accuracy: current.accuracy,
        wpm: current.wpm,
        comparison_direction,
        accuracy_delta,
        wpm_delta,
    }
}

/// 纯函数：`ComparisonResult` -> Slint `ComparisonDirection` + 两个差值。
///
/// `FirstRecord`/`Equal` 分支不携带差值（前者没有可比对象，后者两个差值恒
/// 为 0），统一返回 `0.0`——`result_summary.slint` 在 `first-record` 分支
/// 下不渲染差值行，在 `equal` 分支下渲染的 `+0.0%`/`+0.0 WPM` 与语义一致。
fn comparison_result_to_slint(result: ComparisonResult) -> (ComparisonDirection, f32, f32) {
    match result {
        ComparisonResult::FirstRecord => (ComparisonDirection::FirstRecord, 0.0, 0.0),
        ComparisonResult::Higher {
            accuracy_delta,
            wpm_delta,
        } => (ComparisonDirection::Higher, accuracy_delta, wpm_delta),
        ComparisonResult::Lower {
            accuracy_delta,
            wpm_delta,
        } => (ComparisonDirection::Lower, accuracy_delta, wpm_delta),
        ComparisonResult::Equal => (ComparisonDirection::Equal, 0.0, 0.0),
    }
}

/// 纯函数：`domain::RewardFeedback` -> 奖励反馈浮层展示数据（Req 7.3）。
///
/// - `encouragement_text: None` 写入空字符串（`RewardOverlay` 据此不渲染
///   鼓励文案行）。
/// - `display_duration_ms` 直接透传（`RewardOverlay` 内部另有 2000ms 的
///   下限兜底，与 `MIN_REWARD_DISPLAY_DURATION_MS` 一致）。
fn build_reward_overlay_view(feedback: &domain::RewardFeedback) -> RewardOverlayView {
    RewardOverlayView {
        show_animation: feedback.animation,
        show_badge: feedback.badge,
        encouragement_text: feedback.encouragement_text.unwrap_or("").to_string(),
        display_duration_ms: feedback
            .display_duration_ms
            .try_into()
            .unwrap_or(i32::MAX),
    }
}

/// 将持久化层的 `storage::PracticeResult` 转换为领域层的
/// `domain::PracticeResult`（`compare_with_best` 的入参类型）。
///
/// 与 `curriculum_state.rs` 中同名的私有转换函数逻辑一致——两层同构但类型
/// 不同，跨模块无法复用其私有实现，因此在桥接层重复一份最小转换。
fn to_domain_practice_result(result: &storage::PracticeResult) -> domain::PracticeResult {
    domain::PracticeResult {
        lesson_id: domain::LessonId(result.lesson_id.0.clone()),
        accuracy: result.accuracy,
        wpm: result.wpm,
        error_count: result.error_count,
        duration_ms: result.duration_ms,
        score: result.score,
    }
}

fn navigation_target_to_app_page(target: NavigationTarget) -> AppPage {
    match target {
        NavigationTarget::CurriculumSequence => AppPage::CurriculumMap,
        NavigationTarget::PracticeSession => AppPage::PracticeView,
        NavigationTarget::ProfileSelect => AppPage::ProfileSelect,
    }
}

/// 刷新全部三个页面的展示数据（档案列表、课程卡片、练习视图）。
///
/// 每个页面各自的刷新函数都能安全处理"当前不存在对应状态"的情况
/// （如未选择档案时课程卡片列表为空、无练习环节时练习视图为空），因此
/// 在每次 `dispatch` 之后统一调用全部三个刷新函数是安全的，不会因为
/// 当前处于某个无关页面而产生错误的属性写入。
fn refresh_all_pages(window: &AppWindow, controller: &AppController) {
    refresh_profiles(window, controller);
    refresh_lessons(window, controller);
    refresh_practice_view(window, controller);
}

/// 刷新档案选择界面的 `profiles` 属性（Req 6.2, 6.4）。
fn refresh_profiles(window: &AppWindow, controller: &AppController) {
    let profiles: Vec<ProfileSummary> = controller
        .profile_manager()
        .list_profiles()
        .iter()
        .map(|summary| ProfileSummary {
            profile_id: format_profile_id(summary.profile_id).into(),
            nickname: summary.nickname.clone().into(),
        })
        .collect();

    let count = profiles.len();
    window.set_profiles(ModelRc::new(VecModel::from(profiles)));
    crate::trace!(
        "refresh_profiles: 构造 {count} 个 ProfileSummary，写入后 window.get_profiles().row_count()={}",
        window.get_profiles().row_count()
    );
}

/// 刷新课程序列界面的 `lessons` 属性（Req 1.2, 1.6, 1.7）。
///
/// 未选择任何学员档案（`curriculum_state()` 为 `None`）时，写入空数组
/// ——课程序列界面在这种状态下本就不应展示任何课程卡片。
fn refresh_lessons(window: &AppWindow, controller: &AppController) {
    let Some(curriculum_state) = controller.curriculum_state() else {
        crate::trace!("refresh_lessons: curriculum_state()=None（未选择档案），写入空数组");
        window.set_lessons(ModelRc::new(VecModel::from(Vec::<LessonCardData>::new())));
        crate::trace!(
            "refresh_lessons: 写入后 window.get_lessons().row_count()={}",
            window.get_lessons().row_count()
        );
        return;
    };

    let cards = build_lesson_cards(controller.curriculum(), curriculum_state.lesson_states());
    crate::trace!(
        "refresh_lessons: curriculum_state()=Some，build_lesson_cards 产出 {} 张卡片",
        cards.len()
    );
    window.set_lessons(ModelRc::new(VecModel::from(cards)));
    crate::trace!(
        "refresh_lessons: 写入后 window.get_lessons().row_count()={}",
        window.get_lessons().row_count()
    );
}

/// 纯函数：将课程序列与其对应的 `DomainLessonState` 视图转换为 Slint 侧
/// 的 `LessonCardData` 数组。
///
/// - `title`：直接取课程数据中的人类可读标题 `Lesson::title`（面向儿童的
///   简体中文短标题，如"左手基准键 ASDF"），不再回退到 kebab-case 的课程
///   id——`id` 是内部标识，不适合直接呈现给孩子。
/// - `subtitle`：按 `LessonGoal` 给出简短的学习目标描述。
/// - `state`：`Locked -> locked`、`Unlocked -> unlocked`、
///   `Unavailable(_) -> unavailable`。
/// - `unlock-hint`：`Unlocked` 时为空字符串（不展示提示区域，与
///   `curriculum_map.slint` 的渲染约定一致）；`Locked` 时给出"完成上一课程
///   后解锁"提示；`Unavailable(reason)` 时原样携带该原因文案。
fn build_lesson_cards(curriculum: &Curriculum, states: &[DomainLessonState]) -> Vec<LessonCardData> {
    curriculum
        .lessons
        .iter()
        .zip(states.iter())
        .map(|(lesson, state)| LessonCardData {
            title: lesson.title.clone().into(),
            subtitle: lesson_goal_subtitle(lesson.goal).into(),
            state: lesson_state_to_slint(state),
            unlock_hint: lesson_unlock_hint(state).into(),
        })
        .collect()
}

fn lesson_goal_subtitle(goal: LessonGoal) -> &'static str {
    match goal {
        LessonGoal::SingleKey => "单键练习",
        LessonGoal::Word => "词语练习",
        LessonGoal::Sentence => "句子练习",
    }
}

fn lesson_state_to_slint(state: &DomainLessonState) -> LessonState {
    match state {
        DomainLessonState::Locked => LessonState::Locked,
        DomainLessonState::Unlocked => LessonState::Unlocked,
        DomainLessonState::Unavailable(_) => LessonState::Unavailable,
    }
}

fn lesson_unlock_hint(state: &DomainLessonState) -> String {
    match state {
        DomainLessonState::Unlocked => String::new(),
        DomainLessonState::Locked => "完成上一课程后解锁".to_string(),
        DomainLessonState::Unavailable(reason) => reason.clone(),
    }
}

/// 刷新练习环节界面的 `practice-chars`/`practice-key-states` 属性
/// （Req 3.4, 2.1-2.3），以及 `practice-elapsed-display`（Req 3.7）。
///
/// 无进行中的练习环节（`practice_state()` 为 `None`）时，写入空数组/初始
/// 时长文本——与课程序列界面在无档案时写空数组的策略一致。
fn refresh_practice_view(window: &AppWindow, controller: &AppController) {
    let Some(practice_state) = controller.practice_state() else {
        crate::trace!("refresh_practice_view: practice_state()=None，写入空数组与初始时长");
        window.set_practice_chars(ModelRc::new(VecModel::from(Vec::<PracticeCharView>::new())));
        window.set_practice_cursor(0);
        window.set_practice_key_states(ModelRc::new(VecModel::from(Vec::<KeyState>::new())));
        window.set_practice_hint_needs_shift(false);
        window.set_practice_elapsed_display("00:00".into());
        return;
    };

    let chars = build_practice_char_views(&practice_state.typing_state);
    window.set_practice_chars(ModelRc::new(VecModel::from(chars)));

    // 折行后用于把当前待输入位置所在行滚入可见区域（纯视觉，见
    // `practice_view.slint` 的 `cursor` 属性说明）。`cursor` 在练习完成时
    // 等于文本长度，Slint 侧已按 `chars.length - 1` 做了钳制。
    window.set_practice_cursor(practice_state.typing_state.cursor as i32);

    let key_states = build_key_states(&practice_state.typing_state);
    window.set_practice_key_states(ModelRc::new(VecModel::from(key_states)));
    window.set_practice_hint_needs_shift(hint_char_needs_shift(&practice_state.typing_state));

    window.set_practice_elapsed_display(format_elapsed(practice_state.elapsed()).into());

    crate::trace!(
        "refresh_practice_view: 写入后 practice-chars.row_count()={} practice-key-states.row_count()={} elapsed={:?}",
        window.get_practice_chars().row_count(),
        window.get_practice_key_states().row_count(),
        window.get_practice_elapsed_display()
    );
}

/// 仅刷新已用时长显示（`practice-poll-elapsed` 回调的最小职责，避免每秒
/// 一次的计时器触发都重新计算整段练习文本的字符视图/按键状态数组）。
fn refresh_practice_elapsed(window: &AppWindow, controller: &AppController) {
    let Some(practice_state) = controller.practice_state() else {
        window.set_practice_elapsed_display("00:00".into());
        return;
    };
    window.set_practice_elapsed_display(format_elapsed(practice_state.elapsed()).into());
}

/// 纯函数：将 `TypingMatchState` 展开为 Slint 侧的 `PracticeCharView` 数组，
/// 每个字符位置调用 `char_state_at` 计算其显示状态（Req 3.4）。
fn build_practice_char_views(
    typing_state: &domain::TypingMatchState,
) -> Vec<PracticeCharView> {
    typing_state
        .text
        .iter()
        .enumerate()
        .map(|(index, ch)| PracticeCharView {
            ch: ch.to_string().into(),
            state: char_state_to_slint(char_state_at(typing_state, index)),
        })
        .collect()
}

fn char_state_to_slint(state: CharState) -> PracticeCharState {
    match state {
        CharState::Pending => PracticeCharState::Pending,
        CharState::Correct => PracticeCharState::Correct,
        CharState::Error => PracticeCharState::Error,
    }
}

/// 标准 QWERTY 键盘按键的物理布局顺序（与 `virtual_keyboard.slint` 中
/// `default-key-layout()` 的 48 个键位顺序一一对应：数字行 13 键、QWERTY
/// 行 13 键、ASDF 行 11 键、ZXCV 行 10 键、空格 1 键）。桥接层需要按同样的
/// 顺序构造 `KeyState` 数组，否则 `VirtualKeyboard` 会把状态错配到错误的
/// 按键上。
const KEY_LAYOUT_ORDER: &[KeyCode] = &[
    // 数字行
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
    // QWERTY 行
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
    // ASDF 行
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
    // ZXCV 行
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
    // 空格
    KeyCode::Space,
];

/// 纯函数：把一个练习输入字符映射为它在 [`KEY_LAYOUT_ORDER`] 中的下标，供
/// 虚拟键盘定位"当前被按下的键帽"（敲击动画）。
///
/// 返回 `None` 的情况：该字符不对应任何标准键位（大写字母、未能还原的中文
/// 标点等）。调用方据此跳过动画，但**不应**据此吞掉输入事件——那类输入仍要
/// 交给状态机判为错误输入（Req 3.3）。
///
/// 下标口径与 `build_key_states` 完全一致（同一个 `KEY_LAYOUT_ORDER`），因此
/// `.slint` 侧用同一个下标既能取到 `KeyState` 也能定位按下的那个键。
fn layout_index_of_char(ch: char) -> Option<usize> {
    let key = char_to_key(ch)?;
    KEY_LAYOUT_ORDER.iter().position(|&candidate| candidate == key)
}

/// 纯函数：给定当前练习环节的字符匹配状态，按 `KEY_LAYOUT_ORDER` 构造完整
/// 的 48 个 `KeyState`：
/// - `zone`：始终按 `finger_zone_of` 映射（Req 2.1，与课程/提示无关，任何
///   按键在任何时刻都属于唯一的手指分区）。
/// - `is-active`：该按键是否属于当前练习目标涉及的键位集合。由于
///   `PracticeState` 本身未直接暴露"当前课程 target_keys"，这里改用
///   "练习文本中实际出现过的字符对应的键位"集合作为 `is-active` 的判定
///   依据——对单键练习（`LessonGoal::SingleKey`）而言，练习文本本身就是仅
///   由 `target_keys` 采样而来，因此该集合与 `target_keys` 集合一致；对
///   词语/句子练习而言，这一集合是"本次练习文本实际用到的键位"，同样是
///   对"课程涉及键位"的合理近似（Req 2.2 的核心要求是"课程涉及的按键"）。
/// - `is-hint`：仅 `current_key_hint` 返回的唯一键位为 `true`，其余全部为
///   `false`，从数据层保证"同一时刻仅一个提示"（Req 2.3，Property 5）。
/// - `needs-shift`：仅提示键位这一项可能为 `true`——当 `cursor` 指向的字符
///   本身需要按住 Shift 才能输入（大写字母、Shift 组合符号，通过
///   `domain::needs_shift` 判定）时置位，其余按键恒为 `false`。
/// - `shift-label`：该键位的 Shift 组合字符（如 `;` 键的 `":"`），与真实
///   键盘键帽的"上档/下档"印刷方式一致。由 [`shift_label_for`] 计算，
///   **恒定显示、不随课程或提示状态变化**——键帽上印着什么字符是键盘的
///   物理属性，不是"当前该练哪个键"的动态提示（那是 `is-active`/`is-hint`
///   的职责）。
fn build_key_states(typing_state: &domain::TypingMatchState) -> Vec<KeyState> {
    let active_keys: std::collections::HashSet<KeyCode> =
        typing_state.text.iter().filter_map(|&c| char_to_key(c)).collect();
    let hint_key = current_key_hint(typing_state);
    let hint_needs_shift = hint_char_needs_shift(typing_state);

    KEY_LAYOUT_ORDER
        .iter()
        .map(|&key| {
            let is_hint = hint_key == Some(key);
            KeyState {
                label: key_to_char(key).to_ascii_uppercase_label(key),
                zone: finger_zone_to_slint(finger_zone_of(key)),
                is_active: active_keys.contains(&key),
                is_hint,
                needs_shift: is_hint && hint_needs_shift,
                shift_label: shift_label_for(key),
            }
        })
        .collect()
}

/// 纯函数：给定一个物理键位，返回其键帽上应恒定显示的 Shift 组合字符标签。
///
/// - 字母键：返回空字符串——`label` 本身已经是大写字母（如 `"A"`），键帽上
///   不需要再重复印一遍"按 Shift 后是大写"这件事，与真实键盘的印刷惯例
///   一致（字母键帽只印一个大写字母，不会额外标注）。
/// - `Space`：返回空字符串（没有 Shift 变体）。
/// - 其余键位（数字行、`-=[]\;',./`）：返回 [`domain::shifted_char`] 对应的
///   字符（如 `";"` 键返回 `":"`），与你贴的 Apple Magic Keyboard 87 键
///   布局逐键对应。
fn shift_label_for(key: KeyCode) -> slint::SharedString {
    if key_to_char(key).is_ascii_alphabetic() {
        return "".into();
    }
    domain::shifted_char(key)
        .map(|c| c.to_string())
        .unwrap_or_default()
        .into()
}

/// 纯函数：给定当前练习环节的字符匹配状态，判定"当前提示字符是否需要按住
/// Shift"——即 `cursor` 指向的字符经 `domain::needs_shift` 判定的结果。
///
/// 状态已完成（无提示字符）时返回 `false`，与 `current_key_hint` 在该情形下
/// 返回 `None` 的语义一致（不存在提示，因此也不存在"是否需要 Shift"）。
fn hint_char_needs_shift(typing_state: &domain::TypingMatchState) -> bool {
    typing_state
        .text
        .get(typing_state.cursor)
        .is_some_and(|&c| domain::needs_shift(c))
}

fn finger_zone_to_slint(zone: domain::FingerZone) -> FingerZoneId {
    match zone {
        domain::FingerZone::LeftPinky => FingerZoneId::LeftPinky,
        domain::FingerZone::LeftRing => FingerZoneId::LeftRing,
        domain::FingerZone::LeftMiddle => FingerZoneId::LeftMiddle,
        domain::FingerZone::LeftIndex => FingerZoneId::LeftIndex,
        domain::FingerZone::LeftThumb => FingerZoneId::LeftThumb,
        domain::FingerZone::RightThumb => FingerZoneId::RightThumb,
        domain::FingerZone::RightIndex => FingerZoneId::RightIndex,
        domain::FingerZone::RightMiddle => FingerZoneId::RightMiddle,
        domain::FingerZone::RightRing => FingerZoneId::RightRing,
        domain::FingerZone::RightPinky => FingerZoneId::RightPinky,
    }
}

/// 将已用时长格式化为 `mm:ss` 显示文本（Req 3.7）。
fn format_elapsed(elapsed: std::time::Duration) -> String {
    let total_secs = elapsed.as_secs();
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}

/// 帮助 trait：把 `key_to_char` 返回的小写字符转换为虚拟键盘按键标签
/// （`virtual_keyboard.slint` 的 `default-key-layout()` 中字母键标签均为
/// 大写，数字/符号键标签与字符本身一致，空格键标签为 `"Space"`）。
trait KeyLabel {
    fn to_ascii_uppercase_label(&self, key: KeyCode) -> slint::SharedString;
}

impl KeyLabel for char {
    fn to_ascii_uppercase_label(&self, key: KeyCode) -> slint::SharedString {
        if key == KeyCode::Space {
            return "Space".into();
        }
        self.to_ascii_uppercase().to_string().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Lesson, TypingMatchState, UnlockCriteria, apply_input};

    #[test]
    fn create_main_window_fn_has_expected_signature() {
        // Slint 的窗口事件循环在 macOS 上要求必须在主线程创建（`winit` 约束），
        // 而 `cargo test` 默认在非主线程的测试 worker 上运行，因此这里不实际调用
        // `create_main_window()` 去实例化窗口，仅通过类型系统验证该函数签名
        // 与生成的 `AppWindow` 类型可以对齐、编译通过。
        let _: fn() -> Result<AppWindow, slint::PlatformError> = create_main_window;
    }

    #[test]
    fn run_app_fn_has_expected_signature() {
        // 与上一个测试同理：`run_app()` 内部会调用 `AppWindow::new()` 与
        // `window.run()`，两者均要求主线程，因此这里同样只做类型级签名校验，
        // 不实际调用。
        let _: fn() -> Result<(), Box<dyn std::error::Error>> = run_app;
    }

    // ---- parse_profile_id / format_profile_id ----

    #[test]
    fn parse_profile_id_roundtrips_with_format_profile_id() {
        let id = ProfileId::new();
        let formatted = format_profile_id(id);
        let parsed = parse_profile_id(&formatted).expect("valid uuid string should parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn parse_profile_id_rejects_invalid_string() {
        assert!(parse_profile_id("not-a-uuid").is_none());
        assert!(parse_profile_id("").is_none());
    }

    // ---- navigation_target_to_app_page ----

    #[test]
    fn navigation_target_maps_to_expected_app_page() {
        assert_eq!(
            navigation_target_to_app_page(NavigationTarget::CurriculumSequence),
            AppPage::CurriculumMap
        );
        assert_eq!(
            navigation_target_to_app_page(NavigationTarget::PracticeSession),
            AppPage::PracticeView
        );
        assert_eq!(
            navigation_target_to_app_page(NavigationTarget::ProfileSelect),
            AppPage::ProfileSelect
        );
    }

    // ---- lesson_state_to_slint / lesson_unlock_hint ----

    #[test]
    fn lesson_state_mapping_covers_all_variants_distinctly() {
        assert_eq!(
            lesson_state_to_slint(&DomainLessonState::Locked),
            LessonState::Locked
        );
        assert_eq!(
            lesson_state_to_slint(&DomainLessonState::Unlocked),
            LessonState::Unlocked
        );
        assert_eq!(
            lesson_state_to_slint(&DomainLessonState::Unavailable("x".to_string())),
            LessonState::Unavailable
        );
    }

    #[test]
    fn unlock_hint_is_empty_for_unlocked_state() {
        assert_eq!(lesson_unlock_hint(&DomainLessonState::Unlocked), "");
    }

    #[test]
    fn unlock_hint_is_non_empty_for_locked_state() {
        assert!(!lesson_unlock_hint(&DomainLessonState::Locked).is_empty());
    }

    #[test]
    fn unlock_hint_preserves_unavailable_reason_text() {
        let reason = "配置冲突：无法生成满足长度要求的文本".to_string();
        assert_eq!(
            lesson_unlock_hint(&DomainLessonState::Unavailable(reason.clone())),
            reason
        );
    }

    // ---- build_lesson_cards ----

    fn sample_lesson(id: &str, goal: LessonGoal) -> Lesson {
        Lesson {
            id: domain::LessonId(id.to_string()),
            title: format!("课程 {id}"),
            goal,
            target_keys: vec![KeyCode::A],
            requires_shift: false,
            unlock_criteria: UnlockCriteria {
                min_accuracy: Some(80.0),
                max_duration_secs: None,
                min_attempts: None,
            },
        }
    }

    #[test]
    fn build_lesson_cards_preserves_order_and_maps_state_per_lesson() {
        let curriculum = Curriculum {
            lessons: vec![
                sample_lesson("lesson-1", LessonGoal::SingleKey),
                sample_lesson("lesson-2", LessonGoal::Word),
                sample_lesson("lesson-3", LessonGoal::Sentence),
            ],
        };
        let states = vec![
            DomainLessonState::Unlocked,
            DomainLessonState::Locked,
            DomainLessonState::Unavailable("损坏".to_string()),
        ];

        let cards = build_lesson_cards(&curriculum, &states);

        assert_eq!(cards.len(), 3);
        assert_eq!(cards[0].state, LessonState::Unlocked);
        assert_eq!(cards[0].subtitle, slint::SharedString::from("单键练习"));
        assert_eq!(cards[0].unlock_hint, slint::SharedString::from(""));
        assert_eq!(cards[1].state, LessonState::Locked);
        assert_eq!(cards[1].subtitle, slint::SharedString::from("词语练习"));
        assert!(!cards[1].unlock_hint.is_empty());

        assert_eq!(cards[2].state, LessonState::Unavailable);
        assert_eq!(cards[2].subtitle, slint::SharedString::from("句子练习"));
        assert_eq!(cards[2].unlock_hint, slint::SharedString::from("损坏"));
    }

    #[test]
    fn build_lesson_cards_uses_human_readable_title_not_lesson_id() {
        let curriculum = Curriculum {
            lessons: vec![Lesson {
                id: domain::LessonId("home-row-left-asdf".to_string()),
                title: "左手基准键 ASDF".to_string(),
                goal: LessonGoal::SingleKey,
                target_keys: vec![KeyCode::A],
                requires_shift: false,
                unlock_criteria: UnlockCriteria {
                    min_accuracy: Some(80.0),
                    max_duration_secs: None,
                    min_attempts: None,
                },
            }],
        };
        let states = vec![DomainLessonState::Unlocked];

        let cards = build_lesson_cards(&curriculum, &states);

        assert_eq!(cards[0].title, slint::SharedString::from("左手基准键 ASDF"));
        assert_ne!(
            cards[0].title,
            slint::SharedString::from("home-row-left-asdf")
        );
    }

    #[test]
    fn build_lesson_cards_titles_of_builtin_curriculum_are_all_human_readable() {
        let curriculum = domain::builtin_curriculum();
        let states = vec![DomainLessonState::Unlocked; curriculum.lessons.len()];

        let cards = build_lesson_cards(&curriculum, &states);

        for (card, lesson) in cards.iter().zip(curriculum.lessons.iter()) {
            assert!(!card.title.is_empty());
            assert_ne!(card.title.as_str(), lesson.id.0.as_str());
        }
    }

    // ---- 新建档案 -> 课程序列界面的完整 Rust 链路（回归测试） ----

    /// 分区测试：验证「输入昵称 -> 创建档案 -> 课程序列界面数据就绪」这条
    /// 链路在 Rust 侧是完好的（不涉及任何 Slint 窗口实例化，因此可在
    /// `cargo test` 的非主线程 worker 上运行）。
    ///
    /// 覆盖四个环节：
    /// 1. `dispatch(CreateProfile)` 恰好返回
    ///    `[NavigateTo(CurriculumSequence)]`——若返回 `ShowToast(..)`，说明
    ///    档案创建本身失败（校验不通过或落盘失败），断言失败信息会带出该
    ///    错误文案便于定位。
    /// 2. 档案激活后 `curriculum_state()` 为 `Some`（否则
    ///    `refresh_lessons` 会写入空数组，界面上"什么都没有"）。
    /// 3. `build_lesson_cards` 为内置课程序列的每一门课各产出一张卡片（Req 1.1），
    ///    首课 `Unlocked`、其余 `Locked`（Req 1.2, 6.3），且标题均非空。
    /// 4. `navigation_target_to_app_page` 把 `CurriculumSequence` 映射为
    ///    `AppPage::CurriculumMap`（`app.slint` 中课程序列页的路由条件）。
    #[test]
    fn create_profile_prepares_curriculum_map_page_data_end_to_end() {
        let dir = std::env::temp_dir().join(format!(
            "typing-ui-bridge-test-create-profile-flow-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir for test");
        let store = crate::storage::ProfileStore::new(&dir);
        let mut controller = AppController::new(store, domain::builtin_curriculum());

        // 环节 1：创建档案应只产生一条"导航到课程序列界面"的命令。
        let commands = controller.dispatch(AppEvent::CreateProfile {
            nickname: "小明".to_string(),
        });
        assert_eq!(
            commands,
            vec![UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)],
            "创建档案未返回导航命令，实际命令：{commands:?}"
        );

        // 环节 2：课程解锁状态视图必须已构造，否则 `refresh_lessons` 会
        // 写入空的 `lessons` 数组。
        let curriculum_state = controller
            .curriculum_state()
            .expect("创建档案后 curriculum_state() 应为 Some");

        // 环节 3：课程卡片视图模型。
        //
        // 期望张数直接取自内置课程序列的长度，而不是写死一个数字：这里要验证的
        // 是"每一门课都产出一张卡片"这条对应关系，课程序列本身增删课程时不应该
        // 让这个端到端测试失败（课程内容的约束由
        // `domain::curriculum` 的分组/覆盖测试负责）。
        let cards = build_lesson_cards(controller.curriculum(), curriculum_state.lesson_states());
        let expected_cards = domain::builtin_curriculum().lessons.len();
        assert_eq!(
            cards.len(),
            expected_cards,
            "内置课程序列有 {expected_cards} 门课，应产出同样张数的课程卡片"
        );
        assert_eq!(
            cards[0].state,
            LessonState::Unlocked,
            "首课应为 Unlocked（Req 1.2, 6.3）"
        );
        for (index, card) in cards.iter().enumerate().skip(1) {
            assert_eq!(
                card.state,
                LessonState::Locked,
                "新建档案时第 {} 张卡片应为 Locked",
                index + 1
            );
        }
        for (index, card) in cards.iter().enumerate() {
            assert!(
                !card.title.is_empty(),
                "第 {} 张课程卡片的标题不应为空",
                index + 1
            );
        }

        // 环节 4：导航目标 -> `app.slint` 页面枚举的映射。
        assert_eq!(
            navigation_target_to_app_page(NavigationTarget::CurriculumSequence),
            AppPage::CurriculumMap
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- build_practice_char_views ----

    #[test]
    fn build_practice_char_views_reflects_correct_pending_and_error_states() {
        let text: Vec<char> = "ab".chars().collect();
        let mut state = TypingMatchState::new(text);

        // 第一个字符正确输入 -> Correct；第二个字符输入错误 -> Error（当前
        // cursor 指向且 current_char_has_error 为 true）。
        let (s, _) = apply_input(&state, 'a');
        state = s;
        let (s, _) = apply_input(&state, 'x');
        state = s;

        let views = build_practice_char_views(&state);

        assert_eq!(views.len(), 2);
        assert_eq!(views[0].ch, slint::SharedString::from("a"));
        assert_eq!(views[0].state, PracticeCharState::Correct);
        assert_eq!(views[1].ch, slint::SharedString::from("b"));
        assert_eq!(views[1].state, PracticeCharState::Error);
    }

    #[test]
    fn build_practice_char_views_marks_untouched_chars_as_pending() {
        let text: Vec<char> = "abc".chars().collect();
        let state = TypingMatchState::new(text);

        let views = build_practice_char_views(&state);

        assert!(views.iter().all(|v| v.state == PracticeCharState::Pending));
    }

    // ---- build_key_states ----

    #[test]
    fn build_key_states_has_exactly_one_hint_key_when_not_complete() {
        let text: Vec<char> = "ab".chars().collect();
        let state = TypingMatchState::new(text);

        let key_states = build_key_states(&state);

        assert_eq!(key_states.len(), KEY_LAYOUT_ORDER.len());
        let hint_count = key_states.iter().filter(|k| k.is_hint).count();
        assert_eq!(hint_count, 1);
    }

    #[test]
    fn build_key_states_has_no_hint_key_when_complete() {
        let text: Vec<char> = "a".chars().collect();
        let mut state = TypingMatchState::new(text);
        let (s, _) = apply_input(&state, 'a');
        state = s;
        assert!(state.is_complete());

        let key_states = build_key_states(&state);

        assert!(key_states.iter().all(|k| !k.is_hint));
    }

    #[test]
    fn build_key_states_marks_only_keys_present_in_practice_text_as_active() {
        let text: Vec<char> = "aj".chars().collect();
        let state = TypingMatchState::new(text);

        let key_states = build_key_states(&state);

        let active_labels: Vec<&str> = KEY_LAYOUT_ORDER
            .iter()
            .zip(key_states.iter())
            .filter(|(_, k)| k.is_active)
            .map(|(key, _)| match key {
                KeyCode::A => "A",
                KeyCode::J => "J",
                _ => "other",
            })
            .collect();

        assert_eq!(active_labels.len(), 2);
        assert!(active_labels.contains(&"A"));
        assert!(active_labels.contains(&"J"));
    }

    #[test]
    fn build_key_states_assigns_expected_finger_zone_for_known_key() {
        let text: Vec<char> = "a".chars().collect();
        let state = TypingMatchState::new(text);

        let key_states = build_key_states(&state);
        // KEY_LAYOUT_ORDER 中 'A' 位于 ASDF 行第一个（索引 26）。
        let a_index = KEY_LAYOUT_ORDER
            .iter()
            .position(|&k| k == KeyCode::A)
            .expect("A should be present in layout");

        assert_eq!(key_states[a_index].zone, FingerZoneId::LeftPinky);
    }

    /// Req 2.1 的扩展：`shift-label` 必须在**任何练习状态**下都恒定显示
    /// （不随文本内容/提示位置变化），且与真实键盘键帽的印刷一致——字母键
    /// 留空，符号键显示对应的 Shift 组合字符。
    #[test]
    fn build_key_states_shift_label_matches_physical_keycap_for_every_key() {
        let text: Vec<char> = "a".chars().collect();
        let state = TypingMatchState::new(text);
        let key_states = build_key_states(&state);

        let index_of = |key: KeyCode| {
            KEY_LAYOUT_ORDER
                .iter()
                .position(|&k| k == key)
                .expect("key should be present in layout")
        };

        // 数字行/符号键：显示对应的 Shift 组合字符。
        assert_eq!(
            key_states[index_of(KeyCode::Digit1)].shift_label,
            slint::SharedString::from("!")
        );
        assert_eq!(
            key_states[index_of(KeyCode::Semicolon)].shift_label,
            slint::SharedString::from(":")
        );
        assert_eq!(
            key_states[index_of(KeyCode::Quote)].shift_label,
            slint::SharedString::from("\"")
        );
        assert_eq!(
            key_states[index_of(KeyCode::Slash)].shift_label,
            slint::SharedString::from("?")
        );

        // 字母键与空格：不重复显示（label 本身已是大写字母/无 Shift 变体）。
        assert_eq!(
            key_states[index_of(KeyCode::A)].shift_label,
            slint::SharedString::from("")
        );
        assert_eq!(
            key_states[index_of(KeyCode::Space)].shift_label,
            slint::SharedString::from("")
        );
    }

    // ---- layout_index_of_char（敲击动画的键位定位） ----

    /// 动画要点亮的必须是**真正被按下的那个键**：对每一个标准键位，用它的字符
    /// 反查出来的下标必须等于它在 `KEY_LAYOUT_ORDER` 中的位置——这个下标同时被
    /// `.slint` 用于取 `KeyState`，一旦两者口径不一致，按下 A 会让别的键下沉。
    #[test]
    fn layout_index_of_char_agrees_with_key_layout_order() {
        for (expected_index, &key) in KEY_LAYOUT_ORDER.iter().enumerate() {
            let ch = crate::domain::key_to_char(key);
            assert_eq!(
                layout_index_of_char(ch),
                Some(expected_index),
                "字符 {ch:?} 应定位到 KEY_LAYOUT_ORDER 的第 {expected_index} 项"
            );
        }
    }

    #[test]
    fn layout_index_of_char_returns_none_for_keys_outside_the_layout() {
        // 未能还原的中文标点/汉字不对应任何标准键位：跳过动画，但输入
        // 事件仍会照常派发并被判为错误（Req 3.3）。
        for ch in ['，', '。', '中'] {
            assert_eq!(layout_index_of_char(ch), None, "{ch:?} 不应定位到任何键位");
        }
    }

    #[test]
    fn layout_index_of_char_resolves_uppercase_letters_to_their_physical_key() {
        // 大写字母是已有物理键位的 Shift 取值，应定位到与小写字母相同的下标。
        for ch in ['A', 'Z'] {
            assert_eq!(
                layout_index_of_char(ch),
                layout_index_of_char(ch.to_ascii_lowercase()),
                "{ch:?} 应与其小写字母定位到同一下标"
            );
        }
    }

    // ---- practice_input_char（物理键盘输入的过滤/转换，Req 3.2, 3.3, 3.6） ----

    #[test]
    fn practice_input_char_accepts_a_printable_letter() {
        assert_eq!(practice_input_char("a"), Some('a'));
        assert_eq!(practice_input_char("Z"), Some('Z'));
    }

    #[test]
    fn practice_input_char_accepts_space() {
        // 空格是练习文本中的合法字符（词语/句子练习），必须被接受。
        assert_eq!(practice_input_char(" "), Some(' '));
    }

    #[test]
    fn practice_input_char_rejects_empty_text() {
        assert_eq!(practice_input_char(""), None);
    }

    #[test]
    fn practice_input_char_rejects_multi_char_text() {
        // 输入法组合输入等场景可能一次带来多个字符，没有单字符语义。
        assert_eq!(practice_input_char("ab"), None);
        assert_eq!(practice_input_char("你好"), None);
    }

    #[test]
    fn practice_input_char_rejects_control_characters() {
        assert_eq!(practice_input_char("\n"), None);
        assert_eq!(practice_input_char("\u{8}"), None);
        assert_eq!(practice_input_char("\t"), None);
    }

    #[test]
    fn practice_input_char_rejects_private_use_area_characters() {
        // Slint 把 Esc/方向键等非打印键映射到 U+E000..U+F8FF。
        assert_eq!(practice_input_char("\u{E000}"), None);
        assert_eq!(practice_input_char("\u{F8FF}"), None);
    }

    #[test]
    fn practice_input_char_normalizes_fullwidth_semicolon_from_chinese_ime() {
        // macOS 简体拼音输入法（含英文模式）按下 `;` 送达的是全角分号
        // U+FF1B；孩子按的是正确键位，不应被判为输入错误。
        assert_eq!(practice_input_char("；"), Some(';'));
    }

    #[test]
    fn practice_input_char_normalizes_fullwidth_ascii_range_by_fixed_offset() {
        // 全角 ASCII 区整段还原：区间两端与若干练习文本中会出现的标点。
        assert_eq!(practice_input_char("！"), Some('!')); // U+FF01 区间下界
        assert_eq!(practice_input_char("～"), Some('~')); // U+FF5E 区间上界
        assert_eq!(practice_input_char("，"), Some(','));
        assert_eq!(practice_input_char("："), Some(':'));
        assert_eq!(practice_input_char("？"), Some('?'));
        assert_eq!(practice_input_char("／"), Some('/'));
        assert_eq!(practice_input_char("ａ"), Some('a')); // 全角字母
        assert_eq!(practice_input_char("１"), Some('1')); // 全角数字
    }

    #[test]
    fn practice_input_char_normalizes_cjk_punctuation_outside_fullwidth_block() {
        // 这些中文标点不在全角 ASCII 区，必须靠显式表还原到产生它们的按键。
        assert_eq!(practice_input_char("。"), Some('.'));
        assert_eq!(practice_input_char("、"), Some('\\'));
        assert_eq!(practice_input_char("·"), Some('`'));
        assert_eq!(practice_input_char("—"), Some('-'));
        assert_eq!(practice_input_char("‘"), Some('\''));
        assert_eq!(practice_input_char("’"), Some('\''));
        assert_eq!(practice_input_char("【"), Some('['));
        assert_eq!(practice_input_char("】"), Some(']'));
        assert_eq!(practice_input_char("「"), Some('['));
        assert_eq!(practice_input_char("」"), Some(']'));
        assert_eq!(practice_input_char("\u{3000}"), Some(' ')); // 全角空格
    }

    #[test]
    fn practice_input_char_normalization_targets_are_all_typable_keys() {
        // 还原结果必须落在 `char_to_key` 的定义域内（即键盘上真实存在的
        // 键位），否则"还原"就没有意义——这是本次修改的核心不变式：
        // 输入法产出的全角变体 -> 该变体所对应的那个物理按键的字符。
        for source in [
            '；', '，', '。', '、', '·', '‘', '’', '【', '】', '「', '」', '\u{3000}', 'ａ', '１',
            '－', '＝', '［', '］', '＼', '＇', '／',
        ] {
            let normalized =
                practice_input_char(&source.to_string()).expect("单个可打印字符不应被过滤");
            assert!(
                crate::domain::char_to_key(normalized).is_some(),
                "全角/中文标点 {source:?} 还原为 {normalized:?}，但它不对应任何键位"
            );
        }
    }

    #[test]
    fn practice_input_char_leaves_halfwidth_characters_unchanged() {
        // 规范化不得影响本来就正确的半角输入（回归保护）。
        for ch in "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./ ".chars() {
            assert_eq!(
                practice_input_char(&ch.to_string()),
                Some(ch),
                "半角字符 {ch:?} 不应被规范化改写"
            );
        }
    }

    #[test]
    fn profile_management_callback_setters_have_expected_signatures() {
        // 与 `practice_char_input_callback_setter_has_expected_signature` 同理：
        // `AppWindow` 需在主线程实例化，因此只做类型级校验——确认
        // `app.slint` 确实导出了三个档案管理回调，且参数类型与桥接层派发
        // `AppEvent::{DeleteProfile, ResetProfileProgress, RenameProfile}` 时
        // 需要的输入可对齐（profile-id 字符串 / profile-id + 昵称）。
        let _: fn(&AppWindow, fn(slint::SharedString)) = |window, handler| {
            window.on_delete_profile(handler);
        };
        let _: fn(&AppWindow, fn(slint::SharedString)) = |window, handler| {
            window.on_reset_profile_progress(handler);
        };
        let _: fn(&AppWindow, fn(slint::SharedString, slint::SharedString)) = |window, handler| {
            window.on_rename_profile(handler);
        };
    }

    #[test]
    fn practice_char_input_callback_setter_has_expected_signature() {
        // 与 `create_main_window_fn_has_expected_signature` 同理：`AppWindow`
        // 需在主线程实例化，因此这里只做类型级校验——确认 `app.slint` 确实
        // 导出了 `practice-char-input(string)` 回调，且其参数类型与
        // `practice_input_char` 的入参可对齐。
        let _: fn(&AppWindow, fn(slint::SharedString)) = |window, handler| {
            window.on_practice_char_input(handler);
        };
    }

    // ---- format_elapsed ----

    #[test]
    fn format_elapsed_formats_as_mm_ss() {
        assert_eq!(format_elapsed(std::time::Duration::from_secs(0)), "00:00");
        assert_eq!(format_elapsed(std::time::Duration::from_secs(59)), "00:59");
        assert_eq!(format_elapsed(std::time::Duration::from_secs(60)), "01:00");
        assert_eq!(format_elapsed(std::time::Duration::from_secs(125)), "02:05");
    }

    // ---- lesson_id_at ----

    #[test]
    fn lesson_id_at_returns_none_for_out_of_range_index() {
        let dir = std::env::temp_dir().join(format!(
            "typing-ui-bridge-test-lesson-id-at-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir for test");
        let store = crate::storage::ProfileStore::new(&dir);
        let controller = AppController::new(store, domain::builtin_curriculum());

        assert!(lesson_id_at(&controller, -1).is_none());
        assert!(lesson_id_at(&controller, 999).is_none());
        assert_eq!(
            lesson_id_at(&controller, 0),
            Some(domain::builtin_curriculum().lessons[0].id.clone())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- toast_target_for_page（ShowToast 的展示位路由） ----

    #[test]
    fn toast_on_profile_select_page_routes_to_inline_nickname_error() {
        // Req 6.5, 6.7：昵称校验错误必须内联展示在输入框旁（并保留用户输入），
        // 而不是几秒后自动消失的全局 Toast。
        assert_eq!(
            toast_target_for_page(AppPage::ProfileSelect),
            ToastTarget::NicknameError
        );
    }

    #[test]
    fn toast_on_other_pages_routes_to_global_toast_overlay() {
        assert_eq!(
            toast_target_for_page(AppPage::CurriculumMap),
            ToastTarget::GlobalToast
        );
        assert_eq!(
            toast_target_for_page(AppPage::PracticeView),
            ToastTarget::GlobalToast
        );
    }

    // ---- expected_char_of（Req 7.6 的"正确答案"展示文本） ----

    #[test]
    fn expected_char_of_returns_char_at_cursor() {
        let state = TypingMatchState::new("abc".chars().collect());
        assert_eq!(expected_char_of(&state), "a");

        let (state, _) = apply_input(&state, 'a');
        assert_eq!(expected_char_of(&state), "b");
    }

    #[test]
    fn expected_char_of_renders_space_as_visible_label() {
        let state = TypingMatchState::new(vec![' ', 'a']);
        assert_eq!(expected_char_of(&state), "空格");
    }

    #[test]
    fn expected_char_of_is_empty_when_text_is_complete() {
        let state = TypingMatchState::new("a".chars().collect());
        let (state, _) = apply_input(&state, 'a');
        assert!(state.is_complete());
        assert_eq!(expected_char_of(&state), "");
    }

    #[test]
    fn expected_char_display_is_empty_without_active_practice_session() {
        let dir = std::env::temp_dir().join(format!(
            "typing-ui-bridge-test-expected-char-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir for test");
        let store = crate::storage::ProfileStore::new(&dir);
        let controller = AppController::new(store, domain::builtin_curriculum());

        assert_eq!(expected_char_display(&controller), "");

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- build_reward_overlay_view（RewardFeedback -> Slint 属性） ----

    #[test]
    fn build_reward_overlay_view_maps_every_field() {
        let feedback = domain::RewardFeedback {
            animation: true,
            badge: false,
            encouragement_text: Some("太棒了"),
            display_duration_ms: 3500,
        };

        let view = build_reward_overlay_view(&feedback);

        assert!(view.show_animation);
        assert!(!view.show_badge);
        assert_eq!(view.encouragement_text, "太棒了");
        assert_eq!(view.display_duration_ms, 3500);
    }

    #[test]
    fn build_reward_overlay_view_maps_none_encouragement_to_empty_string() {
        let feedback = domain::RewardFeedback {
            animation: false,
            badge: true,
            encouragement_text: None,
            display_duration_ms: domain::MIN_REWARD_DISPLAY_DURATION_MS,
        };

        let view = build_reward_overlay_view(&feedback);

        assert_eq!(view.encouragement_text, "");
        assert_eq!(view.display_duration_ms, 2000);
    }

    #[test]
    fn build_reward_overlay_view_of_builtin_reward_generator_is_displayable() {
        let feedback =
            domain::generate_lesson_completion_reward(&domain::LessonId("lesson-1".to_string()));
        let view = build_reward_overlay_view(&feedback);

        // Req 7.3：三种形式至少一种存在，且展示时长不少于 2 秒。
        assert!(view.show_animation || view.show_badge || !view.encouragement_text.is_empty());
        assert!(view.display_duration_ms >= 2000);
    }

    // ---- comparison_result_to_slint（ComparisonResult -> ComparisonDirection） ----

    #[test]
    fn comparison_result_maps_to_expected_direction_and_deltas() {
        assert_eq!(
            comparison_result_to_slint(ComparisonResult::FirstRecord),
            (ComparisonDirection::FirstRecord, 0.0, 0.0)
        );
        assert_eq!(
            comparison_result_to_slint(ComparisonResult::Equal),
            (ComparisonDirection::Equal, 0.0, 0.0)
        );
        assert_eq!(
            comparison_result_to_slint(ComparisonResult::Higher {
                accuracy_delta: 2.5,
                wpm_delta: 1.5,
            }),
            (ComparisonDirection::Higher, 2.5, 1.5)
        );
        assert_eq!(
            comparison_result_to_slint(ComparisonResult::Lower {
                accuracy_delta: -3.0,
                wpm_delta: -0.5,
            }),
            (ComparisonDirection::Lower, -3.0, -0.5)
        );
    }

    // ---- previous_best_excluding_current / best_of（历史最佳提取） ----

    fn stored_result(accuracy: f32, wpm: f32, duration_ms: u64) -> storage::PracticeResult {
        storage::PracticeResult {
            lesson_id: storage::LessonId("lesson-1".to_string()),
            accuracy,
            wpm,
            error_count: 0,
            duration_ms,
            score: 0,
            completed_at: chrono::Utc::now(),
        }
    }

    fn record_with_history(history: Vec<storage::PracticeResult>) -> storage::LessonRecord {
        storage::LessonRecord {
            best_accuracy: history
                .iter()
                .map(|r| r.accuracy)
                .fold(0.0f32, f32::max),
            best_wpm: history.iter().map(|r| r.wpm).fold(0.0f32, f32::max),
            best_score: 0,
            achieved_at: chrono::Utc::now(),
            attempt_count: history.len() as u32,
            history,
        }
    }

    #[test]
    fn previous_best_is_none_when_current_is_the_only_history_entry() {
        // 本次练习是该课程的首次练习：`record_practice_result` 已把本次成绩
        // push 进 history，剔除后 history 为空 -> 首次记录（Req 4.5）。
        let current = stored_result(90.0, 20.0, 1000);
        let record = record_with_history(vec![current.clone()]);

        assert!(previous_best_excluding_current(&record, &current).is_none());
    }

    #[test]
    fn previous_best_excludes_current_result_from_history() {
        // 历史两次（80/85），本次 95 破纪录。若不剔除本次，best_* 会等于本次
        // 成绩，比较结果被误判为 Equal。
        let older = stored_result(80.0, 15.0, 3000);
        let previous = stored_result(85.0, 18.0, 2500);
        let current = stored_result(95.0, 25.0, 2000);
        let record = record_with_history(vec![older, previous.clone(), current.clone()]);

        let best = previous_best_excluding_current(&record, &current)
            .expect("history 中剔除本次后仍有记录");

        assert_eq!(best.accuracy, previous.accuracy);
        assert_eq!(best.wpm, previous.wpm);
    }

    #[test]
    fn previous_best_prefers_accuracy_then_wpm() {
        let a = stored_result(90.0, 10.0, 3000);
        let b = stored_result(90.0, 30.0, 2000);
        let c = stored_result(88.0, 99.0, 1000);
        let current = stored_result(70.0, 5.0, 5000);
        let record = record_with_history(vec![a, b.clone(), c, current.clone()]);

        let best = previous_best_excluding_current(&record, &current)
            .expect("history 中剔除本次后仍有记录");

        // 正确率优先：90.0 组胜出；组内比较 WPM：30.0 胜出。
        assert_eq!(best.accuracy, b.accuracy);
        assert_eq!(best.wpm, b.wpm);
    }

    #[test]
    fn previous_best_keeps_all_history_when_current_is_not_recorded() {
        // 防御式行为：若 `ShowPracticeResult` 由未经 `record_practice_result`
        // 的路径产生（history 中不含本次成绩），不应误删一条真实历史记录。
        let previous = stored_result(85.0, 18.0, 2500);
        let record = record_with_history(vec![previous.clone()]);
        let current = stored_result(95.0, 25.0, 2000);

        let best = previous_best_excluding_current(&record, &current)
            .expect("history 中的唯一历史记录应被保留");

        assert_eq!(best.accuracy, previous.accuracy);
    }

    #[test]
    fn previous_best_is_none_for_empty_history() {
        let current = stored_result(90.0, 20.0, 1000);
        let record = record_with_history(Vec::new());

        assert!(previous_best_excluding_current(&record, &current).is_none());
    }

    // ---- build_result_summary_view ----

    #[test]
    fn build_result_summary_view_marks_first_record_when_no_previous_best() {
        let current = stored_result(92.5, 21.0, 12_345);

        let view = build_result_summary_view(&current, None);

        assert_eq!(view.duration_ms, 12_345);
        assert_eq!(view.error_count, 0);
        assert_eq!(view.accuracy, 92.5);
        assert_eq!(view.wpm, 21.0);
        assert_eq!(view.comparison_direction, ComparisonDirection::FirstRecord);
        assert_eq!(view.accuracy_delta, 0.0);
        assert_eq!(view.wpm_delta, 0.0);
    }

    #[test]
    fn build_result_summary_view_reports_higher_with_concrete_deltas() {
        let previous_best = stored_result(80.0, 15.0, 3000);
        let current = stored_result(90.0, 20.0, 2000);

        let view = build_result_summary_view(&current, Some(&previous_best));

        assert_eq!(view.comparison_direction, ComparisonDirection::Higher);
        assert!((view.accuracy_delta - 10.0).abs() < 1e-3);
        assert!((view.wpm_delta - 5.0).abs() < 1e-3);
    }

    #[test]
    fn build_result_summary_view_reports_lower_with_negative_deltas() {
        let previous_best = stored_result(95.0, 30.0, 2000);
        let current = stored_result(90.0, 20.0, 3000);

        let view = build_result_summary_view(&current, Some(&previous_best));

        assert_eq!(view.comparison_direction, ComparisonDirection::Lower);
        assert!(view.accuracy_delta < 0.0);
        assert!(view.wpm_delta < 0.0);
    }

    #[test]
    fn build_result_summary_view_reports_equal_for_identical_metrics() {
        let previous_best = stored_result(90.0, 20.0, 5000);
        let current = stored_result(90.0, 20.0, 1000);

        let view = build_result_summary_view(&current, Some(&previous_best));

        assert_eq!(view.comparison_direction, ComparisonDirection::Equal);
    }

    #[test]
    fn pending_overlays_starts_empty() {
        // 「先庆祝、后看成绩」串联顺序的初始状态：没有待展示的结果统计。
        let pending = PendingOverlays::default();
        assert!(pending.pending_result.is_none());
    }
}

/// 任务 18.9：静态检查测试——校验 `src/ui/*.slint` 中 slintcn 组件默认样式
/// 常量（最小可点击尺寸 44px、间距 8px）符合 Req 7.1、7.4 规定的数值要求。
///
/// # 为什么是"静态/文本"检查而不是运行时 UI 测试
///
/// Slint 生成的窗口类型（如 `AppWindow`）在 macOS 上只能在主线程创建/运行
/// （`winit` 约束），而 `cargo test` 默认在非主线程的测试 worker 上运行
/// ——本文件上方 `create_main_window_fn_has_expected_signature` 等测试的
/// 注释已经说明了同样的限制。因此本模块不实例化任何 Slint 组件，而是直接
/// 读取 `.slint` 源文件的原始文本，用正则表达式提取与"可点击目标尺寸"/
/// "间距"相关的数值字面量并做数值断言。
///
/// # 已知局限性（有意为之，非遗漏）
///
/// Slint 没有公开的"列出全部样式常量"反射 API 可供 Rust 侧调用，因此这是
/// 一种启发式的文本扫描，而不是通用的 Slint 语法解析器：
/// - 只扫描 `width`/`height`/`min-width`/`min-height` 中明确用于渲染
///   "可点击目标"（`TouchArea`/被 `TouchArea` 包裹的固定尺寸元素/
///   `LineEdit` 输入框）的数值字面量，不尝试解析任意 Slint 表达式
///   （如涉及 `parent.width - self.width` 等算术的属性不在本检查范围）。
/// - 只扫描顶层 `spacing:` 字面量数值（`VerticalLayout`/`HorizontalLayout`
///   /`KeyRow` 的 `spacing:` 属性），不区分该间距是否恰好用在两个可点击
///   元素之间——设计文档与各 `.slint` 文件的注释已经明确标注了哪些
///   `spacing:` 对应 Req 7.1 的"相邻可点击控件间距"约定。
/// - 不校验"未显式设置"的默认值（如某个新增元素完全没有写
///   `min-height`），只校验已经写在源码中的数值字面量本身是否达标。
/// - 对 `min-height`/`min-width` 会额外解析一层"具名长度常量"间接绑定
///   （`min-height: root.card-height;` + `property <length> card-height: 96px;`）。
///   `profile_select.slint`/`curriculum_map.slint` 的列表卡片高度必须写成
///   这种确定的具名常量，Flickable 的 `viewport-height` 才能按
///   「条目数 × (卡片高度 + 间距)」算出真实内容高度（Slint 无法从 repeater
///   生成的元素推导 viewport 尺寸约束）。这层解析是为了让 Req 7.1 的下限
///   断言穿透间接引用，而不是放宽检查。
///
/// 这些局限性在设计文档 Testing Strategy 表中也有对应描述："Req 7.1, 7.4
/// | 快照/静态检查 | 校验 slintcn 组件默认样式常量（最小尺寸、间距）符合
/// 数值要求，非输入相关的属性测试对象"。
#[cfg(test)]
mod slint_style_constant_checks {
    /// 最小可点击目标尺寸（宽/高），对应 Req 7.1："可点击区域的宽度和高度
    /// 均不小于 44 像素"。
    const MIN_TOUCH_TARGET_PX: u32 = 44;

    /// 相邻可点击控件之间的最小间距，对应 Req 7.1："相邻可点击控件之间的
    /// 间距不小于 8 像素"。
    const MIN_SPACING_PX: u32 = 8;

    /// 从 `.slint` 源文本中提取形如 `<property>: <number>px` 的数值（如
    /// `min-height: 44px`、`width: 44px`）。不匹配依赖表达式的写法（如
    /// `width: 100%`、`width: self.width / 2`），这些不是本检查关注的
    /// 固定像素常量声明。
    ///
    /// 有意不引入正则表达式依赖（`regex`/`regex-lite` 均未在 `Cargo.toml`
    /// 中声明为直接依赖）——这里的匹配规则足够简单（固定字面量 `属性名:
    /// 数字px`），手写的逐行/逐 token 扫描即可满足需求，避免为一个纯测试
    /// 用途的静态检查新增生产依赖。
    fn extract_px_values(source: &str, property: &str) -> Vec<u32> {
        property_binding_offsets(source, property)
            .into_iter()
            .filter_map(|offset| parse_colon_px_value(&source.as_bytes()[offset..]))
            .collect()
    }

    /// 返回 `source` 中每一处"合法的属性名出现位置"之后的字节偏移（即属性名
    /// 末尾的下一个字节），供调用方继续解析该属性的绑定值（`44px`、
    /// `root.card-height` 等）。
    fn property_binding_offsets(source: &str, property: &str) -> Vec<usize> {
        let mut results = Vec::new();
        let bytes = source.as_bytes();
        let prop_bytes = property.as_bytes();
        let mut search_from = 0usize;

        while let Some(rel_idx) = find_subslice(&bytes[search_from..], prop_bytes) {
            let match_start = search_from + rel_idx;
            let match_end = match_start + prop_bytes.len();

            // 属性名前一个字符必须不是标识符字符（避免 `min-height` 匹配
            // 到 `height` 子串；也避免匹配到注释中形如 `xheight` 的误报）。
            let prev_is_ident = match_start
                .checked_sub(1)
                .and_then(|i| bytes.get(i))
                .is_some_and(|&b| is_ident_byte(b));
            // 属性名后一个字符必须不是标识符字符（避免 `spacing` 匹配到
            // `spacing-x` 这类不存在于本项目但理论上可能出现的变体）。
            let next_is_ident = bytes.get(match_end).is_some_and(|&b| is_ident_byte(b));

            if !prev_is_ident && !next_is_ident {
                results.push(match_end);
            }

            search_from = match_end;
        }

        results
    }

    /// 解析组件级的具名长度常量声明（如
    /// `private property <length> card-height: 96px;`），返回
    /// `(属性名, 像素值)` 列表。
    ///
    /// 为什么需要它：`profile_select.slint`/`curriculum_map.slint` 的列表
    /// 卡片高度必须是"确定的具名常量"，Flickable 的 `viewport-height` 才能
    /// 按「条目数 × (卡片高度 + 间距)」算出真实内容高度（Slint 无法从
    /// repeater 生成的元素推导 viewport 尺寸约束）。因此这两个文件里卡片的
    /// `min-height` 不再是字面量，而是 `min-height: root.card-height;`
    /// 这样的间接绑定——本函数与 `extract_px_values_resolved` 配合，让
    /// Req 7.1 的下限断言可以穿透这一层间接引用，而不是放宽检查。
    fn extract_length_constants(source: &str) -> Vec<(String, u32)> {
        let mut results = Vec::new();

        for segment in source.split("property <length>").skip(1) {
            let trimmed = segment.trim_start();
            let name: String = trimmed
                .chars()
                .take_while(|&c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            if let Some(value) = parse_colon_px_value(&trimmed.as_bytes()[name.len()..]) {
                results.push((name, value));
            }
        }

        results
    }

    /// 提取某个属性的像素值，既包含直接写在源码中的字面量
    /// （`min-height: 44px;`），也包含经由组件级具名长度常量间接绑定的值
    /// （`min-height: root.card-height;` + `property <length> card-height: 96px;`）。
    /// 若出现无法解析的 `root.<名字>` 引用则直接 panic——那说明常量声明被
    /// 改名/删除，此时应修正源码或本检查，而不是让下限断言静默漏检。
    fn extract_px_values_resolved(source: &str, property: &str) -> Vec<u32> {
        let constants = extract_length_constants(source);
        let mut values = extract_px_values(source, property);

        for offset in property_binding_offsets(source, property) {
            let Some(referenced) = parse_colon_root_property_ref(&source.as_bytes()[offset..])
            else {
                continue;
            };
            let resolved = constants
                .iter()
                .find(|(name, _)| *name == referenced)
                .map(|&(_, value)| value)
                .unwrap_or_else(|| {
                    panic!(
                        "{property}: root.{referenced} 引用了未声明为固定像素常量的属性，无法校验 Req 7.1 下限"
                    )
                });
            values.push(resolved);
        }

        values
    }

    /// 从属性名之后的字节切片中解析 `\s*:\s*root\.(标识符)`，返回被引用的
    /// 属性名。不是这种形式（如字面量 `44px`、`100%`、算术表达式）时返回
    /// `None`。
    fn parse_colon_root_property_ref(rest: &[u8]) -> Option<String> {
        let mut i = 0;
        while rest.get(i).is_some_and(|&b| b.is_ascii_whitespace()) {
            i += 1;
        }
        if rest.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        while rest.get(i).is_some_and(|&b| b.is_ascii_whitespace()) {
            i += 1;
        }

        const PREFIX: &[u8] = b"root.";
        if rest.get(i..i + PREFIX.len()) != Some(PREFIX) {
            return None;
        }
        i += PREFIX.len();

        let name_start = i;
        while rest.get(i).is_some_and(|&b| is_ident_byte(b)) {
            i += 1;
        }
        if i == name_start {
            return None;
        }

        std::str::from_utf8(&rest[name_start..i]).ok().map(str::to_owned)
    }

    fn is_ident_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || needle.len() > haystack.len() {
            return None;
        }
        (0..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
    }

    /// 从属性名之后的字节切片中解析 `\s*:\s*(\d+)px`，返回其中的数字部分。
    /// 只要紧随属性名之后（跳过空白）不是 `:`，或紧随冒号之后（跳过空白）
    /// 不是"数字 + px"，就返回 `None`（说明该属性此处的值是表达式而非
    /// 固定像素字面量，不在本检查范围内，如 `width: 100%`）。
    fn parse_colon_px_value(rest: &[u8]) -> Option<u32> {
        let mut i = 0;
        while rest.get(i).is_some_and(|&b| b.is_ascii_whitespace()) {
            i += 1;
        }
        if rest.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        while rest.get(i).is_some_and(|&b| b.is_ascii_whitespace()) {
            i += 1;
        }

        let digits_start = i;
        while rest.get(i).is_some_and(|b| b.is_ascii_digit()) {
            i += 1;
        }
        if i == digits_start {
            return None;
        }
        let digits = std::str::from_utf8(&rest[digits_start..i]).ok()?;

        if rest.get(i..i + 2) != Some(b"px") {
            return None;
        }

        digits.parse::<u32>().ok()
    }

    fn read_slint(file_name: &str) -> String {
        let path = format!("{}/src/ui/{file_name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
    }

    /// 校验 `profile_select.slint`：档案卡片 `TouchArea`（`min-height`）、
    /// 昵称输入框 `LineEdit`（`min-height`）、创建按钮 `TouchArea`
    /// （`min-width`/`min-height`）均不小于 44px；卡片列表与新建区域的
    /// `spacing` 均不小于 8px（Req 7.1）。
    #[test]
    fn profile_select_touch_targets_and_spacing_meet_minimums() {
        let source = read_slint("profile_select.slint");

        let min_heights = extract_px_values_resolved(&source, "min-height");
        assert!(
            !min_heights.is_empty(),
            "profile_select.slint 应至少声明一个 min-height 常量"
        );
        for value in &min_heights {
            assert!(
                *value >= MIN_TOUCH_TARGET_PX,
                "profile_select.slint 中 min-height: {value}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）"
            );
        }

        let min_widths = extract_px_values_resolved(&source, "min-width");
        for value in &min_widths {
            assert!(
                *value >= MIN_TOUCH_TARGET_PX,
                "profile_select.slint 中 min-width: {value}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）"
            );
        }

        let spacings = extract_px_values(&source, "spacing");
        assert!(
            !spacings.is_empty(),
            "profile_select.slint 应至少声明一个 spacing 常量"
        );
        for value in &spacings {
            assert!(
                *value >= MIN_SPACING_PX,
                "profile_select.slint 中 spacing: {value}px 小于最小间距 {MIN_SPACING_PX}px（Req 7.1）"
            );
        }
    }

    /// 校验 `curriculum_map.slint`：课程卡片 `TouchArea`（`min-height`）不
    /// 小于 44px；卡片列表 `spacing` 不小于 8px（Req 7.1）。
    #[test]
    fn curriculum_map_touch_targets_and_spacing_meet_minimums() {
        let source = read_slint("curriculum_map.slint");

        let min_heights = extract_px_values_resolved(&source, "min-height");
        assert!(
            !min_heights.is_empty(),
            "curriculum_map.slint 应至少声明一个 min-height 常量"
        );
        for value in &min_heights {
            assert!(
                *value >= MIN_TOUCH_TARGET_PX,
                "curriculum_map.slint 中 min-height: {value}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）"
            );
        }

        // 仅校验课程卡片列表所在的 `VerticalLayout { spacing: 8px; for
        // lesson-data[i] in root.lessons: ... }`（即相邻可点击课程卡片之间
        // 的间距，Req 7.1 的真实适用范围）。卡片内部标题/副标题/提示文案
        // 的 `VerticalLayout { spacing: 4px; }` 是非可点击文本元素之间的
        // 排版间距，不在 Req 7.1"相邻可点击控件之间的间距"约束范围内，
        // 因此这里不对整份文件的全部 `spacing` 字面量做无差别断言。
        let lesson_list_block = source
            .split("for lesson-data[i] in root.lessons:")
            .next()
            .expect("curriculum_map.slint 应包含课程卡片列表的 for 循环声明");
        let lesson_list_spacing = extract_px_values(lesson_list_block, "spacing")
            .last()
            .copied()
            .expect("curriculum_map.slint 应在课程卡片列表外层声明 spacing 常量");
        assert!(
            lesson_list_spacing >= MIN_SPACING_PX,
            "curriculum_map.slint 课程卡片列表 spacing: {lesson_list_spacing}px 小于最小间距 {MIN_SPACING_PX}px（Req 7.1）"
        );
    }

    /// 校验 `practice_view.slint`：固定锚点退出按钮 `width`/`height` 不
    /// 小于 44px（Req 7.4）。
    ///
    /// 注：本组件内部的 `HorizontalLayout { spacing: 4px; for char-view[i]
    /// in root.chars: PracticeCharCell { .. } }` 是练习文本字符格
    /// （`PracticeCharCell`）之间的间距——字符格是纯文本展示元素，不是
    /// `TouchArea`/可点击控件，因此不在 Req 7.1"相邻可点击控件之间的
    /// 间距"约束范围内，本测试不对其断言。
    #[test]
    fn practice_view_exit_button_meets_touch_target_minimum() {
        let source = read_slint("practice_view.slint");

        // 退出按钮固定为 `width: 44px; height: 44px;`（字面量），单独定位
        // 提取，避免与 `PracticeCharCell` 的 `width: 28px; height: 36px;`
        // （非可点击目标，仅为文本字符格）混在一起断言。
        let exit_button_block = source
            .split("exit-button := TouchArea")
            .nth(1)
            .expect("practice_view.slint 应包含 exit-button := TouchArea 声明");

        let widths = extract_px_values(exit_button_block, "width");
        let heights = extract_px_values(exit_button_block, "height");
        assert!(
            !widths.is_empty() && !heights.is_empty(),
            "practice_view.slint 的退出按钮应声明 width/height 常量"
        );
        assert!(
            widths[0] >= MIN_TOUCH_TARGET_PX,
            "practice_view.slint 退出按钮 width: {}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.4）",
            widths[0]
        );
        assert!(
            heights[0] >= MIN_TOUCH_TARGET_PX,
            "practice_view.slint 退出按钮 height: {}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.4）",
            heights[0]
        );
    }

    /// 校验 `virtual_keyboard.slint`：`KeyButton`（虚拟键盘按键，可点击
    /// 目标）的 `width`/`height` 不小于 44px（Req 7.1）。
    ///
    /// 键盘改为「按键位单位（1u）排布 + 外壳包裹」的真实键盘造型后，按键
    /// 尺寸不再是写在 `KeyButton` 内的字面量，而是统一由 `KeyMetrics` global
    /// 的 `unit`（1u 边长，同时也是行高）推导：`height: KeyMetrics.unit`、
    /// `width: KeyMetrics.span(units)`，且 `span(n) = n * unit + (n-1) * gap`
    /// 对任意 `n >= 1` 都单调不减。因此校验下限只需盯住两点：`unit` 常量
    /// 本身不小于 44px，且 `KeyButton` 的尺寸确实绑定到该常量（而不是被
    /// 悄悄改回更小的字面量）。
    #[test]
    fn virtual_keyboard_key_button_meets_touch_target_minimum() {
        let source = read_slint("virtual_keyboard.slint");

        let key_unit_px = extract_length_constants(&source)
            .into_iter()
            .find(|(name, _)| name == "unit")
            .map(|(_, value)| value)
            .expect("virtual_keyboard.slint 的 KeyMetrics 应声明固定像素常量 unit");
        assert!(
            key_unit_px >= MIN_TOUCH_TARGET_PX,
            "virtual_keyboard.slint KeyMetrics.unit: {key_unit_px}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）"
        );

        let key_button_block = source
            .split("component KeyButton inherits Rectangle {")
            .nth(1)
            .expect("virtual_keyboard.slint 应包含 KeyButton 组件声明")
            .split("component ChromeKey")
            .next()
            .expect("KeyButton 组件声明后应存在 ChromeKey 组件声明作为边界");

        assert!(
            key_button_block.contains("width: KeyMetrics.span(units);"),
            "KeyButton 的 width 应由 KeyMetrics.span(units) 推导，否则 Req 7.1 的下限无法通过 KeyMetrics.unit 保证"
        );
        assert!(
            key_button_block.contains("height: KeyMetrics.unit;"),
            "KeyButton 的 height 应绑定 KeyMetrics.unit，否则 Req 7.1 的下限无法通过 KeyMetrics.unit 保证"
        );
        // `span(units)` 的下限出现在最窄的键上：所有 KeyButton 的 units 默认
        // 为 1.0，且行内覆写值（1.5 / 6.25）均大于 1，故最小宽度即 unit。
        assert!(
            key_button_block.contains("in property <float> units: 1.0;"),
            "KeyButton 的 units 默认值应为 1.0（最窄键 = 1u = KeyMetrics.unit）"
        );
    }

    /// 回归校验：`virtual_keyboard.slint` 渲染空格键时使用的下标必须等于
    /// `KEY_LAYOUT_ORDER` 中 `Space` 的真实下标。
    ///
    /// 背景：`KEY_LAYOUT_ORDER` 共 48 项（13 + 13 + 11 + 10 + 1），空格位于
    /// 索引 47；而 `.slint` 侧曾按“共 47 键”的错误注释把空格写成
    /// `key-states[46]`，导致空格键实际渲染出 `/` 的标签与右手小指分区色。
    /// 这类跨语言下标错配不会引发编译错误也不会 panic（数组下标合法），
    /// 只能靠这样的静态契约检查兜住。
    #[test]
    fn virtual_keyboard_indexes_space_key_at_its_real_layout_offset() {
        let source = read_slint("virtual_keyboard.slint");

        let layout_order = crate::ui::KEY_LAYOUT_ORDER;
        let space_index = layout_order
            .iter()
            .position(|&k| k == crate::domain::KeyCode::Space)
            .expect("KEY_LAYOUT_ORDER 应包含 Space");
        assert_eq!(
            space_index,
            layout_order.len() - 1,
            "Space 应是 KEY_LAYOUT_ORDER 的最后一项（空格行位于键盘最底部）"
        );

        assert!(
            source.contains(&format!("key-states[{space_index}]")),
            "virtual_keyboard.slint 应以 key-states[{space_index}] 渲染空格键"
        );
        assert!(
            !source.contains(&format!("key-states[{}]", space_index + 1)),
            "virtual_keyboard.slint 不应访问越界下标 key-states[{}]",
            space_index + 1
        );
    }

    /// 校验 `virtual_keyboard.slint`：键盘每一行的「键位单位总和」都必须
    /// 等于 `KeyMetrics.row-units`（标准 ANSI 的 15u）。
    ///
    /// 这是「看起来像一块真键盘」的充要几何条件：由于
    /// `span(n) = n * unit + (n-1) * gap`，只要各行单位总和相同，各行的总
    /// 宽度（含行内间隙）就严格相等、左右边缘对齐；某一行少算/多算 0.25u，
    /// 键盘就会出现肉眼可见的锯齿边缘。这类偏差不会导致编译失败，因此用
    /// 静态解析把每行的单位账目算清楚。
    #[test]
    fn virtual_keyboard_every_row_sums_to_declared_row_units() {
        let source = read_slint("virtual_keyboard.slint");

        let expected_units = parse_float_constant(&source, "row-units")
            .expect("KeyMetrics 应声明 float 常量 row-units");
        let expected_rows = parse_int_constant(&source, "row-count")
            .expect("KeyMetrics 应声明 int 常量 row-count");

        let sums = keyboard_row_unit_sums(&source);
        assert_eq!(
            sums.len(),
            expected_rows as usize,
            "键盘实际渲染的行数应与 KeyMetrics.row-count 一致（board-height 依赖它）"
        );
        for (row_index, sum) in sums.iter().enumerate() {
            assert!(
                (sum - expected_units).abs() < 0.001,
                "第 {} 行的键位单位总和为 {sum}u，与 KeyMetrics.row-units（{expected_units}u）不一致，键盘左右边缘将无法对齐",
                row_index + 1
            );
        }
    }

    /// 解析 `VirtualKeyboard` 中每个 `KeyRow` 的键位单位总和：逐个识别行内的
    /// `KeyButton`/`ChromeKey` 实例（含 `for i in N:` 重复），取其 `units`
    /// 绑定值（未显式绑定则为默认 1.0）累加。
    fn keyboard_row_unit_sums(source: &str) -> Vec<f32> {
        let body = source
            .split("export component VirtualKeyboard")
            .nth(1)
            .expect("virtual_keyboard.slint 应包含 VirtualKeyboard 组件声明");

        let mut sums = Vec::new();
        let mut rest = body;
        while let Some(idx) = rest.find("KeyRow {") {
            // 定位到 `KeyRow` 之后的 `{`，取其平衡花括号块作为该行的内容。
            let brace_start = idx + "KeyRow ".len();
            let row_block = balanced_brace_block(&rest[brace_start..]);
            sums.push(row_unit_sum(row_block));
            rest = &rest[brace_start + row_block.len()..];
        }
        sums
    }

    fn row_unit_sum(row_block: &str) -> f32 {
        const ELEMENTS: [&str; 2] = ["KeyButton {", "ChromeKey {"];
        let mut sum = 0.0;
        let mut rest = row_block;

        loop {
            let Some((idx, keyword)) = ELEMENTS
                .iter()
                .filter_map(|kw| rest.find(kw).map(|i| (i, *kw)))
                .min_by_key(|&(i, _)| i)
            else {
                return sum;
            };

            let repeat = repeat_count_before(&rest[..idx]);
            // keyword 以 `{` 结尾，减 1 即该 `{` 在 rest 中的下标。
            let brace_start = idx + keyword.len() - 1;
            let element_block = balanced_brace_block(&rest[brace_start..]);
            sum += repeat as f32 * bound_units(element_block);
            rest = &rest[brace_start + element_block.len()..];
        }
    }

    /// 元素名之前若紧跟 `for i in <N>:`，说明该元素被重复 N 次。
    fn repeat_count_before(prefix: &str) -> u32 {
        let trimmed = prefix.trim_end();
        let Some(without_colon) = trimmed.strip_suffix(':') else {
            return 1;
        };
        let Some(pos) = without_colon.rfind("for i in ") else {
            return 1;
        };
        without_colon[pos + "for i in ".len()..]
            .trim()
            .parse()
            .unwrap_or(1)
    }

    /// 元素块内 `units: <float>;` 的绑定值；未绑定时返回组件默认值 1.0。
    fn bound_units(element_block: &str) -> f32 {
        let Some(idx) = element_block.find("units:") else {
            return 1.0;
        };
        let digits: String = element_block[idx + "units:".len()..]
            .trim_start()
            .chars()
            .take_while(|&c| c.is_ascii_digit() || c == '.')
            .collect();
        digits
            .parse()
            .unwrap_or_else(|_| panic!("无法解析 units 绑定值：{digits:?}"))
    }

    /// `s` 必须以 `{` 开头，返回含首尾花括号的最小平衡子串。
    fn balanced_brace_block(s: &str) -> &str {
        let bytes = s.as_bytes();
        assert_eq!(bytes.first(), Some(&b'{'), "期望以左花括号开头");

        let mut depth = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'{' {
                depth += 1;
            } else if b == b'}' {
                depth -= 1;
                if depth == 0 {
                    return &s[..=i];
                }
            }
        }
        panic!("花括号不平衡：未找到匹配的右花括号");
    }

    /// 解析 `out property <float> <名字>: <数值>;` 形式的常量。
    fn parse_float_constant(source: &str, name: &str) -> Option<f32> {
        parse_typed_constant(source, "float", name)?.parse().ok()
    }

    /// 解析 `out property <int> <名字>: <数值>;` 形式的常量。
    fn parse_int_constant(source: &str, name: &str) -> Option<u32> {
        parse_typed_constant(source, "int", name)?.parse().ok()
    }

    fn parse_typed_constant(source: &str, type_name: &str, name: &str) -> Option<String> {
        let marker = format!("property <{type_name}> {name}:");
        let idx = source.find(&marker)?;
        Some(
            source[idx + marker.len()..]
                .trim_start()
                .chars()
                .take_while(|&c| c.is_ascii_digit() || c == '.')
                .collect(),
        )
    }

    /// 校验练习文本区始终"只显示一行、宽度与键盘一致"的设计约束：
    /// - `visible-char-rows` 必须恒为 1——不管练习文本多长，界面上任意时刻
    ///   只展示当前正在输入的那一行；随着学员输入到本行末尾，视图自动滚动
    ///   显示下一行（由 [`practice_text_follow_scroll_keeps_cursor_row_visible`]
    ///   验证滚动算术本身的正确性）。
    /// - 每行字符格容量（`chars-per-row`）必须由 `KeyMetrics.board-width`
    ///   （虚拟键盘外壳宽度）反算，而不是窗口宽度——这样文本区在视觉上与
    ///   下方键盘同宽，不会比键盘宽出一截。
    ///
    /// 这是此前"单键/词语课程的文本应在多行内完整展示、不应滚动"设计的替代：
    /// 产品需求变更为"始终单行滚动显示"，因此该断言不再检查"是否需要滚动"
    /// （现在恒需要，这是预期行为），转而锁定新设计的两条结构性约束。
    #[test]
    fn practice_text_area_shows_single_line_matching_keyboard_width() {
        let view_source = read_slint("practice_view.slint");
        let keyboard_source = read_slint("virtual_keyboard.slint");

        let visible_rows =
            parse_int_constant(&view_source, "visible-char-rows").expect("应声明 visible-char-rows");
        assert_eq!(visible_rows, 1, "练习文本区应恒为单行显示");

        // 逐字匹配 `chars-per-row` 的绑定表达式，确认它确实以
        // `KeyMetrics.board-width` 为依据，而不是窗口/父容器宽度。
        assert!(
            view_source.contains(
                "private property <int> chars-per-row: max(1, floor((KeyMetrics.board-width + char-cell-gap) / char-column-pitch));"
            ),
            "chars-per-row 应由 KeyMetrics.board-width 反算，保证文本区与键盘同宽"
        );

        // 逐字匹配 `text-area` 的宽度绑定，确认视觉宽度确实与键盘外壳一致。
        assert!(
            view_source.contains("width: KeyMetrics.board-width;"),
            "text-area 的宽度应绑定到 KeyMetrics.board-width"
        );

        // KeyMetrics.board-width 本身必须是一个有效的正数表达式（间接校验
        // virtual_keyboard.slint 确实导出了这个 global 属性，防止绑定失效后
        // 静默退化为 0 或未定义）。
        let key_constants = extract_length_constants(&keyboard_source);
        let unit = named_px(&key_constants, "unit");
        assert!(unit > 0, "KeyMetrics.unit 应为正数，board-width 才有意义");
    }

    /// 校验超出可见行数的长文本（句子课程，最长 120 字符）仍然可读：
    /// `viewport-y` 必须跟随当前待输入位置滚动，把光标所在行保持在可见窗口内。
    ///
    /// 本测试做两件事，缺一不可：
    /// 1. 确认 `.slint` 里的绑定确实是"由 `cursor` 推导行号 + 双向钳制"这套算术
    ///    （逐字匹配表达式，防止有人把跟随滚动改成固定 `viewport-y: 0`）；
    /// 2. 用同一套算术在 Rust 侧遍历所有可能的光标位置与行数组合，断言"光标所在
    ///    行恒落在可见窗口内"。单靠第 1 步只能证明"写了这个式子"，单靠第 2 步只
    ///    能证明"这个式子是对的"——两者合起来才说明界面上的滚动行为正确。
    #[test]
    fn practice_text_follow_scroll_keeps_cursor_row_visible() {
        let source = read_slint("practice_view.slint");

        // ---- 第 1 步：绑定形态 ----
        for expected in [
            "viewport-y: -root.first-visible-char-row * root.char-row-pitch;",
            "private property <int> cursor-char-row: floor(min(cursor, max(0, chars.length - 1)) / chars-per-row);",
            "private property <int> first-visible-char-row: max(0, min(cursor-char-row - visible-char-rows + 1, char-row-count - visible-char-rows));",
        ] {
            assert!(
                source.contains(expected),
                "practice_view.slint 应包含跟随滚动的绑定：{expected}"
            );
        }

        // ---- 第 2 步：算术不变式 ----
        // 与上面 .slint 中 first-visible-char-row 的表达式一致。
        fn first_visible_row(cursor_row: i32, total_rows: i32, visible_rows: i32) -> i32 {
            0.max((cursor_row - visible_rows + 1).min(total_rows - visible_rows))
        }

        let visible_rows =
            parse_int_constant(&source, "visible-char-rows").expect("应声明 visible-char-rows") as i32;

        for total_rows in 1..=40i32 {
            for cursor_row in 0..total_rows {
                let first = first_visible_row(cursor_row, total_rows, visible_rows);
                assert!(first >= 0, "首个可见行不应为负：{first}");
                assert!(
                    first <= cursor_row && cursor_row < first + visible_rows,
                    "总行数={total_rows} 光标行={cursor_row} 时，可见窗口 [{first}, {}] 不包含光标所在行",
                    first + visible_rows - 1
                );
            }
        }
    }

    /// 校验练习界面的垂直空间预算：文本区 + 虚拟键盘外壳 + 布局内外边距
    /// 之和必须留有余量地放进默认窗口高度，否则键盘会被挤出屏幕底部
    /// （Req 2.2/7.4 要求键盘与退出按钮始终可见）。
    ///
    /// 底部使用独立的 `padding-bottom`（而不是顶部/左右共用的 `padding`）
    /// 把键盘从窗口最下边缘往上顶一些（见 `practice_view.slint` 中该属性
    /// 的说明），因此本测试单独解析 `padding-bottom` 参与预算计算，而不是
    /// 假设四边留白相等。
    #[test]
    fn practice_view_text_area_and_keyboard_fit_window_height() {
        let app_source = read_slint("app.slint");
        let view_source = read_slint("practice_view.slint");
        let keyboard_source = read_slint("virtual_keyboard.slint");

        let window_height = extract_px_values(&app_source, "height")[0];
        let padding = practice_view_body_padding(&view_source);
        let padding_bottom = extract_px_values(practice_view_body_block(&view_source), "padding-bottom")
            .first()
            .copied()
            .expect("practice_view.slint 主体 VerticalLayout 应声明 padding-bottom");
        let spacing = practice_view_body_spacing(&view_source);
        let view_constants = extract_length_constants(&view_source);
        let cell_height = named_px(&view_constants, "char-cell-height");
        let gap = named_px(&view_constants, "char-cell-gap");
        let visible_rows =
            parse_int_constant(&view_source, "visible-char-rows").expect("应声明 visible-char-rows");
        let text_area_height = visible_rows * (cell_height + gap) - gap;

        let key_constants = extract_length_constants(&keyboard_source);
        let unit = named_px(&key_constants, "unit");
        let key_gap = named_px(&key_constants, "gap");
        let case_padding = named_px(&key_constants, "case-padding");
        let key_rows = parse_int_constant(&keyboard_source, "row-count").expect("应声明 row-count");
        let board_height = key_rows * unit + (key_rows - 1) * key_gap + 2 * case_padding;

        // VerticalLayout 有 3 个子元素（顶部信息行、文本区、键盘容器），
        // 因此有 2 段 spacing；上边距为共用的 `padding`，下边距为单独加大的
        // `padding-bottom`。
        let used = padding + padding_bottom + 2 * spacing + text_area_height + board_height;
        assert!(
            used <= window_height,
            "文本区({text_area_height}px) + 键盘({board_height}px) + 边距(上{padding}px+下{padding_bottom}px+间距{}px) = {used}px，\
             超出窗口高度 {window_height}px",
            2 * spacing
        );
        // 顶部信息行（时长文本，字号 16px）需要的高度：余量必须够放它。
        let remaining = window_height - used;
        assert!(
            remaining >= MIN_TOUCH_TARGET_PX,
            "文本区与键盘占满后仅剩 {remaining}px，不足以容纳顶部信息行/退出按钮（{MIN_TOUCH_TARGET_PX}px）"
        );
    }

    /// 练习界面主体 `VerticalLayout` 的 padding（文本区可用宽度 = 窗口宽度 -
    /// 2 × padding）。
    fn practice_view_body_padding(view_source: &str) -> u32 {
        let body = practice_view_body_block(view_source);
        extract_px_values(body, "padding")
            .first()
            .copied()
            .expect("practice_view.slint 主体 VerticalLayout 应声明 padding")
    }

    fn practice_view_body_spacing(view_source: &str) -> u32 {
        let body = practice_view_body_block(view_source);
        extract_px_values(body, "spacing")
            .first()
            .copied()
            .expect("practice_view.slint 主体 VerticalLayout 应声明 spacing")
    }

    /// 定位 `PracticeView` 中承载「信息行 / 文本区 / 键盘」的主体
    /// `VerticalLayout` 块（组件内第一个 `VerticalLayout {`）。
    fn practice_view_body_block(view_source: &str) -> &str {
        let component = view_source
            .split("export component PracticeView")
            .nth(1)
            .expect("应包含 PracticeView 组件声明");
        let idx = component
            .find("VerticalLayout {")
            .expect("PracticeView 应包含主体 VerticalLayout");
        balanced_brace_block(&component[idx + "VerticalLayout ".len()..])
    }

    fn named_px(constants: &[(String, u32)], name: &str) -> u32 {
        constants
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|&(_, value)| value)
            .unwrap_or_else(|| panic!("未找到固定像素常量 {name}"))
    }

    /// 校验 `reward_overlay.slint`：奖励反馈里**声明了固定宽度**的展示元素
    /// （动画圆点 96px、徽章 160px）必须被包在 `alignment: center` 的
    /// `HorizontalLayout` 里。
    ///
    /// 背景：外层只有 `VerticalLayout { alignment: center; }` 时，Slint 的
    /// 纵向布局是通过"把子元素横向拉满整行"来实现水平对齐的；显式声明了宽度
    /// 的子元素不参与拉伸，于是会被摆到行首——奖励图案贴在窗口左边。
    /// `result_summary.slint`/`answer_hint.slint`/`toast.slint` 都已采用
    /// 「VerticalLayout(center) + HorizontalLayout(center)」的双层写法，本检查
    /// 把这一约定固定下来。
    #[test]
    fn reward_overlay_fixed_width_content_is_horizontally_centered() {
        let source = read_slint("reward_overlay.slint");

        for marker in [
            "if root.active && root.show-animation:",
            "if root.active && root.show-badge:",
        ] {
            let idx = source
                .find(marker)
                .unwrap_or_else(|| panic!("reward_overlay.slint 应包含条件分支 {marker:?}"));
            let branch = &source[idx + marker.len()..];
            let element = branch.trim_start();
            assert!(
                element.starts_with("HorizontalLayout {"),
                "{marker:?} 的分支元素应是 HorizontalLayout（用于水平居中），实际以 {:?} 开头",
                &element[..element.find('{').map(|i| i + 1).unwrap_or(element.len()).min(40)]
            );

            let block = balanced_brace_block(&element["HorizontalLayout ".len()..]);
            // 按位置判定：`alignment: center;` 必须是包裹层花括号内的**第一条
            // 声明**。徽章内部还有一层用于排布 "✓" 与文字的
            // `HorizontalLayout { alignment: center; }`，若只用 `contains`
            // 判定，删掉包裹层自己的 alignment 也能被内层那一条"蒙混过关"
            // （这一点是通过变异测试实际发现的）。
            let first_statement = block[1..].trim_start();
            assert!(
                first_statement.starts_with("alignment: center;"),
                "{marker:?} 的包裹 HorizontalLayout 的第一条声明必须是 alignment: center;，\
                 否则固定宽度内容仍会贴左；实际以 {:?} 开头",
                &first_statement[..first_statement.len().min(40)]
            );
        }
    }

    /// 校验 `profile_select.slint`：删除/重置/重命名三个回调**只能**在二次
    /// 确认弹窗内部被调用，档案卡片上的管理按钮只允许打开弹窗（Req 6.9）。
    ///
    /// 这是本功能最重要的一条结构约束：如果哪天有人为了"少点一下"把
    /// `root.delete-profile(...)` 直接挂到卡片上的删除按钮，孩子一次误触就会
    /// 永久删掉几个月的学习记录。这类回归在编译期和运行时都不会报错，只能靠
    /// 静态检查守住。
    #[test]
    fn profile_select_destructive_actions_are_only_reachable_from_confirm_dialog() {
        let source = read_slint("profile_select.slint");

        const MANAGEMENT_CALLBACKS: [&str; 3] = [
            "root.delete-profile(",
            "root.reset-profile-progress(",
            "root.rename-profile(",
        ];

        // 卡片块：从 repeater 头部开始的平衡花括号块。
        let card_marker = "for profile[i] in root.profiles: Rectangle ";
        let card_idx = source
            .find(card_marker)
            .expect("profile_select.slint 应包含档案卡片 repeater");
        let card_block = balanced_brace_block(&source[card_idx + card_marker.len()..]);
        for callback in MANAGEMENT_CALLBACKS {
            assert!(
                !card_block.contains(callback),
                "档案卡片内不得直接调用 {callback}——管理操作必须先经过二次确认弹窗（Req 6.9）"
            );
        }
        // 卡片上的三个按钮只负责打开弹窗。
        assert_eq!(
            card_block.matches("root.open-dialog(").count(),
            3,
            "档案卡片应提供恰好三个管理入口（改名/重置/删除），且都只打开确认弹窗（Req 6.8）"
        );

        // 弹窗块：条件元素 `if root.pending-action != ...: Rectangle { .. }`。
        let dialog_marker = "if root.pending-action != ProfileManagementAction.none: Rectangle ";
        let dialog_idx = source
            .find(dialog_marker)
            .expect("profile_select.slint 应包含二次确认弹窗");
        let dialog_block = balanced_brace_block(&source[dialog_idx + dialog_marker.len()..]);
        for callback in MANAGEMENT_CALLBACKS {
            assert!(
                dialog_block.contains(callback),
                "二次确认弹窗内应存在 {callback} 的调用点（确认后才执行操作）"
            );
        }

        // 三个回调在整个文件里的调用点都必须落在弹窗块内：逐个统计出现次数，
        // 文件内总次数应与弹窗块内次数相等。
        for callback in MANAGEMENT_CALLBACKS {
            assert_eq!(
                source.matches(callback).count(),
                dialog_block.matches(callback).count(),
                "{callback} 存在弹窗之外的调用点，绕过了二次确认（Req 6.9）"
            );
        }
    }

    /// 校验 `profile_select.slint`：确认弹窗的"取消"分支不得触发任何档案操作
    /// 回调，只能关闭弹窗（Req 6.10：取消不修改任何数据）。
    #[test]
    fn profile_select_cancel_button_only_closes_the_dialog() {
        let source = read_slint("profile_select.slint");

        let marker = "cancel-button := TouchArea ";
        let idx = source
            .find(marker)
            .expect("确认弹窗应包含取消按钮 cancel-button");
        let block = balanced_brace_block(&source[idx + marker.len()..]);

        assert!(
            block.contains("root.close-dialog();"),
            "取消按钮应调用 close-dialog() 关闭弹窗"
        );
        for callback in [
            "root.delete-profile(",
            "root.reset-profile-progress(",
            "root.rename-profile(",
        ] {
            assert!(
                !block.contains(callback),
                "取消按钮不得调用 {callback}（Req 6.10：取消不得修改任何档案数据）"
            );
        }
    }

    /// 校验配色的单一来源：除 `theme.slint` 以外的任何 `.slint` 文件都不得出现
    /// 颜色字面量，颜色只能引用 `Theme` 令牌。
    ///
    /// 这条约束是"整套主题可以整体换掉"的前提。此前 8 个 `.slint` 文件里散落着
    /// 50 多个互不相干的字面量（Material 的 `#4caf50`、随手取的 `#555555` 等），
    /// 同一语义在不同页面用不同颜色，改主题只能靠全局搜索替换。
    ///
    /// 扫描前先去掉行注释：注释里存在 `slint-ui/slint#407` 这类 issue 引用，
    /// 会被误当成颜色字面量。
    #[test]
    fn slint_theme_tokens_are_the_only_source_of_colors() {
        for file in [
            "app.slint",
            "profile_select.slint",
            "curriculum_map.slint",
            "practice_view.slint",
            "virtual_keyboard.slint",
            "result_summary.slint",
            "reward_overlay.slint",
            "answer_hint.slint",
            "toast.slint",
        ] {
            let source = read_slint(file);
            let literals = color_literals_outside_comments(&source);
            assert!(
                literals.is_empty(),
                "{file} 中存在颜色字面量 {literals:?}——颜色必须引用 theme.slint 的 Theme 令牌"
            );
        }

        // 反向确认：令牌确实定义在 theme.slint 里（否则上面的断言可能只是
        // 因为所有颜色都被误删而"通过"）。
        let theme = read_slint("theme.slint");
        let theme_literals = color_literals_outside_comments(&theme);
        assert!(
            theme_literals.len() >= 30,
            "theme.slint 应集中定义全部色值，实际只找到 {} 个",
            theme_literals.len()
        );
        for token in [
            "background",
            "foreground",
            "card",
            "muted",
            "muted-foreground",
            "border",
            "primary",
            "secondary",
            "destructive",
            "ring",
            "overlay",
            "radius",
        ] {
            assert!(
                theme.contains(&format!("> {token}:")),
                "theme.slint 应包含 shadcn 语义令牌 {token}"
            );
        }
    }

    /// 去掉每一行的行注释，只留代码部分。
    ///
    /// 这两类静态检查都必须先做这一步：`.slint` 文件里大量注释会引用令牌名、
    /// issue 编号（`slint#407`）等，直接对全文做子串匹配会把解释文字误判成代码。
    fn strip_line_comments(source: &str) -> String {
        source
            .lines()
            .map(|line| match line.find("//") {
                Some(idx) => &line[..idx],
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 提取源码中出现在**代码**（而非行注释）里的颜色字面量。
    fn color_literals_outside_comments(source: &str) -> Vec<String> {
        let mut found = Vec::new();
        for line in source.lines() {
            let code = match line.find("//") {
                Some(idx) => &line[..idx],
                None => line,
            };
            let bytes = code.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'#' {
                    let start = i + 1;
                    let mut end = start;
                    while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                        end += 1;
                    }
                    if end - start >= 3 {
                        found.push(code[i..end].to_string());
                    }
                    i = end.max(i + 1);
                } else {
                    i += 1;
                }
            }
        }
        found
    }

    /// 校验 `curriculum_map.slint`：课程序列界面必须提供返回档案选择界面的
    /// 入口，且它只转发 `back-requested()` 意图、不自行导航。
    ///
    /// 这条返回路径是档案管理功能在启动之后唯一可达的路径：缺了它，一旦选定
    /// 档案，整个会话就再也回不到能删除/重置/重命名档案的界面。
    #[test]
    fn curriculum_map_provides_a_back_entry_to_profile_select() {
        let source = read_slint("curriculum_map.slint");

        assert!(
            source.contains("callback back-requested();"),
            "curriculum_map.slint 应声明 back-requested() 回调"
        );
        let marker = "back-button := TouchArea ";
        let idx = source
            .find(marker)
            .expect("curriculum_map.slint 应包含返回按钮 back-button");
        let block = balanced_brace_block(&source[idx + marker.len()..]);
        assert!(
            block.contains("root.back-requested();"),
            "返回按钮应上抛 back-requested()，由 Rust 侧决定导航"
        );

        // app.slint 必须把它接到窗口级回调上，否则按钮点了没反应。
        let app = read_slint("app.slint");
        assert!(
            app.contains("back-requested() => { root.exit-to-profile-select(); }"),
            "app.slint 应把 CurriculumMap 的 back-requested 接到 exit-to-profile-select"
        );
    }

    /// 校验 `virtual_keyboard.slint`：非本课键位必须被视觉淡化，且淡化方式是
    /// 降低**同一分区色相**的浓度（而不是统一涂成中性灰）。
    ///
    /// 两条约束缺一不可：
    /// - 淡化本身是"练 asdfg 时只有这几个键显眼"的前提；
    /// - 保留分区色相是 Req 2.1 的要求（每个手指分区用唯一且可区分的颜色标识、
    ///   同一分区所有按键颜色一致）。若把非本课键位改成中性灰，这些键上的分区
    ///   信息就消失了。
    #[test]
    fn virtual_keyboard_dims_keys_outside_the_current_lesson() {
        let source = read_slint("virtual_keyboard.slint");

        let marker = "component KeyButton inherits Rectangle ";
        let idx = source
            .find(marker)
            .expect("virtual_keyboard.slint 应包含 KeyButton 组件");
        let block = balanced_brace_block(&source[idx + marker.len()..]);

        assert!(
            block.contains("property <bool> highlighted: state.is-active || state.is-hint;"),
            "KeyButton 应以 is-active/is-hint 推导出 highlighted，作为本课键位的判据"
        );
        assert!(
            block.contains("FingerZoneColors.color-of(state.zone).with-alpha("),
            "非本课键位应通过 with-alpha 降低同一分区色的浓度实现淡化"
        );
        // 淡化后的底色浓度必须显著低于本课键位（否则"淡化"名不副实）。
        let alpha_marker = "FingerZoneColors.color-of(state.zone).with-alpha(";
        let alpha_idx = block.find(alpha_marker).expect("应存在 with-alpha 调用");
        let rest = &block[alpha_idx + alpha_marker.len()..];
        let alpha_text: String = rest.chars().take_while(|&c| c != ')').collect();
        let alpha: f32 = alpha_text
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("无法解析 with-alpha 参数：{alpha_text:?}"));
        assert!(
            (0.05..=0.4).contains(&alpha),
            "非本课键位的底色 alpha={alpha} 不在合理的淡化区间 [0.05, 0.4]：过高则淡化无效，过低则分区色相不可见（Req 2.1）"
        );

        // 本课键位仍使用饱和的分区色。
        assert!(
            block.contains("? FingerZoneColors.color-of(state.zone)"),
            "本课键位应使用饱和的分区色"
        );
        // 标签色也要随之切换，否则淡底上的白字不可读。
        assert!(
            block.contains("color: root.highlighted ? Theme.vivid-foreground : Theme.muted-foreground;"),
            "键帽标签色应随 highlighted 切换，保证淡色底上仍可读"
        );
    }

    /// 校验 `virtual_keyboard.slint`：击键时被按下的键帽要有"按下去"的动画，
    /// 且这个按下态**必须能自动回弹**。
    ///
    /// 三条约束：
    /// 1. 键帽在按下时下沉、压暗、收掉投影——这是"敲击感"的全部来源；
    /// 2. 动画足够短（≤80ms）。这条容易被忽视：把时长调成 300ms 看似"更顺滑"，
    ///    实际会让键帽在下一次击键前还没弹回来，连续打字时糊成一片；
    /// 3. `VirtualKeyboard` 内必须有把 `pressed-key-index` 清回 -1 的计时器。
    ///    没有它，Rust 侧写入下标后键帽会**永久停在按下态**——而 Rust 侧刻意
    ///    不做这件纯视觉的事（见该属性的注释），所以这条是真正的功能约束，
    ///    不只是风格检查。
    #[test]
    fn virtual_keyboard_key_press_animation_is_short_and_self_releasing() {
        let source = read_slint("virtual_keyboard.slint");

        let marker = "component KeyButton inherits Rectangle ";
        let idx = source.find(marker).expect("应包含 KeyButton 组件");
        let block = balanced_brace_block(&source[idx + marker.len()..]);

        assert!(
            block.contains("in property <bool> pressed: false;"),
            "KeyButton 应接收 pressed 状态"
        );
        assert!(
            block.contains("y: root.pressed ? 3px : 0px;"),
            "按下时键帽应下沉（y 偏移）"
        );
        assert!(
            block.contains("background: root.pressed ? root.base-color.darker("),
            "按下时键帽底色应压暗一档"
        );
        assert!(
            block.contains("drop-shadow-blur: root.pressed ? 0px :"),
            "按下时应收掉投影（键帽贴到面板上）"
        );

        // 动画时长：取 KeyButton 内声明的最小 duration，必须足够短。
        let duration_marker = "duration: ";
        let mut shortest = u32::MAX;
        let mut rest = block;
        while let Some(pos) = rest.find(duration_marker) {
            let after = &rest[pos + duration_marker.len()..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(value) = digits.parse::<u32>() {
                shortest = shortest.min(value);
            }
            rest = &after[digits.len()..];
        }
        assert!(shortest != u32::MAX, "KeyButton 内应声明动画时长");
        assert!(
            shortest <= 80,
            "按下动画时长 {shortest}ms 过长：键帽会在下一次击键前还没弹回来，连续打字时糊成一片"
        );

        // 自动回弹计时器。
        assert!(
            source.contains("running: root.pressed-key-index != -1;"),
            "回弹计时器的 running 应由 pressed-key-index 推导（写入即启动、清零即停止）"
        );
        assert!(
            source.contains("root.pressed-key-index = -1;"),
            "回弹计时器必须把 pressed-key-index 清回 -1，否则键帽会永久停在按下态"
        );

        // 每一个 KeyButton 实例都要接上 pressed，不能有键位漏掉动画。
        // 组件声明写作 `component KeyButton inherits Rectangle {`，不含
        // `KeyButton {` 这个子串，因此下面的计数只会数到实例化点。
        let instantiations = source.matches("KeyButton {").count();
        let pressed_bindings = source.matches("pressed: root.pressed-key-index == ").count();
        assert_eq!(
            instantiations, pressed_bindings,
            "有 {instantiations} 处 KeyButton 实例，但只有 {pressed_bindings} 处接上了 pressed——存在漏掉敲击动画的键位"
        );
    }

    /// 校验列表条目的悬停底色**同时**满足两条：与常态底色有可见区分（否则悬停
    /// 等于没反应），且与"未解锁/弱化"的底色有可见区分（否则悬停会被读成禁用）。
    ///
    /// 背景（真实踩过的坑）：shadcn 默认主题里 `accent` 与 `muted` 是**同一个**
    /// 值（都是 zinc-100 `#f4f4f5`）——前者语义是"悬停高亮"，后者是"弱化/禁用"。
    /// 卡片列表此前用 `has-hover ? accent : card` 表达悬停、用 `muted` 表达未解锁，
    /// 于是鼠标指到的那张卡片变成了和"锁定"完全相同的灰色，而旁边那张仍是白色
    /// 的卡片反倒看起来像被点亮了。用户报告的现象是"鼠标在第一行，第二行却亮起"，
    /// 但命中测试完全正确，错的是配色。
    ///
    /// 这类问题无法靠"字符串里有没有写 hover"来发现——必须真的去比较色值，因此
    /// 本检查解析令牌的十六进制值并计算通道差之和。
    #[test]
    fn list_card_hover_color_is_visually_distinct() {
        /// 两个颜色的曼哈顿距离（各通道差的绝对值之和，0-765）。
        /// 阈值取 32：低于此值在浅色区间基本看不出区别。
        const MIN_DISTANCE: u32 = 32;

        let theme = read_slint("theme.slint");
        let hover = parse_hex_color(&theme_color_value(&theme, "card-hover"));
        let card = parse_hex_color(&theme_color_value(&theme, "card"));
        let muted = parse_hex_color(&theme_color_value(&theme, "muted"));

        let from_card = channel_distance(hover, card);
        assert!(
            from_card >= MIN_DISTANCE,
            "card-hover 与 card 的色差只有 {from_card}（阈值 {MIN_DISTANCE}）：悬停时看不出变化"
        );

        let from_muted = channel_distance(hover, muted);
        assert!(
            from_muted >= MIN_DISTANCE,
            "card-hover 与 muted 的色差只有 {from_muted}（阈值 {MIN_DISTANCE}）：\
             悬停会被读成未解锁/禁用的灰底——这正是「鼠标指第一行、第二行却像亮起」的成因"
        );

        // 两个列表都必须用这个令牌表达悬停，而不是退回 accent（= muted）。
        //
        // 判定前先剥掉注释：这两个文件的注释里恰好写着"不能用 Theme.accent"的
        // 原因说明，直接对全文做子串匹配会把解释文字本身当成违规用法。
        for file in ["curriculum_map.slint", "profile_select.slint"] {
            let code = strip_line_comments(&read_slint(file));
            assert!(
                code.contains("Theme.card-hover"),
                "{file} 应使用 Theme.card-hover 作为悬停底色"
            );
            assert!(
                !code.contains("Theme.accent"),
                "{file} 不得用 Theme.accent 表达悬停（它与 Theme.muted 同值）"
            );
        }
    }

    /// 从 `theme.slint` 里取某个颜色令牌的字面量（不含分号与行尾注释）。
    fn theme_color_value(theme_source: &str, token: &str) -> String {
        let marker = format!("out property <color> {token}: ");
        let idx = theme_source
            .find(&marker)
            .unwrap_or_else(|| panic!("theme.slint 应定义颜色令牌 {token}"));
        theme_source[idx + marker.len()..]
            .chars()
            .take_while(|&c| c != ';')
            .collect::<String>()
            .trim()
            .to_string()
    }

    /// 解析 `#rrggbb` 为 (r, g, b)。带 alpha 的 `#rrggbbaa` 只取前三通道。
    fn parse_hex_color(literal: &str) -> (u32, u32, u32) {
        let hex = literal.trim().trim_start_matches('#');
        assert!(
            hex.len() >= 6,
            "颜色字面量 {literal:?} 不是 #rrggbb 形式，无法比较色差"
        );
        let channel = |from: usize| {
            u32::from_str_radix(&hex[from..from + 2], 16)
                .unwrap_or_else(|_| panic!("颜色字面量 {literal:?} 含非十六进制字符"))
        };
        (channel(0), channel(2), channel(4))
    }

    fn channel_distance(a: (u32, u32, u32), b: (u32, u32, u32)) -> u32 {
        a.0.abs_diff(b.0) + a.1.abs_diff(b.1) + a.2.abs_diff(b.2)
    }

    /// 校验 `answer_hint.slint`：「继续下一题」按钮（Req 7.6 要求的用户选择
    /// 入口）的 `min-width`/`min-height` 不小于 44px（Req 7.1）。
    #[test]
    fn answer_hint_continue_button_meets_touch_target_minimum() {
        let source = read_slint("answer_hint.slint");

        let block = source
            .split("continue-button := TouchArea")
            .nth(1)
            .expect("answer_hint.slint 应包含 continue-button := TouchArea 声明");

        let min_widths = extract_px_values(block, "min-width");
        let min_heights = extract_px_values(block, "min-height");
        assert!(
            !min_widths.is_empty() && !min_heights.is_empty(),
            "answer_hint.slint 的「继续下一题」按钮应声明 min-width/min-height 常量"
        );
        assert!(
            min_widths[0] >= MIN_TOUCH_TARGET_PX,
            "answer_hint.slint 「继续下一题」按钮 min-width: {}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）",
            min_widths[0]
        );
        assert!(
            min_heights[0] >= MIN_TOUCH_TARGET_PX,
            "answer_hint.slint 「继续下一题」按钮 min-height: {}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）",
            min_heights[0]
        );
    }

    /// 校验 `result_summary.slint`：结果统计浮层的「继续」按钮
    /// （`dismissed()` 的唯一触发入口）的 `min-width`/`min-height` 不小于
    /// 44px（Req 7.1）。
    #[test]
    fn result_summary_continue_button_meets_touch_target_minimum() {
        let source = read_slint("result_summary.slint");

        let block = source
            .split("continue-button := TouchArea")
            .nth(1)
            .expect("result_summary.slint 应包含 continue-button := TouchArea 声明");

        let min_widths = extract_px_values(block, "min-width");
        let min_heights = extract_px_values(block, "min-height");
        assert!(
            !min_widths.is_empty() && !min_heights.is_empty(),
            "result_summary.slint 的「继续」按钮应声明 min-width/min-height 常量"
        );
        assert!(
            min_widths[0] >= MIN_TOUCH_TARGET_PX,
            "result_summary.slint 「继续」按钮 min-width: {}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）",
            min_widths[0]
        );
        assert!(
            min_heights[0] >= MIN_TOUCH_TARGET_PX,
            "result_summary.slint 「继续」按钮 min-height: {}px 小于最小可点击目标 {MIN_TOUCH_TARGET_PX}px（Req 7.1）",
            min_heights[0]
        );
    }

    /// 汇总校验：对全部 9 个 `.slint` 文件中出现的顶层 `spacing:` 数值
    /// 字面量做统一的下限检查（Req 7.1）。`reward_overlay.slint`/
    /// `result_summary.slint`/`toast.slint` 内的元素间距同样纳入统一的最小
    /// 间距约定校验范围，与设计文档"儿童友好的界面约束前置到组件层"的整体
    /// 一致性要求相符。
    ///
    /// 排除项：Req 7.1 明确约束的是"相邻可点击控件之间的间距"，以下几处
    /// `spacing` 均不属于可点击控件之间的间距，因此从本汇总检查中
    /// 排除（各自的语义已在 `curriculum_map_touch_targets_and_spacing_meet_minimums`/
    /// `practice_view_exit_button_meets_touch_target_minimum` 测试的文档
    /// 注释中说明）：
    /// - `curriculum_map.slint`：单张课程卡片内部标题/副标题/解锁提示
    ///   三行文本的排版间距（非可点击元素之间的间距）。
    /// - `practice_view.slint`：练习文本字符格（`PracticeCharCell`，纯
    ///   文本展示元素，不是 `TouchArea`）之间的间距。
    #[test]
    fn all_slint_files_spacing_values_meet_minimum() {
        const FILES: &[&str] = &[
            "answer_hint.slint",
            "app.slint",
            "curriculum_map.slint",
            "practice_view.slint",
            "profile_select.slint",
            "result_summary.slint",
            "reward_overlay.slint",
            "toast.slint",
            "virtual_keyboard.slint",
        ];

        // (file_name, spacing_px) 对，属于非可点击元素之间的间距，不受
        // Req 7.1 约束，见上方文档说明。
        const NON_TOUCH_TARGET_SPACING_EXCEPTIONS: &[(&str, u32)] = &[
            ("curriculum_map.slint", 4),
            ("practice_view.slint", 4),
            // virtual_keyboard.slint 的 `KeyButton` 未包裹 `TouchArea`、
            // 未暴露任何点击回调（截至任务 18.4 的实现，虚拟键盘仅用于
            // 视觉展示手指分区与键位提示，不是可交互控件），因此按键之间
            // 与按键行之间的 `spacing: 6px` 不属于"相邻可点击控件之间的
            // 间距"，不受 Req 7.1 约束。
            ("virtual_keyboard.slint", 6),
        ];

        for file_name in FILES {
            let source = read_slint(file_name);
            let spacings = extract_px_values(&source, "spacing");
            for value in &spacings {
                let is_known_exception = NON_TOUCH_TARGET_SPACING_EXCEPTIONS
                    .iter()
                    .any(|&(exception_file, exception_value)| {
                        exception_file == *file_name && exception_value == *value
                    });
                if is_known_exception {
                    continue;
                }
                assert!(
                    *value >= MIN_SPACING_PX,
                    "{file_name} 中 spacing: {value}px 小于最小间距 {MIN_SPACING_PX}px（Req 7.1），且不在已知的非可点击元素间距排除列表中"
                );
            }
        }
    }
}
