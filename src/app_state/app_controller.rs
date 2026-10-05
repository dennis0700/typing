//! AppController：全局状态机与事件分发（`app_state.rs` 顶层描述的核心结构）。
//!
//! 本文件仅定义 `AppEvent`/`UiCommand` 枚举与 `AppController` 结构本身的字段
//! 布局及构造函数（任务 16.1）。事件分发逻辑（`AppController::dispatch`）
//! 属于后续任务（16.2），本文件不实现。
//!
//! 设计依据（design.md「Components and Interfaces」-「AppController」）：
//!
//! ```text
//! pub struct AppController {
//!     profile_manager: ProfileManager,
//!     curriculum_state: Option<CurriculumState>,   // 仅当已选择学员档案时存在
//!     practice_state: Option<PracticeState>,        // 仅当练习环节进行中时存在
//!     store: ProfileStore,
//! }
//!
//! pub enum AppEvent {
//!     SelectProfile(ProfileId),
//!     CreateProfile { nickname: String },
//!     SelectLesson(LessonId),
//!     KeyPressed(KeyCode),
//!     CharInput(char),
//!     ExitPractice,
//!     SkipCurrentItem,
//! }
//! ```

// 走本层模块入口 `app_state.rs` 的扁平化再导出路径（`pub use
// curriculum_state::*` 等），而不是直接引用私有子模块路径
// （`crate::app_state::curriculum_state::...`）。两种写法都能编译，但前者才是
// 设计文档规定的对外 API 形态："模块入口文件用私有 mod 声明子模块并通过
// pub use 扁平化导出公共类型"——若层内自己都绕过这层再导出，那些 `pub use`
// 就没有任何消费者，会被编译器报成 unused import。
use crate::app_state::{
    CurriculumState, LessonSelectionError, NavigationTarget, PracticeState, ProfileManager,
};
use crate::domain::{
    self, Curriculum, KeyCode, LessonId, check_error_threshold, generate_lesson_completion_reward,
    generate_practice_text,
};
use crate::storage::{self, LearnerProfile, ProfileId, ProfileStore};

/// 由 UI 层转发给 `AppController::dispatch`（任务 16.2）的用户/系统事件。
///
/// 变体与设计文档 `AppEvent` 定义逐一对应，命名与字段结构不做增删——设计
/// 文档未列出的事件（如 slint `Timer` 触发的周期性刷新）不属于本枚举，
/// 由 UI 桥接层单独处理，不经过 `dispatch`。
#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    /// 学员档案选择界面：选择一个已存在的学员档案（Req 6.4）。
    SelectProfile(ProfileId),
    /// 学员档案选择界面：新建档案并指定昵称（Req 6.3, 6.5, 6.6, 6.7）。
    CreateProfile { nickname: String },
    /// 学员档案选择界面：删除一个学员档案（Req 6.11）。
    ///
    /// 二次确认（Req 6.9, 6.10）由 UI 层负责：确认弹窗中点"取消"根本不会
    /// 派发本事件，因此 `AppController` 收到它即意味着用户已经明确确认过，
    /// 状态机不再重复追问，也不持有任何"待确认"中间状态。
    DeleteProfile(ProfileId),
    /// 学员档案选择界面：把一个学员档案的学习进度重置为初始状态（Req 6.12）。
    /// 二次确认同样由 UI 层负责，理由见 [`AppEvent::DeleteProfile`]。
    ResetProfileProgress(ProfileId),
    /// 学员档案选择界面：修改一个学员档案的昵称（Req 6.13）。
    ///
    /// 重命名是可逆操作（改错了再改回来即可），因此不需要二次确认；但昵称
    /// 仍要过与新建档案完全相同的校验，失败时展示错误提示。
    RenameProfile {
        profile_id: ProfileId,
        nickname: String,
    },
    /// 课程序列界面：返回学员档案选择界面（切换学员/管理档案）。
    ///
    /// 与 `ExitPractice` 一起构成完整的反向导航链：练习环节 -> 课程序列 ->
    /// 档案选择。没有本事件时，一旦选定档案就无法再回到档案选择界面，
    /// Req 6.8-6.14 的档案管理入口在整个会话里只有启动那一刻可达。
    ExitToProfileSelect,
    /// 课程序列界面：选择一个课程进入练习环节（Req 1.6, 1.7）。
    SelectLesson(LessonId),
    /// 虚拟键盘/物理键盘按下某个按键（用于键位提示流程，Req 2.4, 2.5）。
    ///
    /// 当前 UI 桥接层不构造该变体：物理键盘输入走 `CharInput`（Slint
    /// `KeyEvent.text` 本身就是字符，没必要做 char -> KeyCode -> char 的往返
    /// 映射，见 `ui.rs` 模块文档），虚拟键盘目前也不可点击。它仍是设计文档
    /// `AppEvent` 定义的一部分、由 `dispatch` 正常处理并有单测覆盖，因此保留
    /// 而不删除；`cfg_attr` 只压制非测试构建下的 dead_code 提示，一旦相关
    /// 单测被删除，测试构建仍会重新报出未使用。
    #[cfg_attr(not(test), allow(dead_code))]
    KeyPressed(KeyCode),
    /// 练习环节中输入一个字符（字符匹配状态机，Req 3.2, 3.3, 3.6）。
    CharInput(char),
    /// 学员主动退出当前练习环节（Req 7.5）。
    ExitPractice,
    /// 学员选择跳过当前练习题目（连续错误达到阈值后的可选操作，Req 7.6）。
    SkipCurrentItem,
}

/// `AppController::dispatch`（任务 16.2）返回给 UI 桥接层的命令。
///
/// UI 层（`ui.rs`）据此更新 Slint 属性/触发导航，本身不包含任何业务判断。
/// 变体覆盖设计文档中提及的命令类别：页面导航、提示信息展示、以及练习/
/// 课程反馈相关的展示指令。
#[derive(Debug, Clone, PartialEq)]
pub enum UiCommand {
    /// 导航到指定界面（复用 `PracticeState::exit` 已定义的
    /// [`crate::app_state::practice_state::NavigationTarget`]，
    /// 避免重复定义导航目标枚举）。
    NavigateTo(NavigationTarget),
    /// 展示一条提示信息（如未解锁课程的解锁条件说明、保存失败提示等）。
    ShowToast(String),
    /// 展示一条**不与页面内输入框绑定**的全局提示。
    ///
    /// 与 [`UiCommand::ShowToast`] 的区别只在 UI 层的落点：桥接层
    /// （`ui::toast_target_for_page`）会把档案选择界面上的 `ShowToast` 路由到
    /// 昵称输入框旁的内联错误提示区——这对"新建档案昵称非法"是恰当的，但
    /// 档案管理（删除/重置/重命名）的结果反馈并不属于那个输入框，且成功提示
    /// 用红色内联错误样式展示会误导用户。因此档案管理的结果统一走本变体，
    /// 落到各页面通用的全局 Toast 浮层（Req 6.11-6.14 的操作结果反馈）。
    ShowGlobalToast(String),
    /// 展示鼓励性提示，允许学员继续尝试或跳过当前题目（Req 7.2，连续错误
    /// 达到 [`domain::ENCOURAGEMENT_THRESHOLD`] 次触发）。
    ShowEncouragement(String),
    /// 展示正确答案/提示，允许学员选择继续下一题目（Req 7.6，连续错误
    /// 达到 [`domain::SHOW_ANSWER_THRESHOLD`] 次触发）。
    ShowCorrectAnswer(String),
    /// 展示课程完成时的奖励性反馈（动画/徽章/鼓励文案，Req 7.3）。
    ShowRewardFeedback(domain::RewardFeedback),
    /// 展示本次练习环节的结果统计（Req 3.5, 4.1, 4.2, 4.4, 4.5）。
    ShowPracticeResult(storage::PracticeResult),
}

