//! Split ratios for the two witch panes, and telling a window resize
//! (re-apply the grimoire's ratio) from a user drag (leave the tab alone).

use super::store::TabKey;
use std::collections::HashMap;

/// Outer rows of a grimoire showing 1 content row: herdr draws a border row
/// above and below every pane in a split. Measured in Task 9 with a client
/// attached: a 3-row rect shows top border, one visible row, bottom border.
/// (Its pty reports 2 rows, herdr's floor, and only the last is visible;
/// the grimoire draws on row `rows/2`, which is that row.)
pub const GRIM_OUTER_ROWS: u16 = 3;

/// herdr 0.9.5 clamps every split ratio to this range (`pane.split
/// --ratio`, `layout.set_split_ratio`, `layout.apply` and `pane.resize`
/// all do). So on a tab taller than 30 rows the grimoire gets
/// `area - round(area * 0.9)` rows, not 3.
pub const RATIO_MIN: f32 = 0.1;
pub const RATIO_MAX: f32 = 0.9;

/// The root split's first-child share that leaves the grimoire
/// `GRIM_OUTER_ROWS` tall, as far as herdr's clamp allows.
pub fn grimoire_ratio(area_rows: u16) -> f32 {
    let above = area_rows.saturating_sub(GRIM_OUTER_ROWS);
    (f32::from(above) / f32::from(area_rows.max(1))).clamp(RATIO_MIN, RATIO_MAX)
}

/// The grimoire's outer rows herdr gives for `grimoire_ratio(area_rows)`
/// (the first child gets `round(rows * ratio)`, probed).
pub fn grim_target_rows(area_rows: u16) -> u16 {
    let first = (f32::from(area_rows) * grimoire_ratio(area_rows)).round() as u16;
    area_rows.saturating_sub(first)
}

/// The sigil split's first-child share for a sigil `pct`% wide.
pub fn sigil_ratio(pct: u8) -> f32 {
    (f32::from(100u8.saturating_sub(pct)) / 100.0).clamp(RATIO_MIN, RATIO_MAX)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizingAction {
    /// The tab's outer size changed: set the grimoire ratio again.
    Reapply,
    /// Same outer size but the grimoire isn't at its target height: the user
    /// dragged the split.
    MarkUserSized,
    Nothing,
}

/// Remembers each tab's last outer size. Only fed tabs that aren't
/// user-sized and whose grimoire is the root split's second child.
#[derive(Debug, Default)]
pub struct DragTracker {
    pub last_outer: HashMap<TabKey, (u16, u16)>,
}

impl DragTracker {
    /// `outer` is the tab area (width, height); `grim_rows` the grimoire's
    /// outer height. The first sighting of a tab only records its size, so
    /// a restarted daemon never mistakes an old drag or resize for a new one.
    pub fn on_layout(&mut self, key: &TabKey, outer: (u16, u16), grim_rows: u16) -> SizingAction {
        match self.last_outer.insert(key.clone(), outer) {
            None => SizingAction::Nothing,
            Some(prev) if prev != outer => SizingAction::Reapply,
            Some(_) if grim_rows != grim_target_rows(outer.1) => SizingAction::MarkUserSized,
            Some(_) => SizingAction::Nothing,
        }
    }

    /// Note the size the daemon itself just sized the tab for.
    pub fn seed(&mut self, key: &TabKey, outer: (u16, u16)) {
        self.last_outer.insert(key.clone(), outer);
    }

    pub fn forget(&mut self, key: &TabKey) {
        self.last_outer.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// herdr's first-child rows for a ratio.
    fn first_rows(area: u16, ratio: f32) -> u16 {
        (f32::from(area) * ratio).round() as u16
    }

    #[test]
    fn grimoire_ratio_leaves_three_rows() {
        for area in [10u16, 20, 24, 27, 30] {
            let r = grimoire_ratio(area);
            assert_eq!(area - first_rows(area, r), GRIM_OUTER_ROWS, "area {area}");
            assert_eq!(grim_target_rows(area), GRIM_OUTER_ROWS, "area {area}");
        }
    }

    #[test]
    fn grimoire_ratio_respects_clamp() {
        assert_eq!(grimoire_ratio(40), RATIO_MAX);
        assert_eq!(grim_target_rows(40), 4);
        assert_eq!(grim_target_rows(44), 4);
        assert_eq!(grim_target_rows(60), 6);
        assert_eq!(grimoire_ratio(3), RATIO_MIN);
        assert_eq!(grimoire_ratio(0), RATIO_MIN);
        assert_eq!(grim_target_rows(0), 0);
    }

    #[test]
    fn sigil_ratio_is_complement() {
        assert!((sigil_ratio(15) - 0.85).abs() < 1e-6);
        assert!((sigil_ratio(50) - 0.5).abs() < 1e-6);
        assert_eq!(sigil_ratio(5), RATIO_MAX);
        assert_eq!(sigil_ratio(100), RATIO_MIN);
    }

    #[test]
    fn tracker_actions() {
        let k = TabKey::from("w1:t1");
        let mut d = DragTracker::default();
        assert_eq!(d.on_layout(&k, (120, 24), 9), SizingAction::Nothing);
        assert_eq!(d.on_layout(&k, (120, 24), 3), SizingAction::Nothing);
        assert_eq!(d.on_layout(&k, (120, 24), 5), SizingAction::MarkUserSized);
        assert_eq!(d.on_layout(&k, (100, 20), 2), SizingAction::Reapply);
        assert_eq!(d.on_layout(&k, (100, 20), 3), SizingAction::Nothing);
        d.forget(&k);
        assert_eq!(d.on_layout(&k, (100, 20), 7), SizingAction::Nothing);
        let k2 = TabKey::from("w1:t2");
        d.seed(&k2, (120, 40));
        assert_eq!(d.on_layout(&k2, (120, 40), 4), SizingAction::Nothing);
        assert_eq!(d.on_layout(&k2, (120, 40), 3), SizingAction::MarkUserSized);
    }
}
