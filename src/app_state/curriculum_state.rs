//! CurriculumEngine：课程解锁状态管理。
//!
//! `CurriculumState` 持有应用内置的 `Curriculum` 静态数据，并维护当前学员的
//! 课程解锁状态视图：
//! - 加载/构造时对每个课程运行 `validate_lesson_config`（Req 1.8, 8.4），
//!   校验失败的课程被标记为 `LessonState::Unavailable(reason)`，不参与
//!   `compute_lesson_states` 的 Locked/Unlocked 判定，也不影响其余课程的
//!   序列位置与判定链条（设计文档 Error Handling："其余课程状态不受影响，
//!   不重新计算整个序列"）。
//! - 校验通过的课程由 `compute_lesson_states` 依据学员的 `Learning_Progress`
//!   计算 Locked/Unlocked 状态。
//! - `refresh` 用于学员的 `Learning_Progress` 发生变化（如完成一次练习）后
//!   重新计算整体视图。

use crate::app_state::profile_session::{ProfileError, ProfileManager};
use crate::domain::{
    self, Curriculum, Learning_Progress, Lesson, LessonId, LessonState, UnlockCriteria,
    compute_lesson_states, meets_unlock_criteria, validate_lesson_config,
};
use crate::storage::{self, LearnerProfile};

/// 课程状态管理：持有内置课程序列，并维护当前学员在该序列下的解锁状态视图。
pub struct CurriculumState {
    curriculum: Curriculum,
    /// 与 `curriculum.lessons` 一一对应、顺序一致的最终课程状态视图
    /// （已合并配置校验的 `Unavailable` 覆盖与学习进度维度的 Locked/Unlocked 判定）。
    lesson_states: Vec<LessonState>,
}

/// `CurriculumState::select_lesson` 的失败结果（Req 1.7, 1.8）。
#[derive(Debug, Clone, PartialEq)]
pub enum LessonSelectionError {
    /// 目标课程当前处于 `Locked` 状态，附带真正约束其解锁的"门槛课程"
    /// （即课程序列中紧邻的前一课程）及其解锁条件，供调用方构造
    /// "该课程尚未解锁及其解锁条件"提示文案（Req 1.7）。
    Locked {
        lesson_id: LessonId,
        gating_lesson_id: LessonId,
        gating_lesson_unlock_criteria: UnlockCriteria,
    },
    /// 目标课程配置缺失/损坏或存在冲突，当前不可用（Req 1.8）。
    Unavailable { lesson_id: LessonId, reason: String },
    /// `lesson_id` 不在当前课程序列中。
    NotFound { lesson_id: LessonId },
}

impl CurriculumState {
    /// 使用给定的内置课程序列与学员当前学习进度构造 `CurriculumState`。
    ///
    /// 构造时立即执行一次完整的状态计算（等价于 `refresh`），因此构造完成后
    /// `lesson_states` 即为可直接供 UI 使用的视图。
    pub fn new(curriculum: Curriculum, progress: &Learning_Progress) -> Self {
        let mut state = Self {
            curriculum,
            lesson_states: Vec::new(),
        };
        state.refresh(progress);
        state
    }

    /// 重新计算当前学员的课程解锁状态视图（如练习完成后 `Learning_Progress`
    /// 发生变化时调用，Req 1.4, 1.5）。
    ///
    /// 计算步骤：
    /// 1. 对每个课程运行 `validate_lesson_config`；校验失败的课程直接标记为
    ///    `LessonState::Unavailable(reason)`，不进入下一步判定。
    /// 2. 对校验通过的课程，构造一个仅包含这些课程的"合法子序列"，交给
    ///    `compute_lesson_states` 按 Learning_Progress 计算 Locked/Unlocked
    ///    （合法子序列内部保持原有的相对顺序，解锁判定链条只在合法课程之间
    ///    传递，不因某个课程被标记 Unavailable 而中断后续合法课程的判定）。
    /// 3. 将计算结果按原始课程顺序重新组装为与 `curriculum.lessons` 一一对应
    ///    的最终 `lesson_states`。
    pub fn refresh(&mut self, progress: &Learning_Progress) {
        let validity: Vec<Result<(), String>> = self
            .curriculum
            .lessons
            .iter()
            .map(|lesson| validate_lesson_config(lesson).map_err(|err| err.to_string()))
            .collect();

        let valid_lessons: Vec<Lesson> = self
            .curriculum
            .lessons
            .iter()
            .zip(validity.iter())
            .filter(|(_, result)| result.is_ok())
            .map(|(lesson, _)| lesson.clone())
            .collect();

        let valid_curriculum = Curriculum {
            lessons: valid_lessons,
        };
        let mut computed_states = compute_lesson_states(&valid_curriculum, progress).into_iter();

        self.lesson_states = validity
            .into_iter()
            .map(|result| match result {
                Err(reason) => LessonState::Unavailable(reason),
                Ok(()) => computed_states
                    .next()
                    .expect("每个校验通过的课程都应有对应的计算状态"),
            })
            .collect();
    }