/// 全局状态机：UI 回调的唯一入口，组合 `ProfileManager`、`CurriculumState`、
/// `PracticeState` 与 `ProfileStore`。
///
/// 字段可见性与所有权模型依据设计文档：
/// - `profile_manager`：始终存在，持有已知档案摘要列表与当前会话选中的档案。
/// - `curriculum_state`：`Option`——仅当学员已选择/新建档案后才存在（课程解锁
///   状态依赖该学员的 `Learning_Progress`，未选择档案时没有意义）。
/// - `practice_state`：`Option`——仅当练习环节进行中时存在（进入练习环节时
///   创建，完成或退出练习环节时清空）。
/// - `store`：`ProfileStore` 的持久化句柄，供选择/新建档案后构造
///   `CurriculumState`/落盘使用；`ProfileManager` 内部也持有一份用于自身的
///   档案扫描与保存操作，二者共享同一底层目录但各自持有独立的
///   `ProfileStore` 值（`ProfileStore` 本身仅封装一个路径，`Clone` 成本
///   可忽略，不构成实质性状态重复）。
/// - `curriculum`：应用内置的静态课程序列定义。设计文档的字段列表未显式
///   列出该字段，但 `CurriculumState::new`/`refresh` 需要一份 `Curriculum`
///   才能在选择/新建档案后构造 `CurriculumState`；`AppController` 是唯一
///   合适的持有者（内置课程数据与"当前学员的解锁状态视图"是两个不同维度，
///   前者不随学员切换而变化，因此不适合放进 `Option<CurriculumState>`
///   内部反复重建）。
pub struct AppController {
    profile_manager: ProfileManager,
    curriculum: Curriculum,
    curriculum_state: Option<CurriculumState>,
    practice_state: Option<PracticeState>,
    /// 当前已选择/新建并激活的学员档案完整数据（Req 6.4）。
    ///
    /// 设计文档字段列表未显式列出该字段，但课程/练习相关操作
    /// （`SelectLesson`/`KeyPressed`/`CharInput`/课程解锁重算、成绩落盘）
    /// 需要完整的 `LearnerProfile`（含 `LearningProgress`），而不仅是
    /// `ProfileManager` 用于渲染档案选择界面的摘要列表；因此在
    /// `SelectProfile`/`CreateProfile` 成功后，本字段持有该学员档案的
    /// 完整内存副本，供后续事件读取/修改并驱动落盘。未选择任何档案时为
    /// `None`，与 `curriculum_state`/`practice_state` 的"仅当……时存在"
    /// 约定保持一致。
    current_profile: Option<LearnerProfile>,
    /// 当前练习题目内连续输入错误的次数（Req 7.2, 7.6）。
    ///
    /// 设计文档字段列表未显式列出该字段；`domain::check_error_threshold`
    /// 是无状态的纯函数，需要调用方自行维护"同一题目内连续错误次数"这一
    /// 计数器。之所以将其放在 `AppController` 而非 `PracticeState`/
    /// `TypingMatchState` 内部，是因为它是"是否已提示过鼓励/正确答案"的
    /// UI 反馈节流状态，与字符匹配状态机本身的正确性判定（`error_count`
    /// 统计的是整段练习文本的累计错误数，语义不同）无关。每当学员在当前
    /// 练习题目上正确输入（`MatchOutcome::Correct`）时清零；每当练习环节
    /// 开始（`SelectLesson`/`SkipCurrentItem` 进入下一题）时也清零。
    consecutive_errors: u32,
}

impl AppController {
    /// 使用给定的 `ProfileStore` 与内置课程序列构造 `AppController`。
    ///
    /// - `profile_manager` 通过 `ProfileManager::new(store)` 构造，立即扫描
    ///   `store` 目录加载已知档案摘要列表（Req 6.2）。`store` 的所有权由
    ///   `ProfileManager` 独占持有：`AppController` 自己不需要第二个句柄
    ///   （所有落盘都经由 `profile_manager`），因此这里不再克隆一份存进
    ///   本结构体。
    /// - `curriculum_state`/`practice_state` 初始为 `None`：启动时尚未选择
    ///   任何学员档案，也没有进行中的练习环节。
    pub fn new(store: ProfileStore, curriculum: Curriculum) -> Self {
        let profile_manager = ProfileManager::new(store);
        Self {
            profile_manager,
            curriculum,
            curriculum_state: None,
            practice_state: None,
            current_profile: None,
            consecutive_errors: 0,
        }
    }

    /// 使用 macOS 默认档案存储目录构造 `AppController`
    /// （`~/Library/Application Support/typing/profiles`）。
    ///
    /// 目录解析失败（如无法定位用户主目录）时返回对应的 `io::Error`，
    /// 由调用方（`main.rs`）决定如何向用户提示。
    pub fn new_with_default_store(curriculum: Curriculum) -> std::io::Result<Self> {
        let store = ProfileStore::new_default()?;
        Ok(Self::new(store, curriculum))
    }

    /// 当前已知的学员档案摘要列表（供档案选择界面渲染，Req 6.2）。
    pub fn profile_manager(&self) -> &ProfileManager {
        &self.profile_manager
    }

    /// 当前学员的课程解锁状态视图；未选择任何档案时为 `None`。
    pub fn curriculum_state(&self) -> Option<&CurriculumState> {
        self.curriculum_state.as_ref()
    }

    /// 当前进行中的练习环节状态；没有练习环节进行中时为 `None`。
    pub fn practice_state(&self) -> Option<&PracticeState> {
        self.practice_state.as_ref()
    }

    /// 当前已激活的学员档案完整数据；未选择任何档案时为 `None`（Req 6.4）。
    pub fn current_profile(&self) -> Option<&LearnerProfile> {
        self.current_profile.as_ref()
    }

    /// 应用内置的静态课程序列（只读）。
    pub fn curriculum(&self) -> &Curriculum {
        &self.curriculum
    }

    /// 唯一的状态变更入口（design.md「Components and Interfaces」-
    /// 「AppController」）：接收 UI 事件 → 调用相应子引擎 → 调用领域纯逻辑
    /// 函数 → 更新内部状态 → 返回 `UiCommand` 列表。
    pub fn dispatch(&mut self, event: AppEvent) -> Vec<UiCommand> {
        match event {
            AppEvent::SelectProfile(profile_id) => self.handle_select_profile(profile_id),
            AppEvent::CreateProfile { nickname } => self.handle_create_profile(&nickname),
            AppEvent::DeleteProfile(profile_id) => self.handle_delete_profile(profile_id),
            AppEvent::ResetProfileProgress(profile_id) => {
                self.handle_reset_profile_progress(profile_id)
            }
            AppEvent::RenameProfile {
                profile_id,
                nickname,
            } => self.handle_rename_profile(profile_id, &nickname),
            AppEvent::ExitToProfileSelect => self.handle_exit_to_profile_select(),
            AppEvent::SelectLesson(lesson_id) => self.handle_select_lesson(&lesson_id),
            AppEvent::KeyPressed(key_code) => self.handle_char_input(domain::key_to_char(key_code)),
            AppEvent::CharInput(c) => self.handle_char_input(c),
            AppEvent::ExitPractice => self.handle_exit_practice(),
            AppEvent::SkipCurrentItem => self.handle_skip_current_item(),
        }
    }

