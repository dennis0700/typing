//! 学员档案文件读写、损坏检测。
//!
//! `ProfileStore` 负责将 `LearnerProfile` 以 JSON 形式持久化到本机文件系统
//! （macOS 路径约定：`~/Library/Application Support/typing/profiles/{profile_id}.json`，
//! 详见设计文档 Storage 决策）。读写失败均通过 `Result` 显式表达，不使用
//! `.unwrap()`/`.expect()`，也不会因为 I/O 异常导致学员当前会话数据丢失
//! （Req 5.3、5.4、5.5）。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::storage::schema::{LearnerProfile, ProfileId};

/// 档案读取失败的错误类型。
#[derive(Debug)]
pub enum ProfileLoadError {
    /// 档案文件不存在。
    NotFound,
    /// 档案文件存在但无法读取或无法解析为合法的 `LearnerProfile`
    /// （空文件、截断 JSON、字段类型错误等）。读取失败时原始文件不会被
    /// 覆盖或删除（Req 5.4）。
    Corrupted,
}

impl fmt::Display for ProfileLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileLoadError::NotFound => write!(f, "档案文件不存在"),
            ProfileLoadError::Corrupted => write!(f, "档案数据无法读取或解析"),
        }
    }
}

impl std::error::Error for ProfileLoadError {}

/// 档案写入失败的错误类型。
#[derive(Debug)]
pub enum ProfileSaveError {
    /// 写入本机文件系统失败（如存储空间不足或写入权限被拒绝，Req 5.5）。
    Io(io::Error),
}

impl fmt::Display for ProfileSaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileSaveError::Io(err) => write!(f, "档案保存失败: {err}"),
        }
    }
}

impl std::error::Error for ProfileSaveError {}

impl From<io::Error> for ProfileSaveError {
    fn from(err: io::Error) -> Self {
        ProfileSaveError::Io(err)
    }
}

/// 档案删除失败的错误类型（Req 6.11, 6.14）。
#[derive(Debug)]
pub enum ProfileDeleteError {
    /// 档案文件不存在（可能已被外部删除，或 `profile_id` 有误）。
    NotFound,
    /// 删除本机文件失败（如权限被拒绝、文件被其他进程占用）。
    Io(io::Error),
}

impl fmt::Display for ProfileDeleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileDeleteError::NotFound => write!(f, "档案文件不存在"),
            ProfileDeleteError::Io(err) => write!(f, "档案删除失败: {err}"),
        }
    }
}

impl std::error::Error for ProfileDeleteError {}

/// 学员档案存储：持有档案文件所在的基础目录，负责单个档案的读写。
///
/// 每个学员档案对应 `{base_dir}/{profile_id}.json` 一个文件，文件之间互不
/// 影响（Req 5.6）：写入某个档案失败或读取某个档案损坏时，不会触碰其他档案
/// 文件，也不会删除或覆盖出问题的文件本身。
#[derive(Debug, Clone)]
pub struct ProfileStore {
    base_dir: PathBuf,
}