    /// 当前学员的课程状态视图：与内置课程序列 `lessons` 一一对应、顺序一致。
    pub fn lesson_states(&self) -> &[LessonState] {
        &self.lesson_states
    }

    /// 查询指定课程当前的状态；若 `lesson_id` 不在课程序列中，返回 `None`。
    ///
    /// UI 侧渲染课程序列走的是 `lesson_states()`（整段一次性取出，与
    /// `lessons` 顺序一一对应），因此当前没有按 id 单点查询的调用方；本方法
    /// 是本层"按 id 查状态"的公开 API 且有单测覆盖，故保留。`cfg_attr` 只
    /// 压制非测试构建的 dead_code 提示——若单测被删除，测试构建会重新报出。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn state_of(&self, lesson_id: &LessonId) -> Option<&LessonState> {
        self.curriculum
            .lessons
            .iter()
            .position(|lesson| &lesson.id == lesson_id)
            .and_then(|index| self.lesson_states.get(index))
    }

    /// 学员在课程序列界面选择某个课程时的判定入口（Req 1.6, 1.7）。
    ///
    /// - 若该课程当前状态为 `Unlocked`：返回 `Ok(&Lesson)`，供调用方（未来的
    ///   `AppController::dispatch`）据此构造进入练习环节的导航命令
    ///   （Req 1.6：2 秒内进入该课程对应的练习环节——本方法只负责判定与信息
    ///   返回，实际的计时/导航命令构造属于后续 UI 命令生成任务）。
    /// - 若该课程当前状态为 `Locked`：返回
    ///   `Err(LessonSelectionError::Locked { lesson, gating_lesson_unlock_criteria })`，
    ///   附带该课程自身的 `unlock_criteria`；由于本课程序列是线性递进关系，
    ///   实际需要满足的解锁条件来自其前一课程（`gating_lesson`/
    ///   `gating_lesson_unlock_criteria`——即"上一课程需达成的解锁条件"，
    ///   若该课程是序列首课则不存在门槛课程），调用方可据此拼出"该课程尚未
    ///   解锁及其解锁条件"提示文案（Req 1.7）。
    /// - 若该课程当前状态为 `Unavailable(reason)`：返回
    ///   `Err(LessonSelectionError::Unavailable { lesson, reason })`（Req 1.8）。
    /// - 若 `lesson_id` 不在课程序列中：返回 `Err(LessonSelectionError::NotFound)`。
    pub fn select_lesson(
        &self,
        lesson_id: &LessonId,
    ) -> Result<&Lesson, LessonSelectionError> {
        let Some(index) = self
            .curriculum
            .lessons
            .iter()
            .position(|lesson| &lesson.id == lesson_id)
        else {
            return Err(LessonSelectionError::NotFound {
                lesson_id: lesson_id.clone(),
            });
        };

        let lesson = &self.curriculum.lessons[index];
        match &self.lesson_states[index] {
            LessonState::Unlocked => Ok(lesson),
            LessonState::Locked => {
                // 序列首课不可能处于 Locked（compute_lesson_states 恒将其判定
                // 为 Unlocked），因此此处必然存在“上一课程”作为门槛课程。
                let gating_lesson = &self.curriculum.lessons[index - 1];
                Err(LessonSelectionError::Locked {
                    lesson_id: lesson.id.clone(),
                    gating_lesson_id: gating_lesson.id.clone(),
                    gating_lesson_unlock_criteria: gating_lesson.unlock_criteria,
                })
            }
            LessonState::Unavailable(reason) => Err(LessonSelectionError::Unavailable {
                lesson_id: lesson.id.clone(),
                reason: reason.clone(),
            }),
        }
    }

    /// 练习完成后的核心流程（Req 1.4, 1.5）：将本次 `PracticeResult` 记录到
    /// 学员档案的学习进度中，重新计算课程解锁状态，并驱动 `ProfileManager`
    /// 将变更后的档案落盘。
    ///
    /// 处理步骤：
    /// 1. 将本次成绩追加到 `profile.progress` 中对应课程的 `LessonRecord`
    ///    （不存在则新建），更新 `history`、`attempt_count` 与 `best_*` 冗余
    ///    缓存字段（`best_*` 取历史最高正确率对应的记录，正确率相同时取更高
    ///    的打字速度）——无论本次是否达标，成绩记录都会被保留（Req 1.5）。
    /// 2. 依据课程自身的 `unlock_criteria` 与更新后的 `attempt_count` 调用
    ///    `meets_unlock_criteria` 判定：满足条件且存在下一课程时，将下一课程
    ///    id 加入 `profile.progress.unlocked_lesson_ids`（Req 1.4）；不满足时
    ///    不做任何解锁状态变更，下一课程保持未解锁（Req 1.5）。
    /// 3. 调用 `refresh` 基于更新后的学习进度重新计算 `lesson_states` 视图。
    /// 4. 通过 `profile_manager.save_progress` 将变更后的 `profile` 落盘。
    ///
    /// 若 `lesson_id` 不在当前课程序列中，本方法不修改任何状态并返回
    /// `Ok(())`（无对应课程可供记录/解锁判定，视为无操作）。
    ///
    /// 落盘失败（如磁盘写入错误）时返回 `Err(ProfileError::SaveFailed(..))`，
    /// 但 `profile` 与 `self.lesson_states` 中的内存状态变更已经生效并保留
    /// （Req 5.5：保存失败不清空调用方内存中的学习进度数据）。
    pub fn record_practice_result(
        &mut self,
        profile: &mut LearnerProfile,
        result: storage::PracticeResult,
        profile_manager: &mut ProfileManager,
    ) -> Result<(), ProfileError> {
        let Some(lesson_index) = self
            .curriculum
            .lessons
            .iter()
            .position(|lesson| lesson.id.0 == result.lesson_id.0)
        else {
            return Ok(());
        };
        let lesson = self.curriculum.lessons[lesson_index].clone();

        let attempt_count = upsert_lesson_record(&mut profile.progress, &result);

        let domain_result = to_domain_practice_result(&result);
        if meets_unlock_criteria(&domain_result, &lesson.unlock_criteria, attempt_count)
            && let Some(next_lesson) = self.curriculum.lessons.get(lesson_index + 1)
        {
            profile
                .progress
                .unlocked_lesson_ids
                .insert(storage::LessonId(next_lesson.id.0.clone()));
        }

        let domain_progress = to_domain_progress(&profile.progress);
        self.refresh(&domain_progress);

        profile_manager.save_progress(profile)
    }
}