    /// 学员档案选择界面：选择一个已存在的学员档案（Req 6.4）。
    ///
    /// 成功后：激活该档案为 `current_profile`，基于其 `progress` 构造
    /// `curriculum_state`，清空任何遗留的练习环节/连续错误计数（选择档案
    /// 意味着开启一个全新的会话上下文），并返回导航到课程序列界面的命令。
    /// 失败时不改变任何状态，返回携带 `ProfileError` 错误信息的
    /// `ShowToast`（`ProfileError` 的 `Display` 实现，Req 6.4 隐含的错误
    /// 反馈路径——例如档案已被删除/损坏）。
    fn handle_select_profile(&mut self, profile_id: ProfileId) -> Vec<UiCommand> {
        match self.profile_manager.select_profile(profile_id) {
            Ok(profile) => self.activate_profile(profile),
            Err(err) => vec![UiCommand::ShowToast(err.to_string())],
        }
    }

    /// 学员档案选择界面：新建档案并指定昵称（Req 6.3, 6.5, 6.6, 6.7）。
    ///
    /// 成功后与 [`Self::handle_select_profile`] 一致地激活新建的档案（新建
    /// 档案的学习进度为初始状态，`compute_lesson_states` 会据此将序列首课
    /// 标记为 `Unlocked`，其余课程为 `Locked`，对应 Req 1.2/6.3）。
    /// 失败时（昵称非空校验/超长/重复/已达上限）返回 `ShowToast`，不改变
    /// 任何状态——`ProfileManager::create_profile` 本身已保证校验失败时不
    /// 修改其内部的已知档案列表，且不获取调用方传入 `nickname` 的所有权，
    /// 调用方（UI 层）持有的原始输入内容不受影响（Req 6.5, 6.7 的"保留
    /// 用户当前输入内容"约束由 UI 层负责，本方法只负责不产生额外副作用）。
    fn handle_create_profile(&mut self, nickname: &str) -> Vec<UiCommand> {
        match self.profile_manager.create_profile(nickname) {
            Ok(profile) => self.activate_profile(profile),
            Err(err) => vec![UiCommand::ShowToast(err.to_string())],
        }
    }

    /// 学员档案选择界面：删除一个学员档案（Req 6.11, 6.14）。
    ///
    /// 收到本事件即代表用户已在 UI 的二次确认弹窗中确认过（Req 6.9）。
    ///
    /// 成功后除了让档案从列表中消失（由 `ProfileManager` 完成，UI 下一次
    /// 刷新即可见），还要**防御性地清理会话状态**：如果被删的恰好是当前
    /// 激活档案，则 `current_profile`/`curriculum_state`/`practice_state`
    /// 必须一并清空，否则会留下一个指向已不存在档案的会话——后续任何一次
    /// 练习完成都会试图把成绩写回一个已被删除的档案文件。
    ///
    /// 按当前的页面路由，档案管理只可能发生在启动时的档案选择界面（选定
    /// 档案后没有返回该界面的入口），因此这一分支实际不会被触发；但它是
    /// 状态机自身的一致性保证，不依赖"UI 恰好不提供某个入口"这一外部前提。
    ///
    /// 失败时（档案不存在/文件删除失败）返回全局提示，不改变任何状态。
    fn handle_delete_profile(&mut self, profile_id: ProfileId) -> Vec<UiCommand> {
        match self.profile_manager.delete_profile(profile_id) {
            Ok(()) => {
                self.clear_session_if_active(profile_id);
                vec![UiCommand::ShowGlobalToast("已删除该学员档案".to_string())]
            }
            Err(err) => vec![UiCommand::ShowGlobalToast(err.to_string())],
        }
    }

    /// 学员档案选择界面：把一个学员档案的学习进度重置为初始状态
    /// （Req 6.12, 6.14）。收到本事件即代表用户已确认（Req 6.9）。
    ///
    /// 若被重置的恰好是当前激活档案，则同步刷新会话内的档案副本与课程解锁
    /// 状态视图——否则内存里仍是重置前的进度，界面会继续按旧的解锁状态渲染，
    /// 并在下一次落盘时把旧进度写回去，等于重置被悄悄撤销。
    fn handle_reset_profile_progress(&mut self, profile_id: ProfileId) -> Vec<UiCommand> {
        match self.profile_manager.reset_progress(profile_id) {
            Ok(profile) => {
                if self
                    .current_profile
                    .as_ref()
                    .is_some_and(|active| active.profile_id == profile_id)
                {
                    let progress_view = to_domain_progress(&profile.progress);
                    self.curriculum_state =
                        Some(CurriculumState::new(self.curriculum.clone(), &progress_view));
                    self.current_profile = Some(profile);
                    self.practice_state = None;
                    self.consecutive_errors = 0;
                }
                vec![UiCommand::ShowGlobalToast(
                    "已重置该学员的学习进度".to_string(),
                )]
            }
            Err(err) => vec![UiCommand::ShowGlobalToast(err.to_string())],
        }
    }

    /// 学员档案选择界面：修改一个学员档案的昵称（Req 6.13, 6.14）。
    ///
    /// 昵称校验在 `ProfileManager::rename_profile` 内完成（复用与新建档案
    /// 相同的规则，重复判定排除自身）。校验失败时不改变任何状态，返回错误
    /// 提示；UI 层保留用户当前输入内容的责任与新建档案一致（Req 6.5）。
    fn handle_rename_profile(&mut self, profile_id: ProfileId, nickname: &str) -> Vec<UiCommand> {
        match self.profile_manager.rename_profile(profile_id, nickname) {
            Ok(profile) => {
                if self
                    .current_profile
                    .as_ref()
                    .is_some_and(|active| active.profile_id == profile_id)
                {
                    // 只同步昵称，进度未被改动，无需重建 curriculum_state。
                    self.current_profile = Some(profile);
                }
                vec![UiCommand::ShowGlobalToast("已更新昵称".to_string())]
            }
            Err(err) => vec![UiCommand::ShowGlobalToast(err.to_string())],
        }
    }

    /// 课程序列界面：返回档案选择界面。
    ///
    /// 语义是"结束当前学员的会话"，因此把激活档案、课程解锁视图、练习环节
    /// 与连续错误计数全部清空——回到档案选择界面后再选一个档案，走的是与
    /// 启动后首次选择完全相同的 `SelectProfile` 路径，不残留上一位学员的
    /// 任何会话状态（避免出现"切换学员后课程解锁状态还是上一个孩子的"）。
    ///
    /// 学习进度不受影响：进度在每次练习完成时就已落盘（Req 5.1），本操作
    /// 只丢弃内存中的会话上下文，不触碰任何档案文件。
    fn handle_exit_to_profile_select(&mut self) -> Vec<UiCommand> {
        self.current_profile = None;
        self.curriculum_state = None;
        self.practice_state = None;
        self.consecutive_errors = 0;

        vec![UiCommand::NavigateTo(NavigationTarget::ProfileSelect)]
    }

