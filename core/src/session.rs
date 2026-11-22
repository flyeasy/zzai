//! 会话状态机（DESIGN §4.5）
//!
//! `Idle → Starting → Listening → Processing → Inserting → Idle`
//!  - Starting 期松开：记 `pending_stop`，就绪后立即停（不要求用户再松一次）
//!  - Inserting 拒绝取消（防半截文本）；Processing 期取消 = 丢弃本次结果
//!  - 录音中再按：继续录，不取消、不重开
//!  - 提交结果未知 → 停在「待复制」，绝不自动重发

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Starting,
    Listening,
    Processing,
    Inserting,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Starting => "starting",
            Phase::Listening => "listening",
            Phase::Processing => "processing",
            Phase::Inserting => "inserting",
        }
    }
}

/// 启动到「音频就绪」的宽限（麦克风起流很快，留一点余量）
pub const READY_GRACE: Duration = Duration::from_millis(60);
/// 提交结果的等待上限；超时按 `unknown` 处理。
/// 这里必须给壳留够时间：Windows 壳的上屏要经历 UIA 尝试 + 剪贴板 + Ctrl+V + 回读校验，
/// 慢一点的机器（VM / 目标程序卡顿）实测 2s 不够，会把已经成功的上屏误判成超时（I-142）。
pub const SUBMIT_TIMEOUT: Duration = Duration::from_millis(6000);
/// 识别看门狗：超过它就报「识别卡住」，不让 UI 永远转圈
pub const DECODE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Machine {
    pub phase: Phase,
    pub session: u64,
    pub generation: u64,
    pub pending_stop: bool,
    pub ready_at: Option<Instant>,
    pub pending_submit: Option<(String, Instant)>,
    pub task_id: Option<String>,
    pub processing_since: Option<Instant>,
    /// 调试/自测路径：预先给定的上屏文本，绕过采音与识别
    pub prepared_text: Option<String>,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            session: 0,
            generation: 0,
            pending_stop: false,
            ready_at: None,
            pending_submit: None,
            task_id: None,
            prepared_text: None,
            processing_since: None,
        }
    }
}

#[derive(Debug)]
pub enum Step {
    None,
    SessionStarted,
    Listening,
    /// 松键：需要开始识别（返回任务号）
    Finish {
        task_id: String,
    },
    SubmitTimedOut {
        task_id: String,
    },
    DecodeStalled {
        task_id: String,
    },
    RejectedCancel,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted,
    StaleTaskId,
    NoPending,
}

