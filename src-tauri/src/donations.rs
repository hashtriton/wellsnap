//! Donation reminder logic. Counters live only in the local settings file and are never sent anywhere.
//!
//! Policy (see research notes): passive "Support" entries are always visible; the active reminder card is
//! shown at most 4 times per install, after 50 / 300 / 1000 / 3000 completed snapshots, never in the first
//! 7 days, only after at least 3 active days, with 60 / 90 / 180 day gaps between reminders.

use serde::{Deserialize, Serialize};

const DAY: i64 = 24 * 60 * 60;
pub const THRESHOLDS: [u32; 4] = [50, 300, 1000, 3000];
const GAP_DAYS_AFTER: [i64; 3] = [60, 90, 180];
const QUIET_DAYS_AFTER_INSTALL: i64 = 7;
const MIN_ACTIVE_DAYS: u32 = 3;
const SUPPORT_CLICK_SNOOZE_DAYS: i64 = 180;
const NOT_SEEN_RETRY_DAYS: i64 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DonationStatus {
    #[default]
    Active,
    Supporter,
    Never,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PromptOutcome {
    /// User opened the support page.
    Support,
    /// "Later", Esc, close button or auto-hide timeout.
    Later,
    /// Honor system: no verification, just stops reminders and shows a badge.
    AlreadySupported,
    Never,
    /// The card was hidden before it was visible for 2 seconds (e.g. a new capture started).
    NotSeen,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DonationState {
    /// Unix seconds of the first launch; 0 means "not initialised yet".
    pub first_run_at: i64,
    /// Snapshots that were copied or saved at least once (not hotkey presses or cancelled selections).
    pub captures: u32,
    pub marks: u32,
    pub active_days: u32,
    /// Local date (YYYY-MM-DD) of the last counted active day.
    pub last_active_day: String,
    pub prompts_shown: u32,
    pub last_prompt_at: i64,
    pub next_eligible_at: i64,
    pub status: DonationStatus,
}

impl DonationState {
    pub fn ensure_initialised(&mut self, now: i64) {
        if self.first_run_at <= 0 || self.first_run_at > now {
            self.first_run_at = now;
        }
    }

    pub fn record_capture(&mut self, today: &str, marks: u32) {
        self.captures = self.captures.saturating_add(1);
        self.marks = self.marks.saturating_add(marks);
        if self.last_active_day != today {
            self.last_active_day = today.to_string();
            self.active_days = self.active_days.saturating_add(1);
        }
    }

    pub fn should_prompt(&self, now: i64) -> bool {
        if self.status != DonationStatus::Active || self.first_run_at <= 0 {
            return false;
        }
        let Some(&threshold) = THRESHOLDS.get(self.prompts_shown as usize) else {
            return false;
        };
        // A clock moved backwards must never make a reminder appear earlier.
        if now < self.last_prompt_at {
            return false;
        }
        now >= self.first_run_at + QUIET_DAYS_AFTER_INSTALL * DAY
            && self.active_days >= MIN_ACTIVE_DAYS
            && self.captures >= threshold
            && now >= self.next_eligible_at
    }

    pub fn apply_outcome(&mut self, now: i64, outcome: PromptOutcome) {
        match outcome {
            PromptOutcome::NotSeen => {
                self.next_eligible_at = self.next_eligible_at.max(now + NOT_SEEN_RETRY_DAYS * DAY);
            }
            PromptOutcome::Later => {
                self.count_prompt(now);
                let gap = GAP_DAYS_AFTER
                    .get((self.prompts_shown as usize).saturating_sub(1))
                    .copied()
                    .unwrap_or(*GAP_DAYS_AFTER.last().unwrap());
                self.next_eligible_at = self.next_eligible_at.max(now + gap * DAY);
            }
            PromptOutcome::Support => {
                self.count_prompt(now);
                self.next_eligible_at = self.next_eligible_at.max(now + SUPPORT_CLICK_SNOOZE_DAYS * DAY);
            }
            PromptOutcome::AlreadySupported => self.status = DonationStatus::Supporter,
            PromptOutcome::Never => self.status = DonationStatus::Never,
        }
    }

    /// Settings toggle "remind me again": supporters and "never" users can opt back in.
    pub fn reactivate(&mut self, now: i64) {
        self.status = DonationStatus::Active;
        self.next_eligible_at = self.next_eligible_at.max(now + 90 * DAY);
    }

    pub fn reset_counters(&mut self) {
        self.captures = 0;
        self.marks = 0;
        self.active_days = 0;
        self.last_active_day.clear();
    }

    fn count_prompt(&mut self, now: i64) {
        self.prompts_shown = self.prompts_shown.saturating_add(1);
        self.last_prompt_at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;

    fn state_with(captures: u32, active_days: u32) -> DonationState {
        let mut s = DonationState::default();
        s.ensure_initialised(T0);
        s.captures = captures;
        s.active_days = active_days;
        s
    }

    #[test]
    fn fresh_install_never_prompts() {
        let s = state_with(0, 0);
        assert!(!s.should_prompt(T0));
        assert!(!s.should_prompt(T0 + 30 * DAY));
    }

    #[test]
    fn quiet_first_week_even_with_many_captures() {
        let s = state_with(100, 5);
        assert!(!s.should_prompt(T0 + 6 * DAY));
        assert!(s.should_prompt(T0 + 7 * DAY));
    }

    #[test]
    fn needs_threshold_and_active_days() {
        assert!(!state_with(49, 10).should_prompt(T0 + 8 * DAY));
        assert!(!state_with(50, 2).should_prompt(T0 + 8 * DAY));
        assert!(state_with(50, 3).should_prompt(T0 + 8 * DAY));
    }

    #[test]
    fn later_applies_gaps_and_next_threshold() {
        let mut s = state_with(50, 3);
        let day10 = T0 + 10 * DAY;
        assert!(s.should_prompt(day10));
        s.apply_outcome(day10, PromptOutcome::Later);
        s.captures = 299;
        assert!(!s.should_prompt(day10 + 59 * DAY));
        assert!(!s.should_prompt(day10 + 60 * DAY), "299 captures is below the second threshold");
        s.captures = 300;
        assert!(s.should_prompt(day10 + 60 * DAY));
    }

    #[test]
    fn cap_of_four_prompts() {
        let mut s = state_with(10_000, 100);
        let mut now = T0 + 8 * DAY;
        for _ in 0..4 {
            assert!(s.should_prompt(now));
            s.apply_outcome(now, PromptOutcome::Later);
            now += 400 * DAY;
        }
        assert!(!s.should_prompt(now));
    }

    #[test]
    fn support_click_snoozes_half_a_year() {
        let mut s = state_with(10_000, 100);
        let now = T0 + 8 * DAY;
        s.apply_outcome(now, PromptOutcome::Support);
        assert!(!s.should_prompt(now + 179 * DAY));
        assert!(s.should_prompt(now + 180 * DAY));
    }

    #[test]
    fn supporter_and_never_stop_prompts_until_reactivated() {
        let now = T0 + 8 * DAY;
        for outcome in [PromptOutcome::AlreadySupported, PromptOutcome::Never] {
            let mut s = state_with(10_000, 100);
            s.apply_outcome(now, outcome);
            assert!(!s.should_prompt(now + 1000 * DAY));
            s.reactivate(now);
            assert!(!s.should_prompt(now + 89 * DAY));
            assert!(s.should_prompt(now + 90 * DAY));
        }
    }

    #[test]
    fn clock_rollback_does_not_prompt_early() {
        let mut s = state_with(10_000, 100);
        let now = T0 + 100 * DAY;
        s.apply_outcome(now, PromptOutcome::Later);
        assert!(!s.should_prompt(now - 50 * DAY));
    }

    #[test]
    fn not_seen_retries_next_day_without_counting() {
        let mut s = state_with(60, 5);
        let now = T0 + 8 * DAY;
        s.apply_outcome(now, PromptOutcome::NotSeen);
        assert_eq!(s.prompts_shown, 0);
        assert!(!s.should_prompt(now + DAY - 1));
        assert!(s.should_prompt(now + DAY));
    }

    #[test]
    fn active_days_count_once_per_date() {
        let mut s = DonationState::default();
        s.record_capture("2026-10-22", 2);
        s.record_capture("2026-10-22", 0);
        s.record_capture("2026-10-23", 1);
        assert_eq!((s.captures, s.marks, s.active_days), (3, 3, 2));
    }

    #[test]
    fn future_first_run_is_clamped() {
        let mut s = DonationState { first_run_at: T0 + 1000, ..Default::default() };
        s.ensure_initialised(T0);
        assert_eq!(s.first_run_at, T0);
    }
}
