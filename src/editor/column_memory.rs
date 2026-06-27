use gpui::Pixels;

/// Tracks the preferred visual pixel column for vertical cursor movement.
///
/// Stored as a pixel offset within a visual row (not character index).
/// This handles variable-width fonts, CJK characters, and mixed text correctly.
///
/// Preserved across blank lines and short lines so the cursor snaps back
/// when reaching a longer line. Reset by horizontal movement, mouse clicks,
/// text edits, and other actions that invalidate the remembered column.
#[derive(Debug, Clone, Default)]
pub struct ColumnMemory {
    preferred_x: Option<Pixels>,
}

impl ColumnMemory {
    pub fn new() -> Self {
        Self { preferred_x: None }
    }

    /// Get the target x for vertical movement.
    /// Returns the remembered column if set, otherwise the current within-row x.
    pub fn target_x(&self, current_within_row_x: Pixels) -> Pixels {
        self.preferred_x.unwrap_or(current_within_row_x)
    }

    /// Update the column memory with the actual x the cursor landed at.
    pub fn record(&mut self, x: Pixels) {
        self.preferred_x = Some(x);
    }

    /// Update column memory only if the target was NOT clamped to the row's
    /// right edge. When clamped, the row is too short — the old value is
    /// preserved so the cursor snaps back when reaching a longer row.
    pub fn record_if_not_clamped(&mut self, x: Pixels, was_clamped_to_right: bool) {
        if !was_clamped_to_right {
            self.preferred_x = Some(x);
        }
    }

    pub fn clear(&mut self) {
        self.preferred_x = None;
    }

    pub fn is_set(&self) -> bool {
        self.preferred_x.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn new_is_empty() {
        let cm = ColumnMemory::new();
        assert!(!cm.is_set());
    }

    #[test]
    fn target_x_returns_current_when_empty() {
        let cm = ColumnMemory::new();
        assert_eq!(cm.target_x(px(42.0)), px(42.0));
    }

    #[test]
    fn target_x_returns_recorded_when_set() {
        let mut cm = ColumnMemory::new();
        cm.record(px(100.0));
        assert_eq!(cm.target_x(px(42.0)), px(100.0));
        assert!(cm.is_set());
    }

    #[test]
    fn clear_resets_to_empty() {
        let mut cm = ColumnMemory::new();
        cm.record(px(100.0));
        assert!(cm.is_set());
        cm.clear();
        assert!(!cm.is_set());
        assert_eq!(cm.target_x(px(42.0)), px(42.0));
    }

    #[test]
    fn record_if_not_clamped_updates_when_not_clamped() {
        let mut cm = ColumnMemory::new();
        cm.record(px(80.0));
        cm.record_if_not_clamped(px(60.0), false);
        // Not clamped → should update to the new value
        assert_eq!(cm.target_x(px(0.0)), px(60.0));
    }

    #[test]
    fn record_if_not_clamped_preserves_old_when_clamped() {
        let mut cm = ColumnMemory::new();
        cm.record(px(200.0));
        cm.record_if_not_clamped(px(100.0), true);
        // Clamped → should preserve the old value for snap-back
        assert_eq!(cm.target_x(px(0.0)), px(200.0));
    }

    #[test]
    fn snap_back_across_short_line() {
        // Simulates: user on long line at 200px → moves to short line (clamped
        // at 100px) → moves to another long line → snaps back to 200px
        let mut cm = ColumnMemory::new();

        // First move from long line
        cm.record(px(200.0));
        assert_eq!(cm.target_x(px(0.0)), px(200.0));

        // Move to short line (clamped)
        cm.record_if_not_clamped(px(100.0), true);
        assert_eq!(cm.target_x(px(0.0)), px(200.0)); // preserved for snap-back

        // Move to another long line (within width, not clamped)
        cm.record_if_not_clamped(px(195.0), false);
        assert_eq!(cm.target_x(px(0.0)), px(195.0)); // updated
    }

    #[test]
    fn survive_blank_line() {
        // Simulates: user moves from content line at 80px → blank line
        // (column preserved) → next content line at 80px
        let mut cm = ColumnMemory::new();

        // Start on content line
        cm.record(px(80.0));

        // Blank line: cursor at start, column memory intentionally NOT updated
        // (caller does not call record/record_if_not_clamped)

        // Next content line: target_x still returns 80px
        assert_eq!(cm.target_x(px(0.0)), px(80.0));
    }

    #[test]
    fn defaults_to_current_when_cleared_by_horizontal_move() {
        let mut cm = ColumnMemory::new();
        cm.record(px(80.0));
        assert_eq!(cm.target_x(px(10.0)), px(80.0));

        // User presses Left → column memory resets
        cm.clear();
        assert!(!cm.is_set());
        assert_eq!(cm.target_x(px(35.0)), px(35.0));
    }
}