impl Machine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn down(&mut self) -> Step {
        match self.phase {
            Phase::Idle => {
                self.session += 1;
                self.generation += 1;
                self.phase = Phase::Starting;
                self.pending_stop = false;
                self.ready_at = Some(Instant::now() + READY_GRACE);
                Step::SessionStarted
            }
            // 录音中再按 = 继续录（不取消、不重开）
            _ => Step::None,
        }
    }

    pub fn up(&mut self) -> Step {
        match self.phase {
            Phase::Starting => {
                self.pending_stop = true;
                Step::None
            }
            Phase::Listening => self.begin_processing(),
            _ => Step::None,
        }
    }

    pub fn tick(&mut self, now: Instant) -> Step {
        match self.phase {
            Phase::Starting => {
                if self.ready_at.map(|t| now >= t).unwrap_or(true) {
                    self.ready_at = None;
                    self.phase = Phase::Listening;
                    Step::Listening
                } else {
                    Step::None
                }
            }
            Phase::Processing => {
                if let (Some(since), Some(task)) = (self.processing_since, self.task_id.clone()) {
                    if now.duration_since(since) > DECODE_TIMEOUT {
                        self.processing_since = None;
                        self.phase = Phase::Idle;
                        return Step::DecodeStalled { task_id: task };
                    }
                }
                Step::None
            }
            Phase::Inserting => {
                if let Some((task_id, deadline)) = self.pending_submit.clone() {
                    if now >= deadline {
                        self.pending_submit = None;
                        self.phase = Phase::Idle;
                        return Step::SubmitTimedOut { task_id };
                    }
                }
                Step::None
            }
            _ => Step::None,
        }
    }

    /// 就绪后如果已经松手，立刻进入识别
    pub fn take_pending_stop(&mut self) -> Step {
        if self.phase == Phase::Listening && self.pending_stop {
            self.pending_stop = false;
            return self.begin_processing();
        }
        Step::None
    }

    pub fn begin_processing(&mut self) -> Step {
        self.phase = Phase::Processing;
        self.processing_since = Some(Instant::now());
        let task_id = format!("t{}-{}", self.session, self.generation);
        self.task_id = Some(task_id.clone());
        Step::Finish { task_id }
    }

    /// 用暂存音频重试：Idle 直接进识别，不开麦（DESIGN §10.1 失败保留可重试）
    pub fn begin_retry(&mut self) -> Step {
        if self.phase != Phase::Idle {
            return Step::None;
        }
        self.session += 1;
        self.generation += 1;
        self.begin_processing()
    }

    /// 识别完成 → 进入上屏（开始等壳回报）
    pub fn commit_submit(&mut self) {
        self.processing_since = None;
        if let Some(task_id) = self.task_id.clone() {
            self.phase = Phase::Inserting;
            self.pending_submit = Some((task_id, Instant::now() + SUBMIT_TIMEOUT));
        }
    }

    /// 识别失败/为空 → 回 idle，保留任务号便于诊断
    pub fn abort_processing(&mut self) {
        self.processing_since = None;
        self.phase = Phase::Idle;
    }

    pub fn submit_result(&mut self, task_id: &str) -> SubmitOutcome {
        match self.pending_submit.take() {
            Some((pending, _)) if pending == task_id => {
                self.phase = Phase::Idle;
                SubmitOutcome::Accepted
            }
            Some(other) => {
                self.pending_submit = Some(other);
                SubmitOutcome::StaleTaskId
            }
            None => SubmitOutcome::NoPending,
        }
    }

    pub fn cancel(&mut self) -> Step {
        match self.phase {
            Phase::Inserting => Step::RejectedCancel,
            _ => {
                self.phase = Phase::Idle;
                self.pending_stop = false;
                self.ready_at = None;
                self.processing_since = None;
                Step::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn pending_stop_still_finishes_after_ready() {
        let mut m = Machine::new();
        assert!(matches!(m.down(), Step::SessionStarted));
        assert!(matches!(m.up(), Step::None));
        assert_eq!(m.phase, Phase::Starting);
        sleep(READY_GRACE + Duration::from_millis(10));
        assert!(matches!(m.tick(Instant::now()), Step::Listening));
        assert_eq!(m.phase, Phase::Listening);
        assert!(matches!(m.take_pending_stop(), Step::Finish { .. }));
        assert_eq!(m.phase, Phase::Processing);
    }

    #[test]
    fn second_down_does_not_restart() {
        let mut m = Machine::new();
        m.down();
        let session = m.session;
        m.down();
        assert_eq!(m.session, session);
    }

    #[test]
    fn cancel_rejected_while_inserting() {
        let mut m = Machine::new();
        m.down();
        sleep(READY_GRACE + Duration::from_millis(10));
        m.tick(Instant::now());
        m.up();
        m.commit_submit();
        assert_eq!(m.phase, Phase::Inserting);
        assert!(matches!(m.cancel(), Step::RejectedCancel));
        assert_eq!(m.phase, Phase::Inserting);
    }

    #[test]
    fn submit_result_only_once() {
        let mut m = Machine::new();
        m.down();
        sleep(READY_GRACE + Duration::from_millis(10));
        m.tick(Instant::now());
        let task = match m.up() {
            Step::Finish { task_id } => task_id,
            other => panic!("预期 Finish，得到 {other:?}"),
        };
        m.commit_submit();
        assert_eq!(m.submit_result(&task), SubmitOutcome::Accepted);
        assert_eq!(m.submit_result(&task), SubmitOutcome::NoPending);
        assert_eq!(m.phase, Phase::Idle);
    }

    #[test]
    fn decode_watchdog_fires() {
        let mut m = Machine::new();
        m.down();
        sleep(READY_GRACE + Duration::from_millis(10));
        m.tick(Instant::now());
        m.up();
        m.processing_since = Some(Instant::now() - DECODE_TIMEOUT - Duration::from_secs(1));
        assert!(matches!(m.tick(Instant::now()), Step::DecodeStalled { .. }));
        assert_eq!(m.phase, Phase::Idle);
    }
}
