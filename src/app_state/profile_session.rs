//! ProfileManager：当前登录学员会话状态。

use std::fmt;

use crate::storage::{LearnerProfile, LearningProgress, ProfileId, ProfileLoadError, ProfileStore};

/// 设备本地最多支持的学员档案数量（Req 6.1）。
pub const MAX_PROFILES: usize = 10;

/// 昵称校验失败的具体原因。
///
/// 对应设计文档 `ProfileError` 中与昵称校验相关的分支
/// （`NicknameEmpty`/`NicknameTooLong`/`NicknameDuplicate`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NicknameError {
    /// 去除首尾空白后为空。
    Empty,
    /// 去除首尾空白后字符数超过 20（按字符数计，非字节数）。
    TooLong,
    /// 去除首尾空白后与 `existing_nicknames` 中某个昵称完全相同。
    Duplicate,
}

/// 纯函数：校验新建学员档案时输入的昵称。
///
/// 校验规则（按顺序判定）：
/// 1. 去除首尾空白后为空 -> `Err(NicknameError::Empty)`
/// 2. 去除首尾空白后字符数（`chars().count()`，正确处理中文等多字节 UTF-8 字符）超过 20
///    -> `Err(NicknameError::TooLong)`
/// 3. 去除首尾空白后与 `existing_nicknames` 中任一昵称完全相同（大小写敏感）
///    -> `Err(NicknameError::Duplicate)`
/// 4. 否则校验通过，返回 `Ok(())`
///
/// 本函数不对输入做任何修改（不裁剪、不落盘），裁剪仅用于校验判断本身；
/// 是否将裁剪后的值作为最终存储的昵称由调用方（`ProfileManager::create_profile`）决定。
///
/// **Validates: Requirements 6.5, 6.7**
pub fn validate_nickname(
    nickname: &str,
    existing_nicknames: &[String],
) -> Result<(), NicknameError> {
    let trimmed = nickname.trim();

    if trimmed.is_empty() {
        return Err(NicknameError::Empty);
    }

    if trimmed.chars().count() > 20 {
        return Err(NicknameError::TooLong);
    }

    if existing_nicknames
        .iter()
        .any(|existing| existing.trim() == trimmed)
    {
        return Err(NicknameError::Duplicate);
    }

    Ok(())
}

/// 学员档案选择界面所需的最小信息（Req 6.2）。
///
/// 启动时通过 `ProfileManager::list_profiles` 获取，避免为渲染档案列表而
/// 加载每个档案的完整学习进度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSummary {
    pub profile_id: ProfileId,
    pub nickname: String,
}

/// `ProfileManager` 操作失败的错误类型。
///
/// 覆盖设计文档 `ProfileError` 的全部分支：昵称校验失败（包装
/// `NicknameError`）、档案数量上限、档案不存在/损坏、以及持久化 I/O 失败。
#[derive(Debug)]
pub enum ProfileError {
    /// 昵称校验未通过，具体原因见 `NicknameError`（Req 6.5, 6.7）。
    InvalidNickname(NicknameError),
    /// 已达到 10 个档案上限，拒绝创建（Req 6.6）。
    ProfileLimitReached,
    /// 指定的档案标识不在当前已知档案列表中。
    NotFound,
    /// 档案数据加载失败（文件损坏/无法解析），不覆盖或删除原始文件（Req 5.4）。
    LoadFailed(ProfileLoadError),
    /// 档案数据保存失败（磁盘空间不足/权限拒绝等），调用方内存状态不受影响（Req 5.5）。
    SaveFailed(crate::storage::ProfileSaveError),
    /// 档案文件删除失败（文件已不存在/权限拒绝等），磁盘数据保持原样（Req 6.14）。
    DeleteFailed(crate::storage::ProfileDeleteError),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::InvalidNickname(err) => match err {
                NicknameError::Empty => write!(f, "昵称不能为空"),
                NicknameError::TooLong => write!(f, "昵称不能超过 20 个字符"),
                NicknameError::Duplicate => write!(f, "该昵称已被使用"),
            },
            ProfileError::ProfileLimitReached => write!(f, "已达到档案数量上限（10 个）"),
            ProfileError::NotFound => write!(f, "未找到指定的学员档案"),
            ProfileError::LoadFailed(err) => write!(f, "档案数据无法读取: {err}"),
            ProfileError::SaveFailed(err) => write!(f, "档案保存失败: {err}"),
            ProfileError::DeleteFailed(err) => write!(f, "档案删除失败: {err}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl From<NicknameError> for ProfileError {
    fn from(err: NicknameError) -> Self {
        ProfileError::InvalidNickname(err)
    }
}

impl From<ProfileLoadError> for ProfileError {
    fn from(err: ProfileLoadError) -> Self {
        ProfileError::LoadFailed(err)
    }
}

impl From<crate::storage::ProfileSaveError> for ProfileError {
    fn from(err: crate::storage::ProfileSaveError) -> Self {
        ProfileError::SaveFailed(err)
    }
}

impl From<crate::storage::ProfileDeleteError> for ProfileError {
    fn from(err: crate::storage::ProfileDeleteError) -> Self {
        ProfileError::DeleteFailed(err)
    }
}

/// 学员档案会话管理：持有已知档案列表（用于档案选择界面），负责新建档案、
/// 选择档案时的加载，以及昵称/数量上限校验。
///
/// 档案的完整数据（`LearnerProfile`，含 `LearningProgress`）不常驻内存——
/// `profiles` 仅保存渲染档案选择界面所需的最小摘要（Req 6.2），
/// 具体某个档案的完整数据在 `select_profile` 时按需从 `ProfileStore` 加载。
pub struct ProfileManager {
    store: ProfileStore,
    profiles: Vec<ProfileSummary>,
}

impl ProfileManager {
    /// 使用给定的 `ProfileStore` 构造 `ProfileManager`，并立即扫描存储目录
    /// 加载已知档案列表（启动时恢复档案选择界面所需数据，Req 6.2）。
    ///
    /// 目录不存在（如首次启动）视为空档案列表，不作为错误处理。
    /// 扫描过程中遇到无法解析的档案文件会被跳过（不计入列表，也不删除该
    /// 文件），保证损坏档案不会阻塞其余档案的正常使用（呼应 Req 5.4 的
    /// "不覆盖或删除原始数据"原则）。
    pub fn new(store: ProfileStore) -> Self {
        let profiles = Self::scan_profiles(&store);
        Self { store, profiles }
    }