impl ProfileStore {
    /// 使用指定的基础目录构造一个 `ProfileStore`（不会立即创建目录，
    /// 目录在首次 `save` 时按需创建）。
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// 解析 macOS 上的默认档案存储目录：
    /// `~/Library/Application Support/typing/profiles/`。
    ///
    /// 通过 `HOME` 环境变量定位用户主目录（项目当前未引入 `dirs` 一类的
    /// 目录解析 crate；若后续需要跨平台目录解析，引入 `dirs` crate 会比手工
    /// 拼接 `HOME` 更健壮，但需先与设计文档确认后再添加新依赖）。
    pub fn default_macos_dir() -> io::Result<PathBuf> {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "无法定位用户主目录（HOME 未设置）")
        })?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("typing")
            .join("profiles"))
    }

    /// 使用 macOS 默认目录构造 `ProfileStore`。
    pub fn new_default() -> io::Result<Self> {
        Ok(Self::new(Self::default_macos_dir()?))
    }

    /// 返回该档案对应的文件路径。
    pub fn path_for(&self, profile_id: ProfileId) -> PathBuf {
        self.base_dir.join(format!("{}.json", profile_id.0))
    }

    /// 返回基础目录路径。
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// 将档案序列化为 JSON 并写入 `{base_dir}/{profile_id}.json`。
    ///
    /// 写入采用"临时文件 + 原子重命名"策略：先写入同目录下的临时文件，
    /// 成功后再 rename 到目标路径。任何一步失败都返回
    /// `Err(ProfileSaveError::Io(..))`，且不会留下部分写入的目标文件，也不会
    /// 清空调用方内存中的档案数据（Req 5.5——保存失败与内存状态无关，调用方
    /// 自行保留原状态）。
    pub fn save(&self, profile: &LearnerProfile) -> Result<(), ProfileSaveError> {
        fs::create_dir_all(&self.base_dir)?;

        let json = serde_json::to_string_pretty(profile)
            .map_err(|err| ProfileSaveError::Io(io::Error::new(io::ErrorKind::InvalidData, err)))?;

        let target_path = self.path_for(profile.profile_id);
        let tmp_path = self
            .base_dir
            .join(format!("{}.json.tmp", profile.profile_id.0));

        fs::write(&tmp_path, json.as_bytes())?;
        fs::rename(&tmp_path, &target_path)?;

        Ok(())
    }

    /// 读取并反序列化 `{base_dir}/{profile_id}.json`。
    ///
    /// 文件不存在返回 `Err(ProfileLoadError::NotFound)`；文件存在但内容无法
    /// 解析为合法的 `LearnerProfile`（空文件、截断 JSON、字段类型错误等）
    /// 返回 `Err(ProfileLoadError::Corrupted)`。两种失败情况均不会覆盖或
    /// 删除原始文件（Req 5.4）。
    pub fn load(&self, profile_id: ProfileId) -> Result<LearnerProfile, ProfileLoadError> {
        let path = self.path_for(profile_id);

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Err(ProfileLoadError::NotFound);
            }
            Err(_) => return Err(ProfileLoadError::Corrupted),
        };

        serde_json::from_str(&content).map_err(|_| ProfileLoadError::Corrupted)
    }

    /// 删除 `{base_dir}/{profile_id}.json`（Req 6.11）。
    ///
    /// 只删除该 `profile_id` 对应的那**一个**文件（路径由 `path_for` 唯一
    /// 确定），不遍历目录、不做任何模式匹配，因此不可能波及其他档案文件
    /// （Req 5.6 的档案隔离原则同样适用于删除）。
    ///
    /// 文件不存在返回 `Err(ProfileDeleteError::NotFound)`；其余 I/O 失败
    /// （权限被拒绝、文件被占用等）返回 `Err(ProfileDeleteError::Io(..))`。
    /// 两种失败情况下磁盘上的档案数据都保持原样（Req 6.14）。
    pub fn delete(&self, profile_id: ProfileId) -> Result<(), ProfileDeleteError> {
        let path = self.path_for(profile_id);

        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Err(ProfileDeleteError::NotFound),
            Err(err) => Err(ProfileDeleteError::Io(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::schema::LearningProgress;
    use chrono::Utc;
    use std::fs::File;
    use std::io::Write;

    /// 在系统临时目录下创建一个唯一的子目录用于测试，返回其路径。
    /// 调用方负责在测试结束时清理（每个测试使用独立子目录，避免相互干扰）。
    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "typing-profile-store-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("failed to create temp dir for test");
        dir
    }

    fn sample_profile() -> LearnerProfile {
        LearnerProfile {
            profile_id: ProfileId::new(),
            nickname: "小明".to_string(),
            created_at: Utc::now(),
            progress: LearningProgress::default(),
        }
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = unique_temp_dir("round-trip");
        let store = ProfileStore::new(&dir);
        let profile = sample_profile();

        store.save(&profile).expect("save should succeed");
        let loaded = store
            .load(profile.profile_id)
            .expect("load should succeed after save");

        assert_eq!(loaded, profile);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_missing_profile_returns_not_found() {
        let dir = unique_temp_dir("missing");
        let store = ProfileStore::new(&dir);

        let result = store.load(ProfileId::new());

        assert!(matches!(result, Err(ProfileLoadError::NotFound)));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_corrupted_file_returns_corrupted_and_does_not_touch_file() {
        let dir = unique_temp_dir("corrupted");
        fs::create_dir_all(&dir).unwrap();
        let store = ProfileStore::new(&dir);
        let profile_id = ProfileId::new();
        let path = store.path_for(profile_id);

        // 写入一个截断/无法解析的 JSON 文件模拟损坏。
        let mut file = File::create(&path).unwrap();
        file.write_all(b"{ \"profile_id\": ").unwrap();
        drop(file);

        let before = fs::read_to_string(&path).unwrap();
        let result = store.load(profile_id);

        assert!(matches!(result, Err(ProfileLoadError::Corrupted)));

        // 损坏文件必须原样保留，不被覆盖或删除。
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(before, after);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_field_type_mismatch_returns_corrupted_and_does_not_touch_file() {
        let dir = unique_temp_dir("field-type-mismatch");
        fs::create_dir_all(&dir).unwrap();
        let store = ProfileStore::new(&dir);
        let profile_id = ProfileId::new();
        let path = store.path_for(profile_id);

        // 语法合法的 JSON，但 `nickname` 字段类型错误（应为字符串，此处为数字），
        // 模拟字段类型错误导致的档案损坏（Req 1.8、5.4）。
        let mut file = File::create(&path).unwrap();
        file.write_all(
            format!(
                r#"{{"profile_id":"{}","nickname":12345,"created_at":"2024-01-01T00:00:00Z","progress":{{"lesson_records":{{}},"unlocked_lesson_ids":[]}}}}"#,
                profile_id.0
            )
            .as_bytes(),
        )
        .unwrap();
        drop(file);

        let before = fs::read_to_string(&path).unwrap();
        let result = store.load(profile_id);

        assert!(matches!(result, Err(ProfileLoadError::Corrupted)));

        // 损坏文件必须原样保留，不被覆盖或删除。
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(before, after);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_empty_file_returns_corrupted() {
        let dir = unique_temp_dir("empty");
        fs::create_dir_all(&dir).unwrap();
        let store = ProfileStore::new(&dir);
        let profile_id = ProfileId::new();
        let path = store.path_for(profile_id);

        File::create(&path).unwrap();

        let result = store.load(profile_id);

        assert!(matches!(result, Err(ProfileLoadError::Corrupted)));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_creates_base_dir_if_missing() {
        let dir = std::env::temp_dir().join(format!(
            "typing-profile-store-test-autocreate-{}",
            uuid::Uuid::new_v4()
        ));
        // 目录尚不存在。
        assert!(!dir.exists());

        let store = ProfileStore::new(&dir);
        let profile = sample_profile();

        store.save(&profile).expect("save should create base dir");
        assert!(dir.exists());
        assert!(store.path_for(profile.profile_id).exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn multiple_profiles_are_isolated() {
        let dir = unique_temp_dir("isolation");
        let store = ProfileStore::new(&dir);

        let profile_a = sample_profile();
        let profile_b = sample_profile();

        store.save(&profile_a).unwrap();
        store.save(&profile_b).unwrap();

        let loaded_a = store.load(profile_a.profile_id).unwrap();
        let loaded_b = store.load(profile_b.profile_id).unwrap();

        assert_eq!(loaded_a, profile_a);
        assert_eq!(loaded_b, profile_b);
        assert_ne!(loaded_a.profile_id, loaded_b.profile_id);

        fs::remove_dir_all(&dir).ok();
    }

    // ---- delete（Req 6.11, 6.14） ----

    #[test]
    fn delete_removes_the_profile_file_and_makes_it_unloadable() {
        let dir = unique_temp_dir("delete-basic");
        let store = ProfileStore::new(&dir);

        let profile = sample_profile();
        store.save(&profile).unwrap();
        assert!(store.path_for(profile.profile_id).exists());

        store.delete(profile.profile_id).expect("删除应成功");

        assert!(!store.path_for(profile.profile_id).exists());
        assert!(matches!(
            store.load(profile.profile_id),
            Err(ProfileLoadError::NotFound)
        ));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_missing_profile_returns_not_found() {
        let dir = unique_temp_dir("delete-missing");
        let store = ProfileStore::new(&dir);

        assert!(matches!(
            store.delete(ProfileId::new()),
            Err(ProfileDeleteError::NotFound)
        ));

        fs::remove_dir_all(&dir).ok();
    }

    /// Req 5.6 的档案隔离原则同样适用于删除：删除一个档案不得影响其他档案。
    #[test]
    fn delete_does_not_touch_other_profile_files() {
        let dir = unique_temp_dir("delete-isolation");
        let store = ProfileStore::new(&dir);

        let victim = sample_profile();
        let survivor = sample_profile();
        store.save(&victim).unwrap();
        store.save(&survivor).unwrap();

        store.delete(victim.profile_id).expect("删除应成功");

        assert!(!store.path_for(victim.profile_id).exists());
        assert_eq!(store.load(survivor.profile_id).unwrap(), survivor);

        fs::remove_dir_all(&dir).ok();
    }

    /// 删除失败时磁盘上的数据必须保持原样（Req 6.14）。这里构造一个确定性的
    /// 失败条件：把目标"档案文件"实际做成一个**非空目录**——`fs::remove_file`
    /// 对目录会返回 I/O 错误（而非 NotFound），且该目录及其内容不会被删掉。
    #[test]
    fn delete_io_failure_leaves_data_untouched() {
        let dir = unique_temp_dir("delete-io-failure");
        let store = ProfileStore::new(&dir);

        let profile_id = ProfileId::new();
        let blocking_path = store.path_for(profile_id);
        fs::create_dir_all(&blocking_path).expect("failed to create blocking dir for test");
        let inner = blocking_path.join("keep-me.txt");
        File::create(&inner)
            .expect("failed to create inner file")
            .write_all(b"keep")
            .expect("failed to write inner file");

        let err = store.delete(profile_id).expect_err("对目录执行删除应失败");
        assert!(matches!(err, ProfileDeleteError::Io(_)), "实际错误: {err:?}");

        // 失败后原数据仍在。
        assert!(blocking_path.exists());
        assert_eq!(fs::read_to_string(&inner).unwrap(), "keep");

        fs::remove_dir_all(&dir).ok();
    }

    /// 模拟磁盘写入失败场景（Req 5.5）：不依赖真实的磁盘空间耗尽或权限修改，
    /// 而是构造一个必然导致写入失败的文件系统条件——让 `base_dir` 本身指向
    /// 一个已存在的普通文件而非目录。`save` 内部的 `fs::create_dir_all` 会
    /// 因为路径中存在同名非目录节点而返回一个真实的 `io::Error`（`NotADirectory`
    /// 或等价的系统错误码），从而确定性地触发 `ProfileSaveError::Io(..)` 分支，
    /// 无需注入 mock 存储适配器。
    #[test]
    fn save_returns_io_error_when_base_dir_path_is_occupied_by_a_file() {
        let parent = unique_temp_dir("save-failure-parent");
        // base_dir 指向的路径实际是一个文件，而不是（可创建的）目录。
        let occupied_path = parent.join("not-a-directory");
        File::create(&occupied_path).expect("failed to create blocking file for test");

        let store = ProfileStore::new(&occupied_path);
        let profile = sample_profile();

        let result = store.save(&profile);

        assert!(
            matches!(result, Err(ProfileSaveError::Io(_))),
            "expected ProfileSaveError::Io, got {result:?}"
        );

        fs::remove_dir_all(&parent).ok();
    }

    /// 验证 Req 5.5 的调用方契约：`save` 失败时不会以任何方式修改或清空
    /// 调用方持有的 `LearnerProfile`——`save` 仅通过 `&LearnerProfile` 借用
    /// 数据，从不获取所有权、不清空、不修改，因此调用方内存中的学习进度
    /// 数据在写入失败后依然完整可用（无需等待下一次成功写入或应用关闭）。
    #[test]
    fn save_failure_leaves_callers_in_memory_profile_intact() {
        let parent = unique_temp_dir("save-failure-integrity-parent");
        let occupied_path = parent.join("not-a-directory");
        File::create(&occupied_path).expect("failed to create blocking file for test");

        let store = ProfileStore::new(&occupied_path);
        let profile = sample_profile();
        let profile_snapshot = profile.clone();

        let result = store.save(&profile);
        assert!(matches!(result, Err(ProfileSaveError::Io(_))));

        // `profile` 在失败的 `save` 调用之后仍然是调用方可继续使用的完整数据，
        // 未被清空、未被截断、字段值与调用前完全一致。
        assert_eq!(profile, profile_snapshot);

        // 重试写入到一个真实可用的目录应当成功，进一步证明该 profile 值
        // 在失败调用后依然完整有效（不是"半损坏"状态）。
        let retry_dir = unique_temp_dir("save-failure-retry");
        let retry_store = ProfileStore::new(&retry_dir);
        retry_store
            .save(&profile)
            .expect("retrying save with the same in-memory profile should succeed");
        let reloaded = retry_store
            .load(profile.profile_id)
            .expect("load should succeed after successful retry");
        assert_eq!(reloaded, profile);

        fs::remove_dir_all(&parent).ok();
        fs::remove_dir_all(&retry_dir).ok();
    }

    /// 集成测试（Req 5.3）：验证持久化路径在"实例/进程边界"之外依然正确——
    /// 使用真实临时目录，用一个 `ProfileStore` 实例 A 保存档案后将其 drop，
    /// 再用指向同一目录的全新 `ProfileStore` 实例 B 读取。`ProfileStore` 不
    /// 持有任何内存缓存，路径完全由 `base_dir` + `profile_id` 推导得出，因此
    /// 实例 B 无需依赖实例 A 的任何运行期状态即可还原数据，这正是应用重启、
    /// 设备重启后仍能找回学员档案的基础保证。
    ///
    /// 本测试聚焦 `ProfileStore` 层本身（路径解析 + 文件持久化），
    /// 与 `app_state::profile_session` 中 `ProfileManager` 级别的重启恢复测试
    /// （`startup_load_discovers_profiles_saved_in_previous_session`，覆盖档案
    /// 列表扫描等更上层行为）互补而不重复。
    #[test]
    fn save_survives_across_fresh_store_instances_in_real_temp_dir() {
        let dir = unique_temp_dir("cross-instance-restart");
        let profile = sample_profile();

        {
            // 实例 A：保存后立即 drop，模拟应用退出前的最后一次写入。
            let store_a = ProfileStore::new(&dir);
            store_a
                .save(&profile)
                .expect("save via instance A should succeed");
        }

        // 实例 B：全新构造，仅通过相同的真实目录路径与实例 A 关联，
        // 不共享任何内存状态（模拟应用重启后的进程）。
        let store_b = ProfileStore::new(&dir);
        let loaded = store_b
            .load(profile.profile_id)
            .expect("load via a freshly constructed instance should succeed");

        assert_eq!(loaded, profile);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn default_macos_dir_matches_design_path() {
        let home = std::env::var("HOME").expect("HOME should be set in test environment");
        let expected = PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("typing")
            .join("profiles");

        assert_eq!(ProfileStore::default_macos_dir().unwrap(), expected);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::storage::schema::LearningProgress;
    use chrono::{DateTime, Utc};
    use proptest::prelude::*;
    use std::collections::HashSet;
    use uuid::Uuid;

    /// 在系统临时目录下创建一个唯一的子目录用于测试，返回其路径。
    /// 调用方负责在测试结束时清理（每个测试使用独立子目录，避免相互干扰）。
    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "typing-profile-store-test-{label}-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("failed to create temp dir for test");
        dir
    }

    /// 生成任意 `ProfileId`：由随机 u128 构造 UUID。
    fn arb_profile_id() -> impl Strategy<Value = ProfileId> {
        any::<u128>().prop_map(|bits| ProfileId(Uuid::from_u128(bits)))
    }

    /// 生成任意 `DateTime<Utc>`：由 unix 时间戳（秒）构造，范围覆盖合理区间。
    fn arb_datetime() -> impl Strategy<Value = DateTime<Utc>> {
        (0i64..=4_102_444_800i64).prop_map(|secs| {
            DateTime::<Utc>::from_timestamp(secs, 0).expect("valid unix timestamp")
        })
    }

    /// 生成任意合法的 `LearnerProfile`：昵称长度 1-20 字符（Req 6.5），学习进度为
    /// 默认空进度（本属性关注多档案隔离性，进度内容细节由 Property 11 覆盖）。
    fn arb_learner_profile() -> impl Strategy<Value = LearnerProfile> {
        (arb_profile_id(), "[\\p{L}0-9 ]{1,20}", arb_datetime()).prop_map(
            |(profile_id, nickname, created_at)| LearnerProfile {
                profile_id,
                nickname,
                created_at,
                progress: LearningProgress::default(),
            },
        )
    }

    /// 生成 2-5 个拥有互不相同 `profile_id` 的 `LearnerProfile`。
    fn arb_distinct_learner_profiles() -> impl Strategy<Value = Vec<LearnerProfile>> {
        prop::collection::vec(arb_learner_profile(), 2..=5).prop_map(|mut profiles| {
            // 强制去重 profile_id：若随机生成中出现碰撞（理论上概率极低），
            // 为碰撞的档案重新分配一个基于其在列表中位置的唯一 id，保证
            // 该属性测试始终作用于"各自拥有不同 profile_id"的档案集合。
            let mut seen = HashSet::new();
            for profile in profiles.iter_mut() {
                while !seen.insert(profile.profile_id) {
                    profile.profile_id = ProfileId::new();
                }
            }
            profiles
        })
    }

    proptest! {
        // Feature: typing-desktop-app, Property 12: 多学员档案的隔离性
        #[test]
        fn prop_multiple_profiles_are_isolated(profiles in arb_distinct_learner_profiles()) {
            let dir = unique_temp_dir(&format!("prop-isolation-{}", Uuid::new_v4()));
            let store = ProfileStore::new(&dir);

            for profile in &profiles {
                store.save(profile).expect("save should succeed");
            }

            // 逐一按 profile_id 加载，验证内容与其对应原始档案完全一致，
            // 不受其他档案存在/内容的影响（无交叉污染）。
            for profile in &profiles {
                let loaded = store
                    .load(profile.profile_id)
                    .expect("load should succeed for a saved profile");
                prop_assert_eq!(&loaded, profile);
            }

            // 档案选择界面展示的昵称列表恰好与已保存档案集合的昵称一一对应：
            // 无遗漏、无多余、无重复归属。
            let expected_ids: HashSet<ProfileId> =
                profiles.iter().map(|p| p.profile_id).collect();
            let loaded_ids: HashSet<ProfileId> = profiles
                .iter()
                .map(|p| store.load(p.profile_id).unwrap().profile_id)
                .collect();
            prop_assert_eq!(expected_ids.len(), profiles.len());
            prop_assert_eq!(loaded_ids, expected_ids);

            fs::remove_dir_all(&dir).ok();
        }
    }
}