/// 将一条 `storage::PracticeResult` 合并进 `progress` 中对应课程的
/// `LessonRecord`（不存在则新建），返回合并后该课程的累计练习次数
/// （`attempt_count`，供 `meets_unlock_criteria` 的 `min_attempts` 门槛判定
/// 使用）。
///
/// 无论本次成绩是否达标，记录都会被追加到 `history` 并保留（Req 1.5）；
/// `best_*` 字段按"正确率优先，相同时比较打字速度"的规则更新为历史最优。
fn upsert_lesson_record(
    progress: &mut storage::LearningProgress,
    result: &storage::PracticeResult,
) -> u32 {
    let record = progress
        .lesson_records
        .entry(result.lesson_id.clone())
        .or_insert_with(|| storage::LessonRecord {
            best_accuracy: result.accuracy,
            best_wpm: result.wpm,
            best_score: result.score,
            achieved_at: result.completed_at,
            attempt_count: 0,
            history: Vec::new(),
        });

    let is_new_best = result.accuracy > record.best_accuracy
        || (result.accuracy == record.best_accuracy && result.wpm > record.best_wpm);
    if is_new_best {
        record.best_accuracy = result.accuracy;
        record.best_wpm = result.wpm;
        record.best_score = result.score;
        record.achieved_at = result.completed_at;
    }

    record.attempt_count += 1;
    record.history.push(result.clone());

    record.attempt_count
}