    /// 扫描 `store` 基础目录下的全部 `*.json` 档案文件，尝试逐一加载并提取
    /// 摘要信息。无法解析的文件被静默跳过。
    fn scan_profiles(store: &ProfileStore) -> Vec<ProfileSummary> {
        let entries = match std::fs::read_dir(store.base_dir()) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        let mut summaries = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(uuid) = uuid::Uuid::parse_str(stem) else {
                continue;
            };
            let profile_id = ProfileId(uuid);

            if let Ok(profile) = store.load(profile_id) {
                summaries.push(ProfileSummary {
                    profile_id: profile.profile_id,
                    nickname: profile.nickname,
                });
            }
        }

        summaries
    }

    /// 返回当前已知的全部档案摘要（供档案选择界面列出所有档案，Req 6.2）。
    pub fn list_profiles(&self) -> &[ProfileSummary] {
        &self.profiles
    }

    /// 新建一个学员档案。
    ///
    /// 校验顺序：
    /// 1. 昵称校验（`validate_nickname`，对照当前已知昵称集合）——失败返回
    ///    `ProfileError::InvalidNickname`（Req 6.5, 6.7）。
    /// 2. 档案数量上限检查：已有 `MAX_PROFILES`（10）个档案时拒绝创建，
    ///    返回 `ProfileError::ProfileLimitReached`（Req 6.1, 6.6）。
    ///
    /// 校验通过后创建一个 `LearningProgress` 为初始状态（空记录、无解锁课程）
    /// 的新档案（Req 6.3），通过 `ProfileStore::save` 落盘，并加入内存中的
    /// 档案列表。
    ///
    /// 保存失败时返回 `ProfileError::SaveFailed`，不将新档案加入内存列表
    /// （避免出现"列表中存在但磁盘上没有"的不一致状态）。
    pub fn create_profile(&mut self, nickname: &str) -> Result<LearnerProfile, ProfileError> {
        let existing_nicknames: Vec<String> = self
            .profiles
            .iter()
            .map(|summary| summary.nickname.clone())
            .collect();

        validate_nickname(nickname, &existing_nicknames)?;

        if self.profiles.len() >= MAX_PROFILES {
            return Err(ProfileError::ProfileLimitReached);
        }

        let profile = LearnerProfile {
            profile_id: ProfileId::new(),
            nickname: nickname.trim().to_string(),
            created_at: chrono::Utc::now(),
            progress: LearningProgress::default(),
        };

        self.store.save(&profile)?;

        self.profiles.push(ProfileSummary {
            profile_id: profile.profile_id,
            nickname: profile.nickname.clone(),
        });

        Ok(profile)
    }

    /// 选择一个已存在的学员档案：加载其完整学习进度（Req 6.4）。
    ///
    /// 若 `profile_id` 不在当前已知档案列表中，返回 `ProfileError::NotFound`，
    /// 不尝试直接触碰存储层（避免绕过档案选择界面展示的列表访问任意文件）。
    pub fn select_profile(&self, profile_id: ProfileId) -> Result<LearnerProfile, ProfileError> {
        if !self
            .profiles
            .iter()
            .any(|summary| summary.profile_id == profile_id)
        {
            return Err(ProfileError::NotFound);
        }

        let profile = self.store.load(profile_id)?;
        Ok(profile)
    }