    /// 若 `profile_id` 是当前激活档案，则清空整个会话状态（激活档案、课程
    /// 解锁视图、练习环节、连续错误计数）。用于档案被删除后避免留下悬挂的
    /// 会话上下文。
    fn clear_session_if_active(&mut self, profile_id: ProfileId) {
        if self
            .current_profile
            .as_ref()
            .is_some_and(|active| active.profile_id == profile_id)
        {
            self.current_profile = None;
            self.curriculum_state = None;
            self.practice_state = None;
            self.consecutive_errors = 0;
        }
    }

    /// 激活一个刚被选择/新建的学员档案：设为 `current_profile`，基于其
    /// `progress` 构造 `curriculum_state`，清空练习环节与连续错误计数，
    /// 返回导航到课程序列界面的命令（Req 6.4；新建档案场景对应 Req 6.3）。
    fn activate_profile(&mut self, profile: LearnerProfile) -> Vec<UiCommand> {
        let progress_view = to_domain_progress(&profile.progress);
        self.curriculum_state = Some(CurriculumState::new(self.curriculum.clone(), &progress_view));
        self.current_profile = Some(profile);
        self.practice_state = None;
        self.consecutive_errors = 0;

        vec![UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)]
    }

    /// 课程序列界面：选择一个课程进入练习环节（Req 1.6, 1.7）。
    ///
    /// - 未选择任何学员档案（`curriculum_state` 不存在）：视为无效操作，
    ///   返回提示信息，不产生导航命令。
    /// - 目标课程已解锁：调用 `generate_practice_text` 生成练习文本，构造
    ///   `PracticeState` 并激活，清空连续错误计数，返回导航到练习环节界面
    ///   的命令（Req 1.6）。
    /// - 目标课程未解锁：返回 `ShowToast`，附带说明该课程尚未解锁及其解锁
    ///   条件的提示文案（Req 1.7），不产生导航命令。
    /// - 目标课程当前不可用（配置缺失/损坏，Req 1.8）或未找到：返回相应的
    ///   `ShowToast` 提示，不产生导航命令。
    fn handle_select_lesson(&mut self, lesson_id: &LessonId) -> Vec<UiCommand> {
        let Some(curriculum_state) = self.curriculum_state.as_ref() else {
            return vec![UiCommand::ShowToast(
                "请先选择学员档案后再选择课程".to_string(),
            )];
        };

        let lesson = match curriculum_state.select_lesson(lesson_id) {
            Ok(lesson) => lesson.clone(),
            Err(err) => return vec![UiCommand::ShowToast(lesson_selection_error_message(&err))],
        };

        let unlocked_keys = self.unlocked_keys_for_current_profile();
        let rng_seed = self.next_rng_seed();
        // 传入两个键位集合：本课目标键位（单键熟悉课程的取字范围，Req 8.1）
        // 与已解锁键位（词语/句子课程的取字范围，Req 8.2/8.3）。取哪一个由
        // `generate_practice_text` 按 `goal` 决定，本层不做该判断。
        // `lesson.requires_shift` 决定取字符时是否使用 Shift 组合字符
        // （Shift 符号课程，如 `!@#$...`），透传给所有降级分支。
        let text = generate_practice_text(
            lesson.goal,
            &lesson.target_keys,
            &unlocked_keys,
            lesson.requires_shift,
            rng_seed,
        )
        .unwrap_or_default();

        self.practice_state = Some(PracticeState::new(lesson.id, text.chars().collect()));
        self.consecutive_errors = 0;

        vec![UiCommand::NavigateTo(NavigationTarget::PracticeSession)]
    }

    /// 当前已激活学员档案已解锁课程涉及的键位集合（供 `generate_practice_text`
    /// 采样已解锁键位对应字符使用，Req 8.2, 8.3）。
    ///
    /// 取自 `curriculum_state.lesson_states()` 计算出的 `Unlocked` 视图，而非
    /// 直接读取 `profile.progress.unlocked_lesson_ids`——课程序列的首个课程
    /// 恒被 `compute_lesson_states` 判定为 `Unlocked`（Req 1.2, 6.3），但这一
    /// 事实不会被写入 `unlocked_lesson_ids`（该集合只记录"因达成前一课程解锁
    /// 条件而被动解锁"的课程，参见 `CurriculumState::record_practice_result`），
    /// 因此必须以计算后的状态视图为准，否则新建档案后进入首课时会得到空的
    /// 已解锁键位集合。
    ///
    /// 未选择任何档案/课程解锁状态视图不存在时，返回空集合。
    fn unlocked_keys_for_current_profile(&self) -> Vec<KeyCode> {
        let Some(curriculum_state) = self.curriculum_state.as_ref() else {
            return Vec::new();
        };

        self.curriculum
            .lessons
            .iter()
            .zip(curriculum_state.lesson_states().iter())
            .filter(|(_, state)| matches!(state, domain::LessonState::Unlocked))
            .flat_map(|(lesson, _)| lesson.target_keys.iter().copied())
            .collect()
    }

    /// 生成一个用于 `generate_practice_text` 的确定性种子来源。
    ///
    /// 设计文档未规定具体的随机源实现；此处以调用时刻的系统时间（纳秒级）
    /// 作为种子，保证同一进程内连续多次调用大概率产生不同的种子（避免连续
    /// 进入同一课程时反复生成完全相同的练习文本），同时不引入额外的外部
    /// 依赖（如专门的计数器字段）。
    fn next_rng_seed(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }

    /// 练习环节中输入一个字符（Req 3.2, 3.3, 3.6）或按下一个物理/虚拟键盘
    /// 按键（Req 2.4, 2.5，按键先映射为字符再复用同一状态机，键位提示流程
    /// 与字符匹配状态机共用 `apply_input`，design.md「PracticeEngine」）。
    ///
    /// 未选择任何练习环节（`practice_state` 不存在）：视为无效操作，返回
    /// 空命令列表（无可路由的目标状态机，静默忽略而非报错——设计文档未将
    /// "练习环节外的按键"列为错误场景）。
    fn handle_char_input(&mut self, input: char) -> Vec<UiCommand> {
        let Some(practice_state) = self.practice_state.as_mut() else {
            return Vec::new();
        };

        let (new_typing_state, outcome) = domain::apply_input(&practice_state.typing_state, input);
        practice_state.typing_state = new_typing_state;

        match outcome {
            domain::MatchOutcome::AlreadyComplete => Vec::new(),
            domain::MatchOutcome::Correct { .. } => {
                self.consecutive_errors = 0;

                if practice_state.is_complete() {
                    self.handle_practice_completion()
                } else {
                    Vec::new()
                }
            }
            domain::MatchOutcome::Incorrect => {
                self.consecutive_errors += 1;
                let mut commands = Vec::new();
                if let Some(threshold_command) = check_error_threshold(self.consecutive_errors) {
                    commands.push(error_threshold_ui_command(threshold_command));
                }
                commands
            }
        }
    }

    /// 练习环节完成后的收尾流程（Req 2.6, 3.5 完成判定触发；Req 4.1-4.5 成绩
    /// 计算与展示；Req 1.4, 1.5 课程解锁重算；Req 5.1 成绩落盘；Req 7.3 课程
    /// 完成奖励反馈）。
    ///
    /// 前置条件：`self.practice_state` 存在且已完成（由调用方
    /// `handle_char_input` 保证）；`self.current_profile`/`self.curriculum_state`
    /// 存在（练习环节只能在选择档案、进入课程之后才会被创建，`SelectLesson`
    /// 已经保证了这一前提）——若前提缺失（如内部状态被意外破坏），本方法
    /// 保守地跳过成绩落盘/解锁重算，仍会清空 `practice_state` 并返回结果
    /// 展示命令，避免 panic。
    fn handle_practice_completion(&mut self) -> Vec<UiCommand> {
        let Some(practice_state) = self.practice_state.take() else {
            return Vec::new();
        };

        let Some(result) = practice_state.to_practice_result() else {
            return Vec::new();
        };

        let lesson_id = result.lesson_id.clone();
        let mut commands = vec![
            UiCommand::NavigateTo(NavigationTarget::CurriculumSequence),
            UiCommand::ShowPracticeResult(result.clone()),
        ];

        if let (Some(curriculum_state), Some(profile)) =
            (self.curriculum_state.as_mut(), self.current_profile.as_mut())
            && let Err(err) =
                curriculum_state.record_practice_result(profile, result, &mut self.profile_manager)
        {
            commands.push(UiCommand::ShowToast(err.to_string()));
        }

        commands.push(UiCommand::ShowRewardFeedback(
            generate_lesson_completion_reward(&LessonId(lesson_id.0.clone())),
        ));

        commands
    }

    /// 学员主动退出当前练习环节（Req 7.5）。
    ///
    /// 无进行中的练习环节时视为无效操作，返回空命令列表。否则委托给
    /// `PracticeState::exit`（不生成正式 `PracticeResult`，Req 4.3），清空
    /// `practice_state` 与连续错误计数，返回其携带的导航命令。
    fn handle_exit_practice(&mut self) -> Vec<UiCommand> {
        let Some(practice_state) = self.practice_state.take() else {
            return Vec::new();
        };

        self.consecutive_errors = 0;
        let outcome = practice_state.exit();

        vec![UiCommand::NavigateTo(outcome.navigate_to)]
    }

    /// 学员选择跳过当前练习题目（连续错误达到阈值后的可选操作，Req 7.6）。
    ///
    /// design.md 未对本事件的具体行为给出超出 Req 7.2/7.6"允许学员继续
    /// 尝试或跳过该题目"/"允许学员选择继续下一题目"之外的明确规定，且当前
    /// 练习环节的最小单元是"整段练习文本"（`TypingMatchState` 没有"题目"
    /// 边界的概念，design.md 也未定义"题目"与"练习文本"之间的拆分关系）。
    /// 在没有可复用的现有子引擎方法可路由、且不应臆造新业务逻辑的前提下，
    /// 本方法按任务说明中的兜底策略实现：仅重置连续错误计数（允许学员在
    /// "跳过"后重新以未触发阈值提示的状态继续尝试当前练习文本），不改变
    /// `practice_state`/`curriculum_state`，返回空命令列表。
    fn handle_skip_current_item(&mut self) -> Vec<UiCommand> {
        self.consecutive_errors = 0;
        Vec::new()
    }
}