/// 将持久化层的 `storage::PracticeResult` 转换为领域层的 `domain::PracticeResult`
/// （字段一一对应，仅 `lesson_id` 需要在两个同构但不同类型的 `LessonId` 之间转换）。
fn to_domain_practice_result(result: &storage::PracticeResult) -> domain::PracticeResult {
    domain::PracticeResult {
        lesson_id: LessonId(result.lesson_id.0.clone()),
        accuracy: result.accuracy,
        wpm: result.wpm,
        error_count: result.error_count,
        duration_ms: result.duration_ms,
        score: result.score,
    }
}

/// 将持久化层的 `storage::LearningProgress` 转换为领域层的 `domain::Learning_Progress`
/// （供 `compute_lesson_states`/`refresh` 使用；领域层不依赖 `serde`/`chrono`，
/// 因此需要在两层同构的数据结构之间显式转换）。
fn to_domain_progress(progress: &storage::LearningProgress) -> Learning_Progress {
    Learning_Progress {
        lesson_records: progress
            .lesson_records
            .iter()
            .map(|(id, record)| {
                let latest =
                    record
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
                (LessonId(id.0.clone()), to_domain_practice_result(&latest))
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
    use crate::domain::{KeyCode, LessonGoal, UnlockCriteria};
    use proptest::prelude::*;

    fn valid_single_key_lesson(id: &str, min_accuracy: Option<f32>) -> Lesson {
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

    /// 配置无效的课程：`Word` 目标但目标键位仅有 `Space`，排除分隔空格后无任何
    /// 可用字符，理论上无法组词（对应 `validate_lesson_config` 的
    /// `InsufficientKeysForGoal`）。
    fn invalid_word_lesson(id: &str) -> Lesson {
        Lesson {
            id: LessonId(id.to_string()),
            title: format!("无效课程 {id}"),
            goal: LessonGoal::Word,
            target_keys: vec![KeyCode::Space],
            requires_shift: false,
            unlock_criteria: UnlockCriteria {
                min_accuracy: Some(90.0),
                max_duration_secs: None,
                min_attempts: None,
            },
        }
    }

    fn practice_result(lesson_id: &str, accuracy: f32) -> crate::domain::PracticeResult {
        crate::domain::PracticeResult {
            lesson_id: LessonId(lesson_id.to_string()),
            accuracy,
            wpm: 0.0,
            error_count: 0,
            duration_ms: 0,
            score: 0,
        }
    }

    #[test]
    fn all_valid_lessons_produce_normal_locked_unlocked_states() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
                valid_single_key_lesson("lesson-3", Some(90.0)),
            ],
        };
        let progress = Learning_Progress::default();

        let state = CurriculumState::new(curriculum, &progress);

        assert_eq!(
            state.lesson_states(),
            &[
                LessonState::Unlocked,
                LessonState::Locked,
                LessonState::Locked
            ]
        );
    }

    #[test]
    fn invalid_lesson_config_is_marked_unavailable_and_does_not_block_others() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                invalid_word_lesson("lesson-2-broken"),
                valid_single_key_lesson("lesson-3", Some(90.0)),
            ],
        };
        let progress = Learning_Progress::default();

        let state = CurriculumState::new(curriculum, &progress);
        let states = state.lesson_states();

        assert_eq!(states.len(), 3);
        // lesson-1 恒为序列首课，正常 Unlocked。
        assert_eq!(states[0], LessonState::Unlocked);
        // lesson-2 配置无效，标记为 Unavailable 而非 Locked/Unlocked。
        assert!(matches!(states[1], LessonState::Unavailable(_)));
        // lesson-3 在"合法子序列"中紧邻 lesson-1（唯一另一个合法课程），
        // 由于 lesson-1 尚无达成记录，lesson-3 应为 Locked（而不是因
        // lesson-2 被跳过而意外变为 Unlocked，也不应因整条序列被判定
        // 为损坏而一并变为 Unavailable）。
        assert_eq!(states[2], LessonState::Locked);
    }

    #[test]
    fn valid_lesson_after_unavailable_one_can_still_unlock_via_progress() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                invalid_word_lesson("lesson-2-broken"),
                valid_single_key_lesson("lesson-3", Some(90.0)),
            ],
        };
        let mut progress = Learning_Progress::default();
        progress.lesson_records.insert(
            LessonId("lesson-1".to_string()),
            practice_result("lesson-1", 95.0),
        );

        let state = CurriculumState::new(curriculum, &progress);
        let states = state.lesson_states();

        assert_eq!(states[0], LessonState::Unlocked);
        assert!(matches!(states[1], LessonState::Unavailable(_)));
        // lesson-1 达标后，合法子序列中紧邻的下一课（lesson-3）应解锁。
        assert_eq!(states[2], LessonState::Unlocked);
    }

    #[test]
    fn state_of_looks_up_by_lesson_id() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
            ],
        };
        let progress = Learning_Progress::default();

        let state = CurriculumState::new(curriculum, &progress);

        assert_eq!(
            state.state_of(&LessonId("lesson-1".to_string())),
            Some(&LessonState::Unlocked)
        );
        assert_eq!(
            state.state_of(&LessonId("lesson-2".to_string())),
            Some(&LessonState::Locked)
        );
        assert_eq!(state.state_of(&LessonId("unknown".to_string())), None);
    }

    #[test]
    fn refresh_recomputes_view_after_progress_changes() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
            ],
        };
        let mut progress = Learning_Progress::default();
        let mut state = CurriculumState::new(curriculum, &progress);

        assert_eq!(state.lesson_states()[1], LessonState::Locked);

        progress.lesson_records.insert(
            LessonId("lesson-1".to_string()),
            practice_result("lesson-1", 95.0),
        );
        state.refresh(&progress);

        assert_eq!(state.lesson_states()[1], LessonState::Unlocked);
    }

    /// 使用一个真实的临时目录构造 `ProfileManager`，并预先创建/持久化一个学员档案，
    /// 供 `record_practice_result` 相关测试驱动落盘验证使用。
    fn manager_with_profile(
        label: &str,
        nickname: &str,
    ) -> (std::path::PathBuf, ProfileManager, LearnerProfile) {
        let dir = std::env::temp_dir().join(format!(
            "typing-curriculum-state-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir for test");

        let store = crate::storage::ProfileStore::new(&dir);
        let mut manager = ProfileManager::new(store);
        let profile = manager
            .create_profile(nickname)
            .expect("seed profile creation should succeed");

        (dir, manager, profile)
    }

    fn storage_practice_result(lesson_id: &str, accuracy: f32) -> storage::PracticeResult {
        storage::PracticeResult {
            lesson_id: storage::LessonId(lesson_id.to_string()),
            accuracy,
            wpm: 30.0,
            error_count: 1,
            duration_ms: 5_000,
            score: 80,
            completed_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn record_practice_result_meeting_criteria_unlocks_next_lesson_and_persists() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
            ],
        };
        let (dir, mut manager, mut profile) = manager_with_profile("unlock-success", "小明");
        let mut state = CurriculumState::new(curriculum, &Learning_Progress::default());

        let result = storage_practice_result("lesson-1", 95.0);
        state
            .record_practice_result(&mut profile, result.clone(), &mut manager)
            .expect("record_practice_result should succeed");

        // 下一课程应被解锁，且反映在重新计算后的 lesson_states 视图中。
        assert_eq!(state.lesson_states()[0], LessonState::Unlocked);
        assert_eq!(state.lesson_states()[1], LessonState::Unlocked);
        assert!(
            profile
                .progress
                .unlocked_lesson_ids
                .contains(&storage::LessonId("lesson-2".to_string()))
        );

        // 成绩记录已写入并保留。
        let record = profile
            .progress
            .lesson_records
            .get(&storage::LessonId("lesson-1".to_string()))
            .expect("lesson record should exist");
        assert_eq!(record.attempt_count, 1);
        assert_eq!(record.history.len(), 1);
        assert_eq!(record.history[0], result);

        // 持久化生效：重新从磁盘加载应得到相同的学习进度。
        let reloaded = manager
            .select_profile(profile.profile_id)
            .expect("reload after save should succeed");
        assert_eq!(reloaded.progress, profile.progress);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_practice_result_failing_criteria_keeps_locked_but_persists_attempt() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
            ],
        };
        let (dir, mut manager, mut profile) = manager_with_profile("unlock-failure", "小红");
        let mut state = CurriculumState::new(curriculum, &Learning_Progress::default());

        let result = storage_practice_result("lesson-1", 50.0);
        state
            .record_practice_result(&mut profile, result.clone(), &mut manager)
            .expect("record_practice_result should succeed even when criteria are not met");

        // 下一课程保持未解锁。
        assert_eq!(state.lesson_states()[0], LessonState::Unlocked);
        assert_eq!(state.lesson_states()[1], LessonState::Locked);
        assert!(
            !profile
                .progress
                .unlocked_lesson_ids
                .contains(&storage::LessonId("lesson-2".to_string()))
        );

        // 未达标课程的成绩记录仍需保留，允许学员重新练习（Req 1.5）。
        let record = profile
            .progress
            .lesson_records
            .get(&storage::LessonId("lesson-1".to_string()))
            .expect("lesson record should be preserved even on failure");
        assert_eq!(record.attempt_count, 1);
        assert_eq!(record.history[0], result);

        // 持久化生效：即使未达标，练习记录也应写入磁盘。
        let reloaded = manager
            .select_profile(profile.profile_id)
            .expect("reload after save should succeed");
        assert_eq!(reloaded.progress, profile.progress);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_practice_result_accumulates_attempt_count_across_multiple_results() {
        let curriculum = Curriculum {
            lessons: vec![valid_single_key_lesson(
                "lesson-1", None, // 依赖 min_attempts 门槛而非正确率
            )],
        };
        let mut curriculum = curriculum;
        curriculum.lessons[0].unlock_criteria.min_attempts = Some(2);
        curriculum
            .lessons
            .push(valid_single_key_lesson("lesson-2", Some(0.0)));

        let (dir, mut manager, mut profile) = manager_with_profile("attempt-count", "小刚");
        let mut state = CurriculumState::new(curriculum, &Learning_Progress::default());

        // 第一次练习：次数不足，下一课仍锁定。
        state
            .record_practice_result(
                &mut profile,
                storage_practice_result("lesson-1", 10.0),
                &mut manager,
            )
            .expect("first record should succeed");
        assert_eq!(state.lesson_states()[1], LessonState::Locked);

        // 第二次练习：累计次数达到 2，满足 min_attempts 门槛，下一课解锁。
        state
            .record_practice_result(
                &mut profile,
                storage_practice_result("lesson-1", 10.0),
                &mut manager,
            )
            .expect("second record should succeed");
        assert_eq!(state.lesson_states()[1], LessonState::Unlocked);

        let record = profile
            .progress
            .lesson_records
            .get(&storage::LessonId("lesson-1".to_string()))
            .expect("lesson record should exist");
        assert_eq!(record.attempt_count, 2);
        assert_eq!(record.history.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 集成测试（Req 1.6, 1.7）：选择已解锁课程应成功返回该课程，供调用方
    /// 构造进入练习环节的导航命令；选择未解锁课程应被阻止，并返回携带
    /// 解锁条件信息的错误，供调用方构造"尚未解锁及其解锁条件"提示。
    #[test]
    fn select_lesson_on_unlocked_lesson_succeeds() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
            ],
        };
        let progress = Learning_Progress::default();
        let state = CurriculumState::new(curriculum, &progress);

        // lesson-1 是序列首课，恒为 Unlocked（Req 1.2），选择应成功。
        let selected = state
            .select_lesson(&LessonId("lesson-1".to_string()))
            .expect("selecting an unlocked lesson should succeed");
        assert_eq!(selected.id, LessonId("lesson-1".to_string()));
    }

    #[test]
    fn select_lesson_on_locked_lesson_is_blocked_with_unlock_criteria() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
            ],
        };
        let progress = Learning_Progress::default();
        let state = CurriculumState::new(curriculum, &progress);

        // lesson-2 尚未解锁（lesson-1 没有任何达成记录）。
        let err = state
            .select_lesson(&LessonId("lesson-2".to_string()))
            .expect_err("selecting a locked lesson should be blocked");

        match err {
            LessonSelectionError::Locked {
                lesson_id,
                gating_lesson_id,
                gating_lesson_unlock_criteria,
            } => {
                assert_eq!(lesson_id, LessonId("lesson-2".to_string()));
                // 门槛课程是紧邻的前一课程（lesson-1），提示文案需要展示
                // 其解锁条件，而不是 lesson-2 自身的解锁条件。
                assert_eq!(gating_lesson_id, LessonId("lesson-1".to_string()));
                assert_eq!(gating_lesson_unlock_criteria.min_accuracy, Some(90.0));
            }
            other => panic!("expected LessonSelectionError::Locked, got {other:?}"),
        }
    }

    #[test]
    fn select_lesson_on_unknown_id_returns_not_found() {
        let curriculum = Curriculum {
            lessons: vec![valid_single_key_lesson("lesson-1", Some(90.0))],
        };
        let progress = Learning_Progress::default();
        let state = CurriculumState::new(curriculum, &progress);

        let err = state
            .select_lesson(&LessonId("unknown-lesson".to_string()))
            .expect_err("selecting an unknown lesson id should fail");

        assert_eq!(
            err,
            LessonSelectionError::NotFound {
                lesson_id: LessonId("unknown-lesson".to_string())
            }
        );
    }

    #[test]
    fn select_lesson_on_unavailable_lesson_returns_unavailable_with_reason() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                invalid_word_lesson("lesson-2-broken"),
            ],
        };
        let progress = Learning_Progress::default();
        let state = CurriculumState::new(curriculum, &progress);

        let err = state
            .select_lesson(&LessonId("lesson-2-broken".to_string()))
            .expect_err("selecting an unavailable lesson should be blocked");

        match err {
            LessonSelectionError::Unavailable { lesson_id, reason } => {
                assert_eq!(lesson_id, LessonId("lesson-2-broken".to_string()));
                assert!(!reason.is_empty());
            }
            other => panic!("expected LessonSelectionError::Unavailable, got {other:?}"),
        }
    }

    /// 生成任意合法的练习成绩样例（accuracy/wpm/error_count/duration_ms 均在
    /// 合理范围内取任意值），供 `prop_record_practice_result_persistence_integrity`
    /// 组装任意长度的成绩序列。`lesson_id`/`completed_at` 固定，因为该属性关注
    /// 的是"同一课程连续多次记录"的完整性，与具体 id/时间取值无关。
    fn arb_result_for_lesson(
        lesson_id: &'static str,
    ) -> impl Strategy<Value = storage::PracticeResult> {
        (0.0f32..=100.0f32, 0.0f32..=300.0f32, 0u32..=50, 100u64..=600_000).prop_map(
            move |(accuracy, wpm, error_count, duration_ms)| storage::PracticeResult {
                lesson_id: storage::LessonId(lesson_id.to_string()),
                accuracy,
                wpm,
                error_count,
                duration_ms,
                score: (accuracy * 10.0) as u32,
                completed_at: chrono::Utc::now(),
            },
        )
    }

    // Feature: typing-desktop-app, Property 10: 练习成绩记录写入的完整性
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(15))]
        #[test]
        fn prop_record_practice_result_persistence_integrity(
            results in prop::collection::vec(arb_result_for_lesson("lesson-1"), 1..8)
        ) {
            let curriculum = Curriculum {
                lessons: vec![valid_single_key_lesson("lesson-1", Some(90.0))],
            };
            let (dir, mut manager, mut profile) =
                manager_with_profile(&format!("pbt-record-{}", uuid::Uuid::new_v4()), "小明");
            let mut state = CurriculumState::new(curriculum, &Learning_Progress::default());

            let mut expected_best_accuracy = f32::MIN;
            let mut expected_best_wpm = f32::MIN;
            let mut expected_best_score: u32 = 0;

            for (i, result) in results.iter().enumerate() {
                state
                    .record_practice_result(&mut profile, result.clone(), &mut manager)
                    .expect("record_practice_result should succeed for arbitrary valid results");

                let record = profile
                    .progress
                    .lesson_records
                    .get(&storage::LessonId("lesson-1".to_string()))
                    .expect("lesson record should exist after at least one recorded result");

                // 每条记录按提交顺序原样出现在 history 中，不丢失、不重排。
                prop_assert_eq!(record.history.len(), i + 1);
                prop_assert_eq!(&record.history[i], result);
                prop_assert_eq!(record.history.as_slice(), &results[..=i]);

                // attempt_count 恒等于目前已记录的成绩数量。
                prop_assert_eq!(record.attempt_count, (i + 1) as u32);

                // best_* 恒反映"正确率优先，相同时比较打字速度"规则下的历史最优。
                let is_new_best = result.accuracy > expected_best_accuracy
                    || (result.accuracy == expected_best_accuracy && result.wpm > expected_best_wpm);
                if is_new_best {
                    expected_best_accuracy = result.accuracy;
                    expected_best_wpm = result.wpm;
                    expected_best_score = result.score;
                }
                prop_assert_eq!(record.best_accuracy, expected_best_accuracy);
                prop_assert_eq!(record.best_wpm, expected_best_wpm);
                prop_assert_eq!(record.best_score, expected_best_score);

                // 每次调用都实际落盘：重新从磁盘加载得到的进度与内存中完全一致。
                let reloaded = manager
                    .select_profile(profile.profile_id)
                    .expect("reload after save should succeed");
                prop_assert_eq!(reloaded.progress, profile.progress.clone());
            }

            std::fs::remove_dir_all(&dir).ok();
        }
    }

    /// 集成测试（Req 5.1）：练习完成后成绩写入学习进度的耗时验证。
    ///
    /// `record_practice_result` 内部会调用 `upsert_lesson_record`（内存操作）
    /// 与 `ProfileManager::save_progress`（真实磁盘 I/O，序列化并写入
    /// `~/.../{profile_id}.json` 对应的临时目录文件）。Req 5.1 要求该次写入
    /// 在 3 秒内完成；本测试使用真实临时目录与真实文件系统写入，测量单次
    /// 提交的墙钟耗时，断言其远低于 3 秒（预期应为毫秒级，此处给予充裕的
    /// 多秒级余量以避免在较慢的 CI 环境下出现误报，同时仍能捕捉到真正的
    /// 性能退化）。
    #[test]
    fn record_practice_result_completes_within_three_second_budget() {
        let curriculum = Curriculum {
            lessons: vec![
                valid_single_key_lesson("lesson-1", Some(90.0)),
                valid_single_key_lesson("lesson-2", Some(90.0)),
            ],
        };
        let (dir, mut manager, mut profile) = manager_with_profile("timing-record", "小时");
        let mut state = CurriculumState::new(curriculum, &Learning_Progress::default());

        let result = storage_practice_result("lesson-1", 95.0);

        let started = std::time::Instant::now();
        state
            .record_practice_result(&mut profile, result, &mut manager)
            .expect("record_practice_result should succeed");
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "record_practice_result should persist within the 3s budget (Req 5.1), took {:?}",
            elapsed
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn all_lessons_invalid_marks_all_unavailable() {
        let curriculum = Curriculum {
            lessons: vec![
                invalid_word_lesson("lesson-1"),
                invalid_word_lesson("lesson-2"),
            ],
        };
        let progress = Learning_Progress::default();

        let state = CurriculumState::new(curriculum, &progress);

        assert!(
            state
                .lesson_states()
                .iter()
                .all(|s| matches!(s, LessonState::Unavailable(_)))
        );
    }
}