    /// 将一个已加载并在内存中修改过的学员档案落盘（与 `create_profile` 不同，
    /// 本方法不做昵称/数量上限校验，也不新建档案标识——用于练习完成后重新
    /// 计算课程解锁状态并持久化已变更的 `Learning_Progress` 的场景，
    /// Req 1.4, 1.5, 5.1）。
    ///
    /// 若 `profile.profile_id` 不在当前已知档案列表中，返回
    /// `ProfileError::NotFound`（该档案未通过 `create_profile`/`select_profile`
    /// 纳入本次会话已知的档案集合，避免绕过档案选择界面写入任意档案文件）。
    ///
    /// 保存失败时返回 `ProfileError::SaveFailed`，调用方持有的 `profile` 内存
    /// 数据不受影响，可在下一次事件或应用关闭前重试写入（Req 5.5）。已知档案
    /// 列表中的昵称摘要会同步刷新，以反映档案可能发生的最新状态。
    pub fn save_progress(&mut self, profile: &LearnerProfile) -> Result<(), ProfileError> {
        let Some(summary) = self
            .profiles
            .iter_mut()
            .find(|summary| summary.profile_id == profile.profile_id)
        else {
            return Err(ProfileError::NotFound);
        };

        self.store.save(profile)?;

        summary.nickname = profile.nickname.clone();

        Ok(())
    }

    /// 删除一个学员档案：昵称、学习进度与全部练习记录随档案文件一并永久
    /// 删除，并释放一个档案名额（Req 6.11）。
    ///
    /// 顺序是"先删文件，再更新内存列表"：删除失败时内存列表保持原样，不会
    /// 出现"列表里已经没有、磁盘上却还在"的不一致（与 `create_profile` 在
    /// 保存失败时不把新档案加入列表是同一条原则）。
    ///
    /// 若 `profile_id` 不在当前已知档案列表中，返回 `ProfileError::NotFound`
    /// 且不触碰存储层——与 `select_profile`/`save_progress` 一致，不允许绕过
    /// 档案选择界面展示的列表去删除任意文件。文件删除失败返回
    /// `ProfileError::DeleteFailed`，磁盘数据保持原样（Req 6.14）。
    ///
    /// **Validates: Requirements 6.11, 6.14**
    pub fn delete_profile(&mut self, profile_id: ProfileId) -> Result<(), ProfileError> {
        if !self.contains(profile_id) {
            return Err(ProfileError::NotFound);
        }

        self.store.delete(profile_id)?;

        self.profiles
            .retain(|summary| summary.profile_id != profile_id);

        Ok(())
    }

    /// 重置一个学员档案的学习进度：课程解锁状态与练习记录恢复为与新建档案
    /// 完全一致的初始状态，档案标识、昵称、创建时间原样保留（Req 6.12）。
    ///
    /// 初始进度直接复用 `LearningProgress::default()`——与 `create_profile`
    /// 构造新档案时用的是同一个表达式，而不是另写一套"逐字段清空"的逻辑。
    /// 这样"重置后的档案"与"新建的档案"在数据层面不可区分，也不存在两处
    /// 初始状态定义各自漂移的可能。
    ///
    /// 返回重置后的完整档案，供调用方（`AppController`）在该档案恰好是当前
    /// 激活档案时同步刷新内存中的会话状态。
    ///
    /// 失败路径：档案不在已知列表 -> `NotFound`（不触碰存储层）；档案文件
    /// 损坏无法加载 -> `LoadFailed`（不写入、不覆盖原文件）；保存失败 ->
    /// `SaveFailed`（原文件内容保持不变，Req 6.14）。
    ///
    /// **Validates: Requirements 6.12, 6.14**
    pub fn reset_progress(
        &mut self,
        profile_id: ProfileId,
    ) -> Result<LearnerProfile, ProfileError> {
        if !self.contains(profile_id) {
            return Err(ProfileError::NotFound);
        }

        let mut profile = self.store.load(profile_id)?;
        profile.progress = LearningProgress::default();

        self.store.save(&profile)?;

        Ok(profile)
    }

    /// 重命名一个学员档案：只更新昵称，学习进度与练习记录保持不变（Req 6.13）。
    ///
    /// 昵称校验复用与新建档案完全相同的 `validate_nickname`，但传入的"已存在
    /// 昵称集合"**排除被重命名档案自身**：否则把昵称"改成"它当前已有的值
    /// （或只改了大小写/首尾空白）会被误判为 `Duplicate`。除此之外非空、
    /// 不超过 20 字符、不与**其他**档案重复这三条规则与创建时逐字一致。
    ///
    /// 与 `create_profile` 一致，最终存储的是去除首尾空白后的昵称。
    ///
    /// 失败路径：档案不在已知列表 -> `NotFound`；昵称校验失败 ->
    /// `InvalidNickname`（此时既不加载也不写入任何文件）；加载/保存失败 ->
    /// `LoadFailed`/`SaveFailed`，原文件内容保持不变（Req 6.14）。
    ///
    /// **Validates: Requirements 6.13, 6.14**
    pub fn rename_profile(
        &mut self,
        profile_id: ProfileId,
        nickname: &str,
    ) -> Result<LearnerProfile, ProfileError> {
        if !self.contains(profile_id) {
            return Err(ProfileError::NotFound);
        }

        let other_nicknames: Vec<String> = self
            .profiles
            .iter()
            .filter(|summary| summary.profile_id != profile_id)
            .map(|summary| summary.nickname.clone())
            .collect();

        validate_nickname(nickname, &other_nicknames)?;

        let mut profile = self.store.load(profile_id)?;
        profile.nickname = nickname.trim().to_string();

        self.store.save(&profile)?;

        if let Some(summary) = self
            .profiles
            .iter_mut()
            .find(|summary| summary.profile_id == profile_id)
        {
            summary.nickname = profile.nickname.clone();
        }

        Ok(profile)
    }