/// 将 `LessonSelectionError` 转换为面向学员/家长的提示文案（Req 1.7, 1.8）。
///
/// 不暴露 Rust 内部错误类型信息，遵循设计文档 Error Handling 一节的通用
/// 原则："业务层负责将错误转换为面向学员/家长的提示文案"。
fn lesson_selection_error_message(
    err: &crate::app_state::curriculum_state::LessonSelectionError,
) -> String {
    match err {
        LessonSelectionError::Locked {
            gating_lesson_unlock_criteria,
            ..
        } => {
            format!(
                "该课程尚未解锁，需先达成前一课程的解锁条件：{}",
                unlock_criteria_description(gating_lesson_unlock_criteria)
            )
        }
        LessonSelectionError::Unavailable { reason, .. } => {
            format!("该课程当前不可用：{reason}")
        }
        LessonSelectionError::NotFound { .. } => "未找到该课程".to_string(),
    }
}

/// 将 `UnlockCriteria` 中已设定的门槛拼接为人类可读的说明文案。
fn unlock_criteria_description(criteria: &domain::UnlockCriteria) -> String {
    let mut parts = Vec::new();
    if let Some(min_accuracy) = criteria.min_accuracy {
        parts.push(format!("正确率不低于 {min_accuracy:.1}%"));
    }
    if let Some(max_duration_secs) = criteria.max_duration_secs {
        parts.push(format!("用时不超过 {max_duration_secs} 秒"));
    }
    if let Some(min_attempts) = criteria.min_attempts {
        parts.push(format!("练习次数不少于 {min_attempts} 次"));
    }

    if parts.is_empty() {
        "无附加条件".to_string()
    } else {
        parts.join("，")
    }
}

/// 将 `domain::ErrorThresholdCommand` 转换为携带具体提示文案的 `UiCommand`
/// （Req 7.2, 7.6）。
fn error_threshold_ui_command(command: domain::ErrorThresholdCommand) -> UiCommand {
    match command {
        domain::ErrorThresholdCommand::ShowEncouragement => {
            UiCommand::ShowEncouragement(domain::ENCOURAGEMENT_POOL[0].to_string())
        }
        domain::ErrorThresholdCommand::ShowCorrectAnswer => {
            UiCommand::ShowCorrectAnswer("再看看正确答案，然后继续下一题吧！".to_string())
        }
    }
}

