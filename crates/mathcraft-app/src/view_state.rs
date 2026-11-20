//! View and calculation state that the ribbon and the shortcuts change: the zoom steps, the
//! automatic or manual calculation mode, and what the page shows. Pure logic, tested without a
//! window.

/// Zoom limits, in percent.
pub const ZOOM_MIN: f32 = 25.0;
pub const ZOOM_MAX: f32 = 400.0;
/// The zoom percentages that the zoom in and zoom out buttons step through.
pub const ZOOM_STEPS: [f32; 13] = [
    25.0, 33.0, 50.0, 67.0, 75.0, 90.0, 100.0, 110.0, 125.0, 150.0, 200.0, 300.0, 400.0,
];
/// The zoom percentage of a new view.
pub const ZOOM_RESET: f32 = 100.0;

/// The next zoom step above `pct`, or the maximum.
pub fn zoom_in(pct: f32) -> f32 {
    ZOOM_STEPS
        .iter()
        .copied()
        .find(|s| *s > pct + 0.5)
        .unwrap_or(ZOOM_MAX)
}

/// The next zoom step below `pct`, or the minimum.
pub fn zoom_out(pct: f32) -> f32 {
    ZOOM_STEPS
        .iter()
        .rev()
        .copied()
        .find(|s| *s < pct - 0.5)
        .unwrap_or(ZOOM_MIN)
}

/// `pct` multiplied by `factor` (a pinch or a Ctrl+wheel step), kept within the limits. A factor
/// that is not finite or not positive leaves `pct` unchanged.
pub fn zoom_by_factor(pct: f32, factor: f32) -> f32 {
    if !factor.is_finite() || factor <= 0.0 {
        return pct;
    }
    (pct * factor).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Automatic calculation re-evaluates the worksheet after each change. Manual calculation only
/// when the user asks (F9 or Calculate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcMode {
    Auto,
    Manual,
}

impl CalcMode {
    pub fn toggled(self) -> Self {
        match self {
            CalcMode::Auto => CalcMode::Manual,
            CalcMode::Manual => CalcMode::Auto,
        }
    }

    /// The status bar word.
    pub fn label(self) -> &'static str {
        match self {
            CalcMode::Auto => crate::i18n::tr("calc.auto"),
            CalcMode::Manual => crate::i18n::tr("calc.manual"),
        }
    }
}

/// Whether the worksheet is evaluated now.
///
/// `forced`: the user asked (F9, Calculate). `has_view`: an evaluated view exists. `changed`: the
/// document changed since that view was made. A changed document in manual mode waits for F9.
pub fn needs_refresh(mode: CalcMode, forced: bool, has_view: bool, changed: bool) -> bool {
    forced || !has_view || (changed && mode == CalcMode::Auto)
}

/// What the page draws besides the regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayFlags {
    /// The light grid on the page. On by default.
    pub grid: bool,
    /// A thin outline around each region. Off by default.
    pub region_borders: bool,
    /// Page layout: the sheet is drawn as pages with margins and page breaks, instead of one
    /// endless sheet. On by default, as in Prime (screenshot 0003 opens in page view).
    pub page_layout: bool,
}

impl Default for DisplayFlags {
    fn default() -> Self {
        Self {
            grid: true,
            region_borders: false,
            page_layout: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_up_and_down_through_the_list() {
        assert_eq!(zoom_in(100.0), 110.0);
        assert_eq!(zoom_out(100.0), 90.0);
        assert_eq!(zoom_in(119.0), 125.0);
        assert_eq!(zoom_out(119.0), 110.0);
    }

    #[test]
    fn zoom_stops_at_the_limits() {
        assert_eq!(zoom_in(ZOOM_MAX), ZOOM_MAX);
        assert_eq!(zoom_out(ZOOM_MIN), ZOOM_MIN);
        assert_eq!(zoom_in(399.0), ZOOM_MAX);
        assert_eq!(zoom_out(26.0), ZOOM_MIN);
    }

    #[test]
    fn reset_is_one_hundred_percent() {
        assert_eq!(ZOOM_RESET, 100.0);
    }

    #[test]
    fn wheel_factor_is_clamped_and_bad_factors_are_ignored() {
        assert_eq!(zoom_by_factor(100.0, 2.0), 200.0);
        assert_eq!(zoom_by_factor(300.0, 10.0), ZOOM_MAX);
        assert_eq!(zoom_by_factor(100.0, 0.0), 100.0);
        assert_eq!(zoom_by_factor(100.0, f32::NAN), 100.0);
    }

    #[test]
    fn manual_mode_waits_for_the_request() {
        assert!(needs_refresh(CalcMode::Auto, false, true, true));
        assert!(!needs_refresh(CalcMode::Manual, false, true, true));
        assert!(needs_refresh(CalcMode::Manual, true, true, false));
        assert!(needs_refresh(CalcMode::Manual, false, false, false));
        assert!(!needs_refresh(CalcMode::Auto, false, true, false));
    }

    #[test]
    fn calc_mode_toggles_and_labels() {
        assert_eq!(CalcMode::Auto.toggled(), CalcMode::Manual);
        assert_eq!(CalcMode::Manual.toggled().toggled(), CalcMode::Manual);
        assert_eq!(CalcMode::Auto.label(), "AUTO");
        assert_eq!(CalcMode::Manual.label(), "MANUAL");
    }

    #[test]
    fn grid_on_and_borders_off_by_default() {
        let flags = DisplayFlags::default();
        assert!(flags.grid);
        assert!(!flags.region_borders);
    }
}