    /// 该档案标识是否在当前已知档案列表中。
    fn contains(&self, profile_id: ProfileId) -> bool {
        self.profiles
            .iter()
            .any(|summary| summary.profile_id == profile_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 在系统临时目录下创建一个唯一的子目录用于测试，返回其路径。
    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "typing-profile-manager-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir for test");
        dir
    }

    /// 生成可能包含前后空白与中文字符的任意字符串，用于覆盖裁剪逻辑。
    fn arb_nickname() -> impl Strategy<Value = String> {
        prop_oneof![
            // 常规可打印字符（含空白），可能为空、可能超长
            ".{0,30}",
            // 中文字符片段，可能超过 20 个字符
            "[\\p{Han}]{0,25}",
            // 带前后空白包裹的中文昵称
            "[ \\t]{0,3}[\\p{Han}a-zA-Z]{0,25}[ \\t]{0,3}",
        ]
    }

    /// 生成小型的已存在昵称列表（每个昵称非空、长度受限，便于构造重复命中的场景）。
    fn arb_existing_nicknames() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec("[\\p{Han}a-zA-Z]{1,10}", 0..5)
    }

    #[test]
    fn empty_string_is_rejected() {
        assert_eq!(validate_nickname("", &[]), Err(NicknameError::Empty));
    }

    #[test]
    fn whitespace_only_is_rejected_as_empty() {
        assert_eq!(validate_nickname("   ", &[]), Err(NicknameError::Empty));
    }

    #[test]
    fn valid_nickname_is_accepted() {
        assert_eq!(validate_nickname("小明", &[]), Ok(()));
    }

    #[test]
    fn nickname_with_surrounding_whitespace_is_trimmed_before_validation() {
        assert_eq!(validate_nickname("  小明  ", &[]), Ok(()));
    }

    #[test]
    fn exactly_20_chars_is_accepted() {
        let nickname = "a".repeat(20);
        assert_eq!(validate_nickname(&nickname, &[]), Ok(()));
    }

    #[test]
    fn over_20_chars_is_rejected() {
        let nickname = "a".repeat(21);
        assert_eq!(
            validate_nickname(&nickname, &[]),
            Err(NicknameError::TooLong)
        );
    }

    #[test]
    fn over_20_chinese_chars_is_rejected_by_char_count_not_byte_len() {
        // 21 个中文字符，字节长度远超 20，但重点是按字符数（chars().count()）判定。
        let nickname = "字".repeat(21);
        assert_eq!(
            validate_nickname(&nickname, &[]),
            Err(NicknameError::TooLong)
        );
    }

    #[test]
    fn twenty_chinese_chars_is_accepted() {
        let nickname = "字".repeat(20);
        assert_eq!(validate_nickname(&nickname, &[]), Ok(()));
    }

    #[test]
    fn duplicate_nickname_is_rejected() {
        let existing = vec!["小明".to_string(), "小红".to_string()];
        assert_eq!(
            validate_nickname("小明", &existing),
            Err(NicknameError::Duplicate)
        );
    }

    #[test]
    fn duplicate_check_is_case_sensitive() {
        let existing = vec!["Alice".to_string()];
        assert_eq!(validate_nickname("alice", &existing), Ok(()));
    }

    #[test]
    fn duplicate_check_compares_trimmed_value() {
        let existing = vec!["小明".to_string()];
        assert_eq!(
            validate_nickname("  小明  ", &existing),
            Err(NicknameError::Duplicate)
        );
    }

    #[test]
    fn non_duplicate_nickname_among_existing_is_accepted() {
        let existing = vec!["小明".to_string(), "小红".to_string()];
        assert_eq!(validate_nickname("小刚", &existing), Ok(()));
    }

    // Feature: typing-desktop-app, Property 14: 昵称校验规则
    proptest! {
        #[test]
        fn prop_nickname_validation_rules(
            nickname in arb_nickname(),
            existing in arb_existing_nicknames(),
        ) {
            let trimmed = nickname.trim();
            let result = validate_nickname(&nickname, &existing);

            if trimmed.is_empty() {
                prop_assert_eq!(result, Err(NicknameError::Empty));
            } else if trimmed.chars().count() > 20 {
                prop_assert_eq!(result, Err(NicknameError::TooLong));
            } else if existing.iter().any(|e| e.trim() == trimmed) {
                prop_assert_eq!(result, Err(NicknameError::Duplicate));
            } else {
                prop_assert_eq!(result, Ok(()));
            }
        }
    }

    // ---- ProfileManager tests (task 12.3) ----

    #[test]
    fn new_manager_with_empty_dir_has_no_profiles() {
        let dir = unique_temp_dir("empty-startup");
        let store = ProfileStore::new(&dir);
        let manager = ProfileManager::new(store);

        assert!(manager.list_profiles().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_profile_persists_and_returns_initial_state() {
        let dir = unique_temp_dir("create");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let profile = manager
            .create_profile("小明")
            .expect("create should succeed");

        assert_eq!(profile.nickname, "小明");
        assert!(profile.progress.lesson_records.is_empty());
        assert!(profile.progress.unlocked_lesson_ids.is_empty());
        assert_eq!(manager.list_profiles().len(), 1);
        assert_eq!(manager.list_profiles()[0].profile_id, profile.profile_id);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_profile_rejects_invalid_nickname() {
        let dir = unique_temp_dir("invalid-nickname");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let result = manager.create_profile("");

        assert!(matches!(
            result,
            Err(ProfileError::InvalidNickname(NicknameError::Empty))
        ));
        assert!(manager.list_profiles().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_profile_rejects_duplicate_nickname_against_known_profiles() {
        let dir = unique_temp_dir("duplicate-nickname");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        manager
            .create_profile("小明")
            .expect("first create should succeed");
        let result = manager.create_profile("小明");

        assert!(matches!(
            result,
            Err(ProfileError::InvalidNickname(NicknameError::Duplicate))
        ));
        assert_eq!(manager.list_profiles().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 新建档案昵称超长（去除首尾空白后字符数 > 20）时，`create_profile`
    /// 应拒绝创建并返回 `NicknameError::TooLong`，且不将该档案计入已知列表
    /// （Req 6.5）。
    #[test]
    fn create_profile_rejects_too_long_nickname() {
        let dir = unique_temp_dir("too-long-nickname");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let too_long_nickname = "字".repeat(21);
        let result = manager.create_profile(&too_long_nickname);

        assert!(matches!(
            result,
            Err(ProfileError::InvalidNickname(NicknameError::TooLong))
        ));
        assert!(manager.list_profiles().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 验证 Req 6.5, 6.7 中"显示错误提示指明具体原因"的文案映射：
    /// 昵称非法的三种具体原因（空/超长/重复）分别对应设计文档中约定的
    /// 三段不同错误提示文本，且互不相同。这里直接对 `create_profile`
    /// 返回的 `ProfileError` 调用 `Display`，确保面向 UI 展示的最终文案
    /// （而非仅 `NicknameError` 内部枚举变体）与需求描述一致。
    #[test]
    fn create_profile_invalid_nickname_error_messages_match_expected_text() {
        let dir = unique_temp_dir("error-message-mapping");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        // 预置一个已存在的档案，用于触发重复昵称场景。
        manager
            .create_profile("小明")
            .expect("seed profile should be created successfully");

        let empty_result = manager.create_profile("");
        let too_long_result = manager.create_profile(&"字".repeat(21));
        let duplicate_result = manager.create_profile("小明");

        let empty_err = empty_result.expect_err("empty nickname should be rejected");
        let too_long_err = too_long_result.expect_err("too-long nickname should be rejected");
        let duplicate_err = duplicate_result.expect_err("duplicate nickname should be rejected");

        assert_eq!(empty_err.to_string(), "昵称不能为空");
        assert_eq!(too_long_err.to_string(), "昵称不能超过 20 个字符");
        assert_eq!(duplicate_err.to_string(), "该昵称已被使用");

        // 三种错误提示文案彼此不同，确保用户能区分具体拒绝原因。
        assert_ne!(empty_err.to_string(), too_long_err.to_string());
        assert_ne!(empty_err.to_string(), duplicate_err.to_string());
        assert_ne!(too_long_err.to_string(), duplicate_err.to_string());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 验证 Req 6.5, 6.7 中"保留用户当前输入内容"的契约：`create_profile`
    /// 仅通过 `&str` 借用调用方传入的昵称，从不获取所有权、不修改、不清空，
    /// 因此无论校验因空/超长/重复哪种原因失败，调用方持有的原始昵称字符串
    /// 在调用后依然完整可用，可直接用于重新展示在输入框或再次尝试提交。
    #[test]
    fn create_profile_failure_leaves_callers_input_nickname_intact() {
        let dir = unique_temp_dir("input-preservation");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        manager
            .create_profile("小明")
            .expect("seed profile should be created successfully");

        // 空昵称场景：调用前后调用方原始字符串内容不变。
        let empty_input = String::from("   ");
        let empty_snapshot = empty_input.clone();
        let empty_result = manager.create_profile(&empty_input);
        assert!(matches!(
            empty_result,
            Err(ProfileError::InvalidNickname(NicknameError::Empty))
        ));
        assert_eq!(empty_input, empty_snapshot);

        // 超长昵称场景：调用前后调用方原始字符串内容不变，可直接重新提交。
        let too_long_input = "字".repeat(25);
        let too_long_snapshot = too_long_input.clone();
        let too_long_result = manager.create_profile(&too_long_input);
        assert!(matches!(
            too_long_result,
            Err(ProfileError::InvalidNickname(NicknameError::TooLong))
        ));
        assert_eq!(too_long_input, too_long_snapshot);

        // 重复昵称场景：调用前后调用方原始字符串内容不变。
        let duplicate_input = String::from("小明");
        let duplicate_snapshot = duplicate_input.clone();
        let duplicate_result = manager.create_profile(&duplicate_input);
        assert!(matches!(
            duplicate_result,
            Err(ProfileError::InvalidNickname(NicknameError::Duplicate))
        ));
        assert_eq!(duplicate_input, duplicate_snapshot);

        // 三次失败均未将非法档案计入已知列表，仅保留最初预置的合法档案。
        assert_eq!(manager.list_profiles().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_profile_rejects_when_limit_reached() {
        let dir = unique_temp_dir("limit");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        for i in 0..MAX_PROFILES {
            manager
                .create_profile(&format!("学员{i}"))
                .expect("creates up to limit should succeed");
        }
        assert_eq!(manager.list_profiles().len(), MAX_PROFILES);

        let result = manager.create_profile("超限档案");

        assert!(matches!(result, Err(ProfileError::ProfileLimitReached)));
        assert_eq!(manager.list_profiles().len(), MAX_PROFILES);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn select_profile_loads_matching_progress() {
        let dir = unique_temp_dir("select");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let created = manager
            .create_profile("小红")
            .expect("create should succeed");
        let loaded = manager
            .select_profile(created.profile_id)
            .expect("select should succeed for known profile");

        assert_eq!(loaded, created);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn select_profile_returns_not_found_for_unknown_id() {
        let dir = unique_temp_dir("select-unknown");
        let store = ProfileStore::new(&dir);
        let manager = ProfileManager::new(store);

        let result = manager.select_profile(ProfileId::new());

        assert!(matches!(result, Err(ProfileError::NotFound)));

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- 档案管理：删除 / 重置进度 / 重命名（Req 6.11-6.14） ----

    /// 给指定档案塞入一条非空的学习进度并落盘，用于验证"重置会清空进度"与
    /// "重命名/删除不会误改进度"。返回写入后的完整档案。
    fn with_some_progress(manager: &mut ProfileManager, profile_id: ProfileId) -> LearnerProfile {
        use crate::storage::{LessonId, LessonRecord, PracticeResult};

        let mut profile = manager
            .select_profile(profile_id)
            .expect("select should succeed");

        let lesson_id = LessonId("lesson-1".to_string());
        let result = PracticeResult {
            lesson_id: lesson_id.clone(),
            accuracy: 98.5,
            wpm: 12.0,
            error_count: 1,
            duration_ms: 30_000,
            score: 90,
            completed_at: chrono::Utc::now(),
        };
        profile.progress.lesson_records.insert(
            lesson_id.clone(),
            LessonRecord {
                best_accuracy: 98.5,
                best_wpm: 12.0,
                best_score: 90,
                achieved_at: result.completed_at,
                attempt_count: 3,
                history: vec![result],
            },
        );
        profile
            .progress
            .unlocked_lesson_ids
            .insert(LessonId("lesson-2".to_string()));

        manager
            .save_progress(&profile)
            .expect("save_progress should succeed");

        profile
    }

    #[test]
    fn delete_profile_removes_it_from_list_and_from_store() {
        let dir = unique_temp_dir("delete-profile");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let profile = manager.create_profile("小明").expect("create should succeed");
        assert_eq!(manager.list_profiles().len(), 1);

        manager
            .delete_profile(profile.profile_id)
            .expect("delete should succeed");

        assert!(manager.list_profiles().is_empty());
        // 重新扫描目录构造的新 manager 也看不到它 —— 文件确实被删了。
        let fresh = ProfileManager::new(ProfileStore::new(&dir));
        assert!(fresh.list_profiles().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Req 6.11：删除后应释放一个档案名额（原本已达上限时可以再建一个）。
    #[test]
    fn delete_profile_frees_one_slot_when_limit_was_reached() {
        let dir = unique_temp_dir("delete-frees-slot");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let mut ids = Vec::new();
        for index in 0..MAX_PROFILES {
            ids.push(
                manager
                    .create_profile(&format!("学员{index}"))
                    .expect("create should succeed")
                    .profile_id,
            );
        }
        assert!(matches!(
            manager.create_profile("溢出的学员"),
            Err(ProfileError::ProfileLimitReached)
        ));

        manager.delete_profile(ids[0]).expect("delete should succeed");

        assert_eq!(manager.list_profiles().len(), MAX_PROFILES - 1);
        manager
            .create_profile("补位的学员")
            .expect("删除释放名额后应可再次创建");
        assert_eq!(manager.list_profiles().len(), MAX_PROFILES);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_profile_unknown_id_returns_not_found_and_changes_nothing() {
        let dir = unique_temp_dir("delete-unknown");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let kept = manager.create_profile("小红").expect("create should succeed");

        let result = manager.delete_profile(ProfileId::new());

        assert!(matches!(result, Err(ProfileError::NotFound)));
        assert_eq!(manager.list_profiles().len(), 1);
        assert_eq!(
            manager.select_profile(kept.profile_id).unwrap(),
            kept,
            "未知 id 的删除不应影响已有档案"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_profile_keeps_other_profiles_intact() {
        let dir = unique_temp_dir("delete-others-intact");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let victim = manager.create_profile("要删的").expect("create should succeed");
        let survivor = manager.create_profile("要留的").expect("create should succeed");
        let survivor = with_some_progress(&mut manager, survivor.profile_id);

        manager
            .delete_profile(victim.profile_id)
            .expect("delete should succeed");

        assert_eq!(manager.list_profiles().len(), 1);
        assert_eq!(
            manager.select_profile(survivor.profile_id).unwrap(),
            survivor,
            "删除一个档案不得改动另一个档案的任何数据"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Req 6.12：重置进度清空解锁状态与练习记录，但保留标识/昵称/创建时间。
    #[test]
    fn reset_progress_clears_progress_but_keeps_identity() {
        let dir = unique_temp_dir("reset-progress");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let created = manager.create_profile("小明").expect("create should succeed");
        let with_progress = with_some_progress(&mut manager, created.profile_id);
        assert!(!with_progress.progress.lesson_records.is_empty());
        assert!(!with_progress.progress.unlocked_lesson_ids.is_empty());

        let reset = manager
            .reset_progress(created.profile_id)
            .expect("reset should succeed");

        assert!(reset.progress.lesson_records.is_empty());
        assert!(reset.progress.unlocked_lesson_ids.is_empty());
        assert_eq!(reset.profile_id, created.profile_id);
        assert_eq!(reset.nickname, created.nickname);
        assert_eq!(reset.created_at, created.created_at);

        // 已落盘：重新加载得到的就是重置后的状态。
        assert_eq!(manager.select_profile(created.profile_id).unwrap(), reset);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Req 6.12 的核心不变式：重置后的进度与"新建档案的进度"完全一致，
    /// 即重置复用的就是同一份初始状态定义，不存在第二套清空逻辑。
    #[test]
    fn reset_progress_yields_the_same_progress_as_a_freshly_created_profile() {
        let dir = unique_temp_dir("reset-equals-fresh");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let old = manager.create_profile("老档案").expect("create should succeed");
        with_some_progress(&mut manager, old.profile_id);
        let reset = manager
            .reset_progress(old.profile_id)
            .expect("reset should succeed");

        let fresh = manager.create_profile("新档案").expect("create should succeed");

        assert_eq!(reset.progress, fresh.progress);
        assert_eq!(reset.progress, LearningProgress::default());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_progress_unknown_id_returns_not_found() {
        let dir = unique_temp_dir("reset-unknown");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        assert!(matches!(
            manager.reset_progress(ProfileId::new()),
            Err(ProfileError::NotFound)
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Req 6.13：重命名只改昵称，学习进度与练习记录不受影响。
    #[test]
    fn rename_profile_updates_nickname_and_preserves_progress() {
        let dir = unique_temp_dir("rename");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let created = manager.create_profile("小明").expect("create should succeed");
        let with_progress = with_some_progress(&mut manager, created.profile_id);

        let renamed = manager
            .rename_profile(created.profile_id, "小明明")
            .expect("rename should succeed");

        assert_eq!(renamed.nickname, "小明明");
        assert_eq!(renamed.progress, with_progress.progress, "进度不应被改动");
        assert_eq!(renamed.profile_id, created.profile_id);
        // 列表摘要与磁盘内容都同步更新。
        assert_eq!(manager.list_profiles()[0].nickname, "小明明");
        assert_eq!(manager.select_profile(created.profile_id).unwrap(), renamed);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Req 6.13 的关键细节：重复判定必须排除被重命名档案自身，否则"改回
    /// 自己当前的昵称"（或只调整首尾空白）会被误判为昵称重复。
    #[test]
    fn rename_profile_allows_keeping_its_own_current_nickname() {
        let dir = unique_temp_dir("rename-self");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let created = manager.create_profile("小明").expect("create should succeed");

        manager
            .rename_profile(created.profile_id, "小明")
            .expect("把昵称改成自己当前的值不应被判为重复");
        manager
            .rename_profile(created.profile_id, "  小明  ")
            .expect("仅首尾空白不同同样不应被判为重复");

        assert_eq!(manager.list_profiles()[0].nickname, "小明");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_profile_rejects_nickname_used_by_another_profile() {
        let dir = unique_temp_dir("rename-duplicate");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let first = manager.create_profile("小明").expect("create should succeed");
        let second = manager.create_profile("小红").expect("create should succeed");

        let result = manager.rename_profile(second.profile_id, "小明");

        assert!(matches!(
            result,
            Err(ProfileError::InvalidNickname(NicknameError::Duplicate))
        ));
        // 双方昵称都没变。
        assert_eq!(manager.select_profile(first.profile_id).unwrap().nickname, "小明");
        assert_eq!(manager.select_profile(second.profile_id).unwrap().nickname, "小红");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_profile_applies_the_same_nickname_rules_as_creation() {
        let dir = unique_temp_dir("rename-rules");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let created = manager.create_profile("小明").expect("create should succeed");

        assert!(matches!(
            manager.rename_profile(created.profile_id, ""),
            Err(ProfileError::InvalidNickname(NicknameError::Empty))
        ));
        assert!(matches!(
            manager.rename_profile(created.profile_id, "   "),
            Err(ProfileError::InvalidNickname(NicknameError::Empty))
        ));
        assert!(matches!(
            manager.rename_profile(created.profile_id, &"啊".repeat(21)),
            Err(ProfileError::InvalidNickname(NicknameError::TooLong))
        ));
        // 20 个字符是允许的上界（与创建时一致）。
        manager
            .rename_profile(created.profile_id, &"啊".repeat(20))
            .expect("20 个字符应被接受");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Req 6.14：昵称校验失败时磁盘上的档案数据不得被改动。
    #[test]
    fn rename_profile_validation_failure_leaves_stored_profile_unchanged() {
        let dir = unique_temp_dir("rename-failure-intact");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let created = manager.create_profile("小明").expect("create should succeed");
        let before = with_some_progress(&mut manager, created.profile_id);

        let _ = manager.rename_profile(created.profile_id, "");

        assert_eq!(
            manager.select_profile(created.profile_id).unwrap(),
            before,
            "校验失败不应写入任何改动"
        );
        assert_eq!(manager.list_profiles()[0].nickname, "小明");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_profile_unknown_id_returns_not_found() {
        let dir = unique_temp_dir("rename-unknown");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        assert!(matches!(
            manager.rename_profile(ProfileId::new(), "新名字"),
            Err(ProfileError::NotFound)
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(15))]
        #[test]
        fn prop_profile_limit_bidirectional_behavior(attempt_count in 0usize..=(MAX_PROFILES + 5)) {
            let dir = unique_temp_dir(&format!("limit-bidirectional-{}", uuid::Uuid::new_v4()));
            let store = ProfileStore::new(&dir);
            let mut manager = ProfileManager::new(store);

            for i in 0..attempt_count {
                let result = manager.create_profile(&format!("学员{i}"));

                if i < MAX_PROFILES {
                    prop_assert!(
                        result.is_ok(),
                        "creation #{} (0-indexed) should succeed while under the limit",
                        i
                    );
                    prop_assert_eq!(manager.list_profiles().len(), i + 1);
                } else {
                    prop_assert!(
                        matches!(result, Err(ProfileError::ProfileLimitReached)),
                        "creation #{} (0-indexed) should be rejected once the limit is reached",
                        i
                    );
                    prop_assert_eq!(manager.list_profiles().len(), MAX_PROFILES);
                }
            }

            std::fs::remove_dir_all(&dir).ok();
        }
    }

    /// 集成测试（Req 5.2）：应用重启后从学员档案恢复课程解锁状态与历史成绩
    /// 的耗时验证。
    ///
    /// 模拟"应用关闭并重新启动"：先在真实临时目录中创建若干个学员档案
    /// （每个档案都写有一定的学习进度数据），随后用同一目录重新构造
    /// `ProfileManager::new`——这正是恢复流程的入口，内部会扫描目录并逐一
    /// 加载、解析每个档案文件（真实磁盘 I/O）。断言该恢复过程在 5 秒内
    /// 完成（预期应为毫秒级，此处给予充裕的多秒级余量以避免在较慢的 CI
    /// 环境下出现误报，同时仍能捕捉到真正的性能退化）。
    #[test]
    fn profile_manager_restoration_completes_within_five_second_budget() {
        let dir = unique_temp_dir("restore-timing");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        // 预先创建若干个学员档案并写入一些学习进度数据，模拟真实的
        // "应用关闭前"状态，而不是空目录下的恢复。
        for i in 0..5 {
            let mut profile = manager
                .create_profile(&format!("学员{i}"))
                .expect("seed profile creation should succeed");

            profile.progress.lesson_records.insert(
                crate::storage::LessonId(format!("lesson-{i}")),
                crate::storage::LessonRecord {
                    best_accuracy: 88.5,
                    best_wpm: 42.0,
                    best_score: 89,
                    achieved_at: chrono::Utc::now(),
                    attempt_count: 3,
                    history: vec![],
                },
            );
            profile
                .progress
                .unlocked_lesson_ids
                .insert(crate::storage::LessonId(format!("lesson-{}", i + 1)));

            manager
                .save_progress(&profile)
                .expect("seeding progress before restart should succeed");
        }

        // 模拟应用重启：用同一磁盘目录重新构造 ProfileManager，这一构造过程
        // 即为"应用重启后从学员档案恢复课程解锁状态与历史成绩"的入口。
        let reopened_store = ProfileStore::new(&dir);

        let started = std::time::Instant::now();
        let restored_manager = ProfileManager::new(reopened_store);
        let elapsed = started.elapsed();

        // 恢复过程正确性的最小校验：全部档案都被重新发现。
        assert_eq!(restored_manager.list_profiles().len(), 5);

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "ProfileManager::new should restore profiles within the 5s budget (Req 5.2), took {:?}",
            elapsed
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn startup_load_discovers_profiles_saved_in_previous_session() {
        let dir = unique_temp_dir("restart");
        let store = ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);

        let created_a = manager
            .create_profile("甲")
            .expect("create a should succeed");
        let created_b = manager
            .create_profile("乙")
            .expect("create b should succeed");

        // 模拟应用重启：用同一目录重新构造 ProfileManager。
        let reopened_store = ProfileStore::new(&dir);
        let reopened_manager = ProfileManager::new(reopened_store);

        let mut nicknames: Vec<String> = reopened_manager
            .list_profiles()
            .iter()
            .map(|s| s.nickname.clone())
            .collect();
        nicknames.sort();
        assert_eq!(nicknames, vec!["乙".to_string(), "甲".to_string()]);

        let reloaded_a = reopened_manager
            .select_profile(created_a.profile_id)
            .expect("profile a should be loadable after restart");
        let reloaded_b = reopened_manager
            .select_profile(created_b.profile_id)
            .expect("profile b should be loadable after restart");
        assert_eq!(reloaded_a, created_a);
        assert_eq!(reloaded_b, created_b);

        std::fs::remove_dir_all(&dir).ok();
    }
}