/// 将持久化层的 `storage::LearningProgress` 转换为领域层的
/// `domain::Learning_Progress`（供 `CurriculumState::new`/`refresh` 使用；
/// 复用与 `curriculum_state.rs` 内部相同的转换逻辑约定——两层同构但类型
/// 不同，需要显式转换，参见 `curriculum_state.rs` 的 `to_domain_progress`）。
fn to_domain_progress(progress: &storage::LearningProgress) -> domain::Learning_Progress {
    domain::Learning_Progress {
        lesson_records: progress
            .lesson_records
            .iter()
            .map(|(id, record)| {
                let latest = record
                    .history
                    .last()
                    .cloned()
                    .unwrap_or_else(|| storage::PracticeResult {
                        lesson_id: id.clone(),
                        accuracy: record.best_accuracy,
                        wpm: record.best_wpm,
                        error_count: 0,
                        duration_ms: 0,
                        score: record.best_score,
                        completed_at: record.achieved_at,
                    });
                (
                    LessonId(id.0.clone()),
                    domain::PracticeResult {
                        lesson_id: LessonId(latest.lesson_id.0.clone()),
                        accuracy: latest.accuracy,
                        wpm: latest.wpm,
                        error_count: latest.error_count,
                        duration_ms: latest.duration_ms,
                        score: latest.score,
                    },
                )
            })
            .collect(),
        unlocked_lesson_ids: progress
            .unlocked_lesson_ids
            .iter()
            .map(|id| LessonId(id.0.clone()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在系统临时目录下创建一个唯一的子目录用于测试，返回其路径。
    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "typing-app-controller-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir for test");
        dir
    }

    fn empty_curriculum() -> Curriculum {
        Curriculum { lessons: Vec::new() }
    }

    #[test]
    fn new_constructs_with_no_active_profile_or_practice_session() {
        let dir = unique_temp_dir("initial-state");
        let store = ProfileStore::new(&dir);

        let controller = AppController::new(store, empty_curriculum());

        // 启动时没有选择任何学员档案，课程解锁状态视图不存在。
        assert!(controller.curriculum_state().is_none());
        // 启动时没有进行中的练习环节。
        assert!(controller.practice_state().is_none());
        // 空目录下没有任何已知档案摘要。
        assert!(controller.profile_manager().list_profiles().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_discovers_profiles_already_saved_in_store_dir() {
        let dir = unique_temp_dir("discovers-existing-profiles");
        let store = ProfileStore::new(&dir);

        // 先用一个独立的 ProfileManager 在同一目录下创建一个档案，模拟
        // "应用重启前已存在档案"的场景。
        let mut seed_manager = ProfileManager::new(store.clone());
        seed_manager
            .create_profile("小明")
            .expect("seed profile creation should succeed");

        let controller = AppController::new(ProfileStore::new(&dir), empty_curriculum());

        assert_eq!(controller.profile_manager().list_profiles().len(), 1);
        assert_eq!(
            controller.profile_manager().list_profiles()[0].nickname,
            "小明"
        );
        // 即便已存在档案，启动时仍不会自动选择/激活任何档案。
        assert!(controller.curriculum_state().is_none());
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn curriculum_accessor_returns_the_constructed_builtin_curriculum() {
        use crate::domain::{KeyCode, Lesson, LessonGoal, UnlockCriteria};

        let dir = unique_temp_dir("curriculum-accessor");
        let store = ProfileStore::new(&dir);
        let curriculum = Curriculum {
            lessons: vec![Lesson {
                id: LessonId("lesson-1".to_string()),
                title: "课程一".to_string(),
                goal: LessonGoal::SingleKey,
                target_keys: vec![KeyCode::A],
                requires_shift: false,
                unlock_criteria: UnlockCriteria {
                    min_accuracy: Some(90.0),
                    max_duration_secs: None,
                    min_attempts: None,
                },
            }],
        };

        let controller = AppController::new(store, curriculum);

        assert_eq!(controller.curriculum().lessons.len(), 1);
        assert_eq!(
            controller.curriculum().lessons[0].id,
            LessonId("lesson-1".to_string())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- dispatch tests (task 16.2 / 16.3) ----
    //
    // 覆盖每类 AppEvent 至少一个正常路径与一个异常路径。

    use crate::domain::{KeyCode, Lesson, LessonGoal, UnlockCriteria};

    fn single_key_lesson(id: &str, min_accuracy: Option<f32>) -> Lesson {
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

    fn two_lesson_curriculum() -> Curriculum {
        Curriculum {
            lessons: vec![
                single_key_lesson("lesson-1", Some(90.0)),
                single_key_lesson("lesson-2", Some(90.0)),
            ],
        }
    }

    fn controller_with_temp_store(label: &str, curriculum: Curriculum) -> (std::path::PathBuf, AppController) {
        let dir = unique_temp_dir(label);
        let store = ProfileStore::new(&dir);
        let controller = AppController::new(store, curriculum);
        (dir, controller)
    }

    // --- SelectProfile ---

    #[test]
    fn dispatch_select_profile_success_navigates_to_curriculum_sequence() {
        let (dir, mut controller) = controller_with_temp_store("select-profile-ok", two_lesson_curriculum());

        // 先通过 CreateProfile 建立一个可供 SelectProfile 选择的档案。
        let create_commands = controller.dispatch(AppEvent::CreateProfile {
            nickname: "小明".to_string(),
        });
        assert_eq!(
            create_commands,
            vec![UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)]
        );
        let profile_id = controller
            .current_profile()
            .expect("profile should be active after creation")
            .profile_id;

        let commands = controller.dispatch(AppEvent::SelectProfile(profile_id));

        assert_eq!(
            commands,
            vec![UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)]
        );
        assert_eq!(
            controller.current_profile().map(|p| p.profile_id),
            Some(profile_id)
        );
        assert!(controller.curriculum_state().is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_select_profile_unknown_id_returns_toast_and_no_state_change() {
        let (dir, mut controller) = controller_with_temp_store("select-profile-err", two_lesson_curriculum());

        let commands = controller.dispatch(AppEvent::SelectProfile(ProfileId::new()));

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowToast(_)));
        assert!(controller.current_profile().is_none());
        assert!(controller.curriculum_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- CreateProfile ---

    #[test]
    fn dispatch_create_profile_success_activates_profile_and_navigates() {
        let (dir, mut controller) = controller_with_temp_store("create-profile-ok", two_lesson_curriculum());

        let commands = controller.dispatch(AppEvent::CreateProfile {
            nickname: "小红".to_string(),
        });

        assert_eq!(
            commands,
            vec![UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)]
        );
        let profile = controller
            .current_profile()
            .expect("newly created profile should be active");
        assert_eq!(profile.nickname, "小红");
        // 新建档案的初始状态：序列首课 Unlocked，其余 Locked（Req 1.2, 6.3）。
        let states = controller
            .curriculum_state()
            .expect("curriculum_state should exist after activation")
            .lesson_states();
        assert_eq!(states[0], crate::domain::LessonState::Unlocked);
        assert_eq!(states[1], crate::domain::LessonState::Locked);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_create_profile_invalid_nickname_returns_toast_and_no_state_change() {
        let (dir, mut controller) = controller_with_temp_store("create-profile-err", two_lesson_curriculum());

        let commands = controller.dispatch(AppEvent::CreateProfile {
            nickname: "".to_string(),
        });

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowToast(_)));
        assert!(controller.current_profile().is_none());
        assert!(controller.curriculum_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- DeleteProfile / ResetProfileProgress / RenameProfile（Req 6.11-6.14） ---

    /// 新建一个档案，然后把控制器恢复成"未激活任何档案"的档案选择界面状态
    /// （档案管理正是在这个状态下发生的）。返回该档案 id。
    fn created_profile_without_active_session(controller: &mut AppController) -> ProfileId {
        let commands = controller.dispatch(AppEvent::CreateProfile {
            nickname: "小明".to_string(),
        });
        assert_eq!(
            commands,
            vec![UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)]
        );
        let profile_id = controller
            .current_profile()
            .expect("create should activate the profile")
            .profile_id;

        // 模拟"回到档案选择界面"：清掉激活状态，只留下已知档案列表。
        controller.clear_session_if_active(profile_id);
        assert!(controller.current_profile().is_none());

        profile_id
    }

    #[test]
    fn dispatch_delete_profile_removes_it_and_reports_success() {
        let (dir, mut controller) =
            controller_with_temp_store("delete-profile-ok", two_lesson_curriculum());
        let profile_id = created_profile_without_active_session(&mut controller);
        assert_eq!(controller.profile_manager().list_profiles().len(), 1);

        let commands = controller.dispatch(AppEvent::DeleteProfile(profile_id));

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowGlobalToast(_)));
        assert!(controller.profile_manager().list_profiles().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_delete_profile_unknown_id_reports_error_and_changes_nothing() {
        let (dir, mut controller) =
            controller_with_temp_store("delete-profile-err", two_lesson_curriculum());
        created_profile_without_active_session(&mut controller);

        let commands = controller.dispatch(AppEvent::DeleteProfile(ProfileId::new()));

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowGlobalToast(_)));
        assert_eq!(controller.profile_manager().list_profiles().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 删除的恰好是当前激活档案时，会话状态必须一并清空——否则会留下一个
    /// 指向已不存在档案文件的会话，后续练习完成会试图往已删除的档案写回成绩。
    #[test]
    fn dispatch_delete_active_profile_clears_the_whole_session() {
        let (dir, mut controller) =
            controller_with_temp_store("delete-active-profile", two_lesson_curriculum());

        let profile_id = controller
            .dispatch(AppEvent::CreateProfile {
                nickname: "小明".to_string(),
            })
            .first()
            .map(|_| {
                controller
                    .current_profile()
                    .expect("create should activate")
                    .profile_id
            })
            .expect("create should return a navigation command");
        // 进入练习环节，构造出"档案激活 + 练习进行中"的完整会话。
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));
        assert!(controller.practice_state().is_some());

        controller.dispatch(AppEvent::DeleteProfile(profile_id));

        assert!(controller.current_profile().is_none());
        assert!(controller.curriculum_state().is_none());
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_reset_profile_progress_restores_initial_progress() {
        let (dir, mut controller) =
            controller_with_temp_store("reset-progress-ok", two_lesson_curriculum());
        let profile_id = created_profile_without_active_session(&mut controller);

        // 先做出一些进度：完成 lesson-1 会解锁 lesson-2 并写入成绩记录。
        controller.dispatch(AppEvent::SelectProfile(profile_id));
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));
        let text: Vec<char> = controller.practice_state().unwrap().text().to_vec();
        for ch in text {
            controller.dispatch(AppEvent::CharInput(ch));
        }
        let progressed = controller
            .profile_manager()
            .select_profile(profile_id)
            .expect("profile should be readable");
        assert!(
            !progressed.progress.lesson_records.is_empty(),
            "完成一次练习后应存在成绩记录，否则本测试无法验证重置效果"
        );

        let commands = controller.dispatch(AppEvent::ResetProfileProgress(profile_id));

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowGlobalToast(_)));
        let after = controller
            .profile_manager()
            .select_profile(profile_id)
            .expect("profile should still exist");
        assert!(after.progress.lesson_records.is_empty());
        assert!(after.progress.unlocked_lesson_ids.is_empty());
        assert_eq!(after.nickname, progressed.nickname, "昵称应保留");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 重置的是当前激活档案时，内存中的会话副本与课程解锁视图必须同步刷新，
    /// 否则界面继续按旧解锁状态渲染，且下一次落盘会把旧进度写回去。
    #[test]
    fn dispatch_reset_active_profile_refreshes_session_state() {
        let (dir, mut controller) =
            controller_with_temp_store("reset-active-profile", two_lesson_curriculum());
        let profile_id = created_profile_without_active_session(&mut controller);

        controller.dispatch(AppEvent::SelectProfile(profile_id));
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));
        let text: Vec<char> = controller.practice_state().unwrap().text().to_vec();
        for ch in text {
            controller.dispatch(AppEvent::CharInput(ch));
        }
        // 完成 lesson-1 后 lesson-2 应已解锁。
        assert_eq!(
            controller.curriculum_state().unwrap().lesson_states()[1],
            crate::domain::LessonState::Unlocked
        );

        controller.dispatch(AppEvent::ResetProfileProgress(profile_id));

        let states = controller
            .curriculum_state()
            .expect("激活档案被重置后课程视图应仍存在（而不是被清空）")
            .lesson_states();
        assert_eq!(states[0], crate::domain::LessonState::Unlocked);
        assert_eq!(
            states[1],
            crate::domain::LessonState::Locked,
            "重置后 lesson-2 应回到未解锁状态"
        );
        assert!(
            controller
                .current_profile()
                .unwrap()
                .progress
                .lesson_records
                .is_empty(),
            "内存中的激活档案副本也应是重置后的进度"
        );
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_rename_profile_updates_nickname_only() {
        let (dir, mut controller) =
            controller_with_temp_store("rename-profile-ok", two_lesson_curriculum());
        let profile_id = created_profile_without_active_session(&mut controller);

        let commands = controller.dispatch(AppEvent::RenameProfile {
            profile_id,
            nickname: "小明明".to_string(),
        });

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowGlobalToast(_)));
        assert_eq!(
            controller.profile_manager().list_profiles()[0].nickname,
            "小明明"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_rename_profile_invalid_nickname_reports_error_and_keeps_old_name() {
        let (dir, mut controller) =
            controller_with_temp_store("rename-profile-err", two_lesson_curriculum());
        let profile_id = created_profile_without_active_session(&mut controller);

        let commands = controller.dispatch(AppEvent::RenameProfile {
            profile_id,
            nickname: "   ".to_string(),
        });

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowGlobalToast(_)));
        assert_eq!(controller.profile_manager().list_profiles()[0].nickname, "小明");

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- ExitToProfileSelect ---

    /// 返回档案选择界面必须结束当前学员会话：否则切换学员后，课程解锁状态
    /// 仍是上一个孩子的，甚至会把上一位学员的练习进度写到新选的档案上。
    #[test]
    fn dispatch_exit_to_profile_select_clears_session_and_navigates() {
        let (dir, mut controller) =
            controller_with_temp_store("exit-to-profile-select", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小明".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));
        assert!(controller.current_profile().is_some());
        assert!(controller.practice_state().is_some());

        let commands = controller.dispatch(AppEvent::ExitToProfileSelect);

        assert_eq!(
            commands,
            vec![UiCommand::NavigateTo(NavigationTarget::ProfileSelect)]
        );
        assert!(controller.current_profile().is_none());
        assert!(controller.curriculum_state().is_none());
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 返回档案选择界面只丢弃内存中的会话上下文，不得影响已落盘的学习进度
    /// （进度在每次练习完成时就已保存，Req 5.1）。
    #[test]
    fn dispatch_exit_to_profile_select_keeps_persisted_progress() {
        let (dir, mut controller) =
            controller_with_temp_store("exit-keeps-progress", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小明".to_string(),
        });
        let profile_id = controller.current_profile().unwrap().profile_id;
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));
        let text: Vec<char> = controller.practice_state().unwrap().text().to_vec();
        for ch in text {
            controller.dispatch(AppEvent::CharInput(ch));
        }
        let before = controller
            .profile_manager()
            .select_profile(profile_id)
            .expect("profile should exist");
        assert!(!before.progress.lesson_records.is_empty());

        controller.dispatch(AppEvent::ExitToProfileSelect);

        // 档案仍在列表中，且磁盘上的进度一字未改。
        assert_eq!(controller.profile_manager().list_profiles().len(), 1);
        assert_eq!(
            controller
                .profile_manager()
                .select_profile(profile_id)
                .expect("profile should still exist"),
            before
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- SelectLesson ---

    #[test]
    fn dispatch_select_lesson_unlocked_lesson_creates_practice_state_and_navigates() {
        let (dir, mut controller) = controller_with_temp_store("select-lesson-ok", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小刚".to_string(),
        });

        let commands = controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        assert_eq!(
            commands,
            vec![UiCommand::NavigateTo(NavigationTarget::PracticeSession)]
        );
        let practice_state = controller
            .practice_state()
            .expect("practice_state should be created for an unlocked lesson");
        assert_eq!(practice_state.lesson_id, LessonId("lesson-1".to_string()));
        assert!(!practice_state.text().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_select_lesson_locked_lesson_returns_toast_and_no_practice_state() {
        let (dir, mut controller) = controller_with_temp_store("select-lesson-locked", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小美".to_string(),
        });

        let commands = controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-2".to_string())));

        assert_eq!(commands.len(), 1);
        match &commands[0] {
            UiCommand::ShowToast(message) => assert!(message.contains("解锁")),
            other => panic!("expected ShowToast, got {other:?}"),
        }
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_select_lesson_without_active_profile_returns_toast() {
        let (dir, mut controller) = controller_with_temp_store("select-lesson-no-profile", two_lesson_curriculum());

        let commands = controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowToast(_)));
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- CharInput / KeyPressed ---

    #[test]
    fn dispatch_char_input_correct_advances_cursor() {
        let (dir, mut controller) = controller_with_temp_store("char-input-ok", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小李".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        let first_char = controller
            .practice_state()
            .expect("practice_state should exist")
            .text()[0];

        let commands = controller.dispatch(AppEvent::CharInput(first_char));

        // 单字符正确输入通常不会立即完成整段练习文本（长度 >= 20），因此
        // 不产生任何命令，仅推进内部状态。
        assert!(commands.is_empty());
        assert_eq!(
            controller
                .practice_state()
                .expect("practice_state should still exist")
                .cursor(),
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_char_input_incorrect_increments_error_count_without_advancing() {
        let (dir, mut controller) = controller_with_temp_store("char-input-err", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小刘".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        let first_char = controller
            .practice_state()
            .expect("practice_state should exist")
            .text()[0];
        let wrong_char = if first_char == 'z' { 'y' } else { 'z' };
        assert_ne!(wrong_char, first_char);

        let commands = controller.dispatch(AppEvent::CharInput(wrong_char));

        // 单次错误输入（未达到连续错误阈值 3 次）不产生任何提示命令。
        assert!(commands.is_empty());
        let practice_state = controller
            .practice_state()
            .expect("practice_state should still exist");
        assert_eq!(practice_state.cursor(), 0);
        assert_eq!(practice_state.typing_state.error_count, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_char_input_three_consecutive_errors_triggers_encouragement() {
        let (dir, mut controller) = controller_with_temp_store("char-input-encourage", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小周".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        let first_char = controller.practice_state().unwrap().text()[0];
        let wrong_char = if first_char == 'z' { 'y' } else { 'z' };

        controller.dispatch(AppEvent::CharInput(wrong_char));
        controller.dispatch(AppEvent::CharInput(wrong_char));
        let commands = controller.dispatch(AppEvent::CharInput(wrong_char));

        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], UiCommand::ShowEncouragement(_)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_char_input_without_practice_state_is_a_noop() {
        let (dir, mut controller) = controller_with_temp_store("char-input-no-session", two_lesson_curriculum());

        let commands = controller.dispatch(AppEvent::CharInput('a'));

        assert!(commands.is_empty());
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_key_pressed_routes_through_same_state_machine_as_char_input() {
        let (dir, mut controller) = controller_with_temp_store("key-pressed-ok", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小陈".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        // lesson-1 的目标键位为 KeyCode::A，对应字符 'a'；由于单键练习文本
        // 仅从 target_keys 采样，第一个字符必为 'a'。
        let commands = controller.dispatch(AppEvent::KeyPressed(KeyCode::A));

        assert!(commands.is_empty());
        assert_eq!(
            controller.practice_state().expect("should exist").cursor(),
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- CharInput 完成整段练习文本（触发 handle_practice_completion） ---

    #[test]
    fn dispatch_char_input_completing_practice_records_result_and_unlocks_next_lesson() {
        let (dir, mut controller) = controller_with_temp_store("char-input-complete", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小吴".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        let text: Vec<char> = controller.practice_state().unwrap().text().to_vec();
        let mut commands = Vec::new();
        for &c in &text {
            commands = controller.dispatch(AppEvent::CharInput(c));
        }

        // 完成整段练习文本后应清空 practice_state，并返回结果/导航相关命令。
        assert!(controller.practice_state().is_none());
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, UiCommand::ShowPracticeResult(_)))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, UiCommand::ShowRewardFeedback(_)))
        );

        // 全部正确输入 -> 100% 正确率，满足 lesson-1 的 90% 解锁门槛，
        // lesson-2 应变为 Unlocked。
        let states = controller.curriculum_state().unwrap().lesson_states();
        assert_eq!(states[1], crate::domain::LessonState::Unlocked);

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- ExitPractice ---

    #[test]
    fn dispatch_exit_practice_mid_session_returns_navigation_and_clears_state() {
        let (dir, mut controller) = controller_with_temp_store("exit-practice-ok", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小赵".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        let commands = controller.dispatch(AppEvent::ExitPractice);

        assert_eq!(
            commands,
            vec![UiCommand::NavigateTo(NavigationTarget::CurriculumSequence)]
        );
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_exit_practice_without_active_session_is_a_noop() {
        let (dir, mut controller) = controller_with_temp_store("exit-practice-noop", two_lesson_curriculum());

        let commands = controller.dispatch(AppEvent::ExitPractice);

        assert!(commands.is_empty());
        assert!(controller.practice_state().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- SkipCurrentItem ---

    #[test]
    fn dispatch_skip_current_item_resets_consecutive_error_counter() {
        let (dir, mut controller) = controller_with_temp_store("skip-item-ok", two_lesson_curriculum());
        controller.dispatch(AppEvent::CreateProfile {
            nickname: "小孙".to_string(),
        });
        controller.dispatch(AppEvent::SelectLesson(LessonId("lesson-1".to_string())));

        let first_char = controller.practice_state().unwrap().text()[0];
        let wrong_char = if first_char == 'z' { 'y' } else { 'z' };
        controller.dispatch(AppEvent::CharInput(wrong_char));
        controller.dispatch(AppEvent::CharInput(wrong_char));

        let skip_commands = controller.dispatch(AppEvent::SkipCurrentItem);
        assert!(skip_commands.is_empty());

        // 计数器被重置：再次输入两次错误不应立即触发鼓励提示（需要重新累计到 3 次）。
        let after_skip = controller.dispatch(AppEvent::CharInput(wrong_char));
        assert!(after_skip.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispatch_skip_current_item_without_active_session_is_a_noop() {
        let (dir, mut controller) = controller_with_temp_store("skip-item-noop", two_lesson_curriculum());

        let commands = controller.dispatch(AppEvent::SkipCurrentItem);

        assert!(commands.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
