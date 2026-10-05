//! 存储文件的序列化数据结构（serde）。
//!
//! 本模块定义学员档案在本机文件系统中持久化的数据结构，与 `domain` 层的课程静态
//! 定义完全解耦（Req 5.6：学习进度记录关联唯一学员档案标识，避免多档案数据相互覆盖）。

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 学员档案唯一标识（UUID），创建时生成，同时用作档案文件名（Req 5.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProfileId(pub Uuid);

impl ProfileId {
    /// 生成一个新的随机档案标识。
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

/// 课程唯一标识，用作学习进度中按课程索引成绩记录的键。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LessonId(pub String);

/// 学员档案：持久化存储的顶层数据结构，每个档案对应一个存储文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearnerProfile {
    /// UUID，创建时生成，唯一标识（Req 5.6）。
    pub profile_id: ProfileId,
    /// 1-20 字符（Req 6.5）。
    pub nickname: String,
    pub created_at: DateTime<Utc>,
    pub progress: LearningProgress,
}

/// 学习进度：某学员档案在课程序列中的完成情况记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LearningProgress {
    pub lesson_records: HashMap<LessonId, LessonRecord>,
    pub unlocked_lesson_ids: HashSet<LessonId>,
}

/// 单个课程的成绩记录，包含冗余缓存的最佳成绩字段与完整历史记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LessonRecord {
    pub best_accuracy: f32,
    pub best_wpm: f32,
    pub best_score: u32,
    pub achieved_at: DateTime<Utc>,
    pub attempt_count: u32,
    /// 每次练习成绩（Req 5.1）。
    pub history: Vec<PracticeResult>,
}

/// 单次练习环节产出的成绩结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PracticeResult {
    pub lesson_id: LessonId,
    /// 0.0-100.0，1位小数。
    pub accuracy: f32,
    /// 1位小数。
    pub wpm: f32,
    pub error_count: u32,
    pub duration_ms: u64,
    pub score: u32,
    pub completed_at: DateTime<Utc>,
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    /// 生成任意合法的 `LessonId`：非空、有限长度的字符串。
    fn arb_lesson_id() -> impl Strategy<Value = LessonId> {
        "[a-zA-Z0-9_]{1,20}".prop_map(LessonId)
    }

    /// 生成任意 `ProfileId`：由随机 u128 构造 UUID。
    fn arb_profile_id() -> impl Strategy<Value = ProfileId> {
        any::<u128>().prop_map(|bits| ProfileId(Uuid::from_u128(bits)))
    }

    /// 生成任意 `DateTime<Utc>`：由 unix 时间戳（秒）构造，范围覆盖过去/未来的合理区间。
    fn arb_datetime() -> impl Strategy<Value = DateTime<Utc>> {
        (0i64..=4_102_444_800i64).prop_map(|secs| {
            DateTime::<Utc>::from_timestamp(secs, 0).expect("valid unix timestamp")
        })
    }

    /// 生成任意合法的正确率数值：有限浮点数，范围 [0.0, 100.0]。
    fn arb_accuracy() -> impl Strategy<Value = f32> {
        0.0f32..=100.0f32
    }

    /// 生成任意合法的打字速度数值：有限浮点数，非负。
    fn arb_wpm() -> impl Strategy<Value = f32> {
        0.0f32..=500.0f32
    }

    fn arb_practice_result() -> impl Strategy<Value = PracticeResult> {
        (
            arb_lesson_id(),
            arb_accuracy(),
            arb_wpm(),
            any::<u32>(),
            any::<u64>(),
            any::<u32>(),
            arb_datetime(),
        )
            .prop_map(
                |(lesson_id, accuracy, wpm, error_count, duration_ms, score, completed_at)| {
                    PracticeResult {
                        lesson_id,
                        accuracy,
                        wpm,
                        error_count,
                        duration_ms,
                        score,
                        completed_at,
                    }
                },
            )
    }

    fn arb_lesson_record() -> impl Strategy<Value = LessonRecord> {
        (
            arb_accuracy(),
            arb_wpm(),
            any::<u32>(),
            arb_datetime(),
            any::<u32>(),
            prop::collection::vec(arb_practice_result(), 0..5),
        )
            .prop_map(
                |(best_accuracy, best_wpm, best_score, achieved_at, attempt_count, history)| {
                    LessonRecord {
                        best_accuracy,
                        best_wpm,
                        best_score,
                        achieved_at,
                        attempt_count,
                        history,
                    }
                },
            )
    }

    fn arb_learning_progress() -> impl Strategy<Value = LearningProgress> {
        (
            prop::collection::vec((arb_lesson_id(), arb_lesson_record()), 0..5),
            prop::collection::vec(arb_lesson_id(), 0..5),
        )
            .prop_map(|(records, unlocked)| LearningProgress {
                lesson_records: records.into_iter().collect(),
                unlocked_lesson_ids: unlocked.into_iter().collect(),
            })
    }

    /// 生成任意合法的 `LearnerProfile`：昵称长度 1-20 字符（Req 6.5），
    /// 学习进度包含任意数量的课程记录与解锁状态。
    fn arb_learner_profile() -> impl Strategy<Value = LearnerProfile> {
        (
            arb_profile_id(),
            "[\\p{L}0-9 ]{1,20}",
            arb_datetime(),
            arb_learning_progress(),
        )
            .prop_map(
                |(profile_id, nickname, created_at, progress)| LearnerProfile {
                    profile_id,
                    nickname,
                    created_at,
                    progress,
                },
            )
    }

    proptest! {
        // Feature: typing-desktop-app, Property 11: 学员档案序列化往返一致性
        #[test]
        fn prop_learner_profile_serde_round_trip(profile in arb_learner_profile()) {
            let json = serde_json::to_string(&profile).expect("serialization should succeed");
            let deserialized: LearnerProfile =
                serde_json::from_str(&json).expect("deserialization should succeed");

            prop_assert_eq!(deserialized, profile);
        }
    }
}
