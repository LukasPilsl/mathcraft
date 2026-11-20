//! Pagination: how the sheet is cut into pages, and where the page breaks fall.
//!
//! The sheet is one tall space in points, measured from the top-left corner of the margin box (see
//! [`PageFrame`]). Page `k` holds the sheet from `k * P` to `(k + 1) * P`, where `P` is the printable
//! height (paper height less the top and bottom margins). A region belongs to the page that holds
//! its top edge. A region that crosses a page's bottom edge is drawn across both pages and gets a
//! break line. Horizontal overflow is not paginated: content wider than the printable width is
//! clipped at the page edge.
//!
//! Everything here is pure geometry, so it is tested without a window.

use mathcraft_import::page::PageSettings;

/// Largest number of pages laid out or exported. Content beyond it is not drawn on any page.
pub const MAX_PAGES: usize = 500;

/// Tolerance, in points, for a page break that already falls at the top of a page.
const TOP_EPSILON: f32 = 0.01;

/// Where one page break is laid out. See [`break_layout`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreakLayout {
    /// The break's sheet y, as stored.
    pub at: f32,
    /// The break's y in the laid-out sheet: the marker is drawn here.
    pub marker: f32,
    /// How far the content from `at` down moves in the laid-out sheet (the total of this break and
    /// the breaks above it).
    pub shift: f32,
}

/// Lays out page breaks on a sheet of pages `paper_h` points high. Content below a break starts
/// the next page: its page top is the top of the page after the one the break falls on. A break
/// that already falls at a page top moves nothing. `breaks` are the stored sheet y of each break.
/// The result is sorted by `at`.
pub fn break_layout(breaks: &[f32], paper_h: f32) -> Vec<BreakLayout> {
    let height = paper_h.max(1.0);
    let mut sorted: Vec<f32> = breaks.iter().copied().filter(|y| y.is_finite()).collect();
    sorted.sort_by(f32::total_cmp);
    let mut shift = 0.0_f32;
    let mut out = Vec::with_capacity(sorted.len());
    for at in sorted {
        let at = at.max(0.0);
        let marker = at + shift;
        let page = (marker / height).floor();
        let inside = marker - page * height;
        if inside > TOP_EPSILON {
            shift = (page + 1.0) * height - at;
        }
        out.push(BreakLayout { at, marker, shift });
    }
    out
}

/// The shift that [`break_layout`] gives content at stored sheet y `y`: that of the last break at
/// or above `y`, or zero. `layout` is sorted by `at` (see [`break_layout`]), so a binary search
/// finds it.
pub fn shift_at(layout: &[BreakLayout], y: f32) -> f32 {
    layout
        .partition_point(|b| b.at <= y)
        .checked_sub(1)
        .map_or(0.0, |i| layout[i].shift)
}

/// The content frame of a page setup. Stored positions are measured in points from the top-left
/// corner of the margin box (the printable area), not from the paper's corner. That corner is the
/// frame's origin, which lies at the left and top margins inside the paper.
///
/// The laid-out sheet (stored positions after the page breaks, see [`break_layout`]) is cut into
/// pages of printable height. In the frame, page `k` holds its printable area at
/// `k * paper_h .. k * paper_h + printable_h`, and the margins of the pages sit in the gaps between
/// those areas. So a frame point always lies on the page whose printable area it is in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageFrame {
    /// Paper height in points: the stride of the pages in the frame.
    pub paper_h: f32,
    /// Margins in points: top, right, bottom, left.
    pub margins: [f32; 4],
}

impl Default for PageFrame {
    fn default() -> Self {
        Self::new(&PageSettings::default())
    }
}

impl PageFrame {
    /// The frame of a page setup.
    pub fn new(page: &PageSettings) -> Self {
        let paper_h = page.paper_points()[1] as f32;
        let m = page.margins();
        Self {
            paper_h,
            margins: [m.top, m.right, m.bottom, m.left].map(|v| v as f32),
        }
    }

    /// Paper position of the frame's origin, as `[left, top]` in points.
    pub fn origin(&self) -> [f32; 2] {
        [self.margins[3], self.margins[0]]
    }

    /// Height of one page's printable area, in points (at least 1).
    pub fn printable_h(&self) -> f32 {
        (self.paper_h - self.margins[0] - self.margins[2]).max(1.0)
    }

    /// The frame y of a laid-out sheet y: each full page of printable height before it adds the
    /// margins between the pages.
    pub fn to_frame(&self, laid: f32) -> f32 {
        let p = self.printable_h();
        let k = (laid / p).floor().max(0.0);
        laid + k * (self.paper_h - p)
    }

    /// The laid-out sheet y of a frame y: the inverse of [`PageFrame::to_frame`]. A point in the gap
    /// below a printable area gives the end of that page.
    pub fn to_laid(&self, frame: f32) -> f32 {
        let p = self.printable_h();
        let k = (frame / self.paper_h).floor().max(0.0);
        k * p + (frame - k * self.paper_h).clamp(0.0, p)
    }
}

/// The page layout of one worksheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// Paper width and height in points, after orientation.
    pub paper: [f32; 2],
    /// Margins in points: top, right, bottom, left.
    pub margins: [f32; 4],
    /// Number of pages, from 1 to [`MAX_PAGES`].
    pub count: usize,
    /// True when the content runs past the last page, so some of it is not on a page.
    pub capped: bool,
}

impl Geometry {
    /// The layout for `page` when the content ends at sheet height `content_bottom`, in points.
    /// A non-finite or non-positive bottom gives one page.
    pub fn new(page: &PageSettings, content_bottom: f32) -> Self {
        let p = page.paper_points();
        let paper = [p[0] as f32, p[1] as f32];
        let m = page.margins();
        let margins = [m.top as f32, m.right as f32, m.bottom as f32, m.left as f32];
        let height = paper[1].max(1.0);
        let needed = if content_bottom.is_finite() && content_bottom > 0.0 {
            (content_bottom / height).ceil() as usize
        } else {
            1
        };
        let needed = needed.max(1);
        Self {
            paper,
            margins,
            count: needed.min(MAX_PAGES),
            capped: needed > MAX_PAGES,
        }
    }

    /// Paper y of the top edge of page `index`, in the stacked paper (the pages stack with no gap).
    pub fn page_top(&self, index: usize) -> f32 {
        index as f32 * self.paper[1]
    }

    /// The content frame of this page setup (see [`PageFrame`]).
    pub fn frame(&self) -> PageFrame {
        PageFrame {
            paper_h: self.paper[1],
            margins: self.margins,
        }
    }

    /// The printable rectangle of page `index`, as `[left, top, right, bottom]` in sheet points.
    pub fn printable(&self, index: usize) -> [f32; 4] {
        let [w, h] = self.paper;
        let [top, right, bottom, left] = self.margins;
        let base = self.page_top(index);
        [left, base + top, w - right, base + h - bottom]
    }

    /// PageFrame y of the boundary between page `k - 1` and page `k`: the bottom of the printable area
    /// of page `k - 1` (see [`PageFrame`]).
    pub fn boundary(&self, k: usize) -> f32 {
        let [top, _, bottom, _] = self.margins;
        k as f32 * self.paper[1] - top - bottom
    }

    /// PageFrame y of each page boundary that a span crosses, for spans given as `(top, bottom)` in
    /// frame points. Sorted and without duplicates. A boundary that a span only touches is not
    /// included.
    pub fn breaks(&self, spans: &[(f32, f32)]) -> Vec<f32> {
        let mut out: Vec<f32> = Vec::new();
        for &(top, bottom) in spans {
            if !top.is_finite() || !bottom.is_finite() {
                continue;
            }
            for k in 1..self.count {
                let y = self.boundary(k);
                if top < y && y < bottom {
                    out.push(y);
                }
            }
        }
        out.sort_by(f32::total_cmp);
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a4() -> PageSettings {
        PageSettings::default()
    }

    #[test]
    fn short_content_is_one_page() {
        let g = Geometry::new(&a4(), 300.0);
        assert_eq!(g.count, 1);
        assert!(!g.capped);
    }

    #[test]
    fn content_is_cut_at_the_paper_height() {
        let h = Geometry::new(&a4(), 1.0).paper[1];
        let g = Geometry::new(&a4(), h + 1.0);
        assert_eq!(g.count, 2);
        let g = Geometry::new(&a4(), 2.0 * h);
        assert_eq!(g.count, 2, "content ending exactly on a boundary");
    }

    #[test]
    fn non_finite_or_empty_content_is_one_page() {
        assert_eq!(Geometry::new(&a4(), f32::NAN).count, 1);
        assert_eq!(Geometry::new(&a4(), f32::INFINITY).count, 1);
        assert_eq!(Geometry::new(&a4(), 0.0).count, 1);
        assert_eq!(Geometry::new(&a4(), -5.0).count, 1);
    }

    #[test]
    fn page_count_is_capped() {
        let g = Geometry::new(&a4(), 1.0e9);
        assert_eq!(g.count, MAX_PAGES);
        assert!(g.capped);
    }

    /// The page that holds sheet height `y`, clamped to the page range.
    fn page_of(g: &Geometry, y: f32) -> usize {
        if !y.is_finite() || y <= 0.0 {
            return 0;
        }
        ((y / g.paper[1]).floor() as usize).min(g.count - 1)
    }

    #[test]
    fn region_belongs_to_the_page_of_its_top() {
        let g = Geometry::new(&a4(), 3000.0);
        let h = g.paper[1];
        assert_eq!(page_of(&g, 10.0), 0);
        assert_eq!(page_of(&g, h - 0.5), 0);
        assert_eq!(page_of(&g, h + 0.5), 1);
        assert_eq!(page_of(&g, f32::NAN), 0);
        assert_eq!(page_of(&g, 1.0e9), g.count - 1);
    }

    #[test]
    fn printable_area_is_inset_by_margins_on_each_page() {
        let g = Geometry::new(&a4(), 3000.0);
        let [top, right, bottom, left] = g.margins;
        let [l, t, r, b] = g.printable(1);
        let h = g.paper[1];
        assert_eq!(l, left);
        assert!((r - (g.paper[0] - right)).abs() < 1e-3);
        assert!((t - (h + top)).abs() < 1e-3);
        assert!((b - (2.0 * h - bottom)).abs() < 1e-3);
    }

    #[test]
    fn crossing_region_gets_a_break_and_contained_one_does_not() {
        let g = Geometry::new(&a4(), 3000.0);
        let b = g.boundary(1);
        let breaks = g.breaks(&[(b - 10.0, b + 10.0), (10.0, 20.0), (b + 5.0, b + 6.0)]);
        assert_eq!(breaks, vec![b]);
    }

    #[test]
    fn breaks_are_sorted_and_unique() {
        let g = Geometry::new(&a4(), 4000.0);
        let (b1, b2) = (g.boundary(1), g.boundary(2));
        let breaks = g.breaks(&[(b1 - 1.0, b2 + 1.0), (b1 - 2.0, b1 + 2.0)]);
        assert_eq!(breaks, vec![b1, b2]);
    }

    #[test]
    fn touching_a_boundary_is_not_a_break() {
        let g = Geometry::new(&a4(), 3000.0);
        let b = g.boundary(1);
        assert!(g.breaks(&[(b - 10.0, b)]).is_empty());
        assert!(g.breaks(&[(b, b + 10.0)]).is_empty());
    }

    #[test]
    fn stored_origin_is_the_margin_box_corner() {
        let g = Geometry::new(&a4(), 3000.0);
        let f = g.frame();
        // A region at stored (0, 0) sits at the top-left of the printable area, inside the margins.
        assert_eq!(f.origin(), [g.margins[3], g.margins[0]]);
        assert!((f.origin()[0] - 70.866).abs() < 0.01, "25 mm left margin");
        assert_eq!(f.to_frame(0.0), 0.0);
        // The printable area of page 0 is 25 mm short of the paper at the bottom and top.
        let printable = f.printable_h();
        assert!((printable - (g.paper[1] - 2.0 * f.margins[0])).abs() < 1e-3);
    }

    #[test]
    fn frame_round_trips_and_pages_are_cut_at_the_printable_height() {
        let f = PageFrame::new(&a4());
        let p = f.printable_h();
        // The last point of page 0 stays on page 0; the next point starts page 1 at its paper stride.
        assert_eq!(f.to_frame(p - 1.0), p - 1.0);
        assert!((f.to_frame(p) - f.paper_h).abs() < 1e-3);
        // The gap between two printable areas (in the frame) maps back to the page end.
        assert!((f.to_laid(f.paper_h - 1.0) - p).abs() < 1e-3);
        for laid in [0.0_f32, 1.0, p - 0.5, p, 2.0 * p + 3.0, 7.0 * p + 100.0] {
            let back = f.to_laid(f.to_frame(laid));
            assert!(
                (back - laid).abs() < 1e-2,
                "laid {laid} came back as {back}"
            );
        }
    }

    #[test]
    fn frame_gap_lies_between_the_printable_areas() {
        let g = Geometry::new(&a4(), 3000.0);
        let f = g.frame();
        // Page 1's printable area starts where the frame says its paper stride puts it.
        let [l, t, _, _] = g.printable(1);
        assert_eq!(l, f.margins[3]);
        assert!((t - (f.paper_h + f.margins[0])).abs() < 1e-3);
        assert!((g.boundary(1) + f.margins[0] + f.margins[2] - f.paper_h).abs() < 1e-3);
    }

    #[test]
    fn a_break_starts_the_next_page_below_it() {
        let h = Geometry::new(&a4(), 1.0).paper[1];
        // A break a third of the way down page 1: the content below it starts page 2's top.
        let layout = break_layout(&[h / 3.0], h);
        assert_eq!(layout.len(), 1);
        assert!((layout[0].marker - h / 3.0).abs() < 1e-3);
        assert!((shift_at(&layout, h / 3.0 + 10.0) - (h - h / 3.0)).abs() < 1e-3);
        // Content above the break does not move.
        assert_eq!(shift_at(&layout, h / 3.0 - 10.0), 0.0);
    }

    #[test]
    fn a_break_at_a_page_top_moves_nothing() {
        let h = Geometry::new(&a4(), 1.0).paper[1];
        let layout = break_layout(&[h, 2.0 * h + 0.005], h);
        assert!(layout.iter().all(|b| b.shift == 0.0));
    }

    #[test]
    fn breaks_accumulate_and_are_sorted() {
        let h = Geometry::new(&a4(), 1.0).paper[1];
        let layout = break_layout(&[2.0 * h + 50.0, 10.0], h);
        assert_eq!(layout[0].at, 10.0);
        assert_eq!(layout[1].at, 2.0 * h + 50.0);
        // The first break moves the rest of page 1 away: the second break is then at 3h + 40 in
        // the laid-out sheet, on the fourth page, so the content below it starts the fifth (4h).
        assert_eq!(layout[0].shift, h - 10.0);
        assert!((layout[1].marker - (3.0 * h + 40.0)).abs() < 1e-2);
        assert!((layout[1].shift - (2.0 * h - 50.0)).abs() < 1e-2);
        assert_eq!(shift_at(&layout, 2.0 * h + 60.0), layout[1].shift);
    }

    #[test]
    fn shift_at_matches_a_linear_scan_over_many_breaks() {
        let h = Geometry::new(&a4(), 1.0).paper[1];
        // Breaks in scrambled order, some at or above zero's clamp, many of them.
        let breaks: Vec<f32> = (0..2_000)
            .map(|i| ((i * 7919) % 2_000) as f32 * h / 37.0 - 5.0)
            .collect();
        let layout = break_layout(&breaks, h);
        assert!(layout.windows(2).all(|w| w[0].at <= w[1].at));
        for step in 0..1_000 {
            let y = step as f32 * h / 17.0 - 3.0;
            let linear = layout
                .iter()
                .take_while(|b| b.at <= y)
                .last()
                .map_or(0.0, |b| b.shift);
            assert_eq!(shift_at(&layout, y), linear, "at y = {y}");
        }
    }

    #[test]
    fn non_finite_breaks_are_ignored() {
        assert!(break_layout(&[f32::NAN, f32::INFINITY], 800.0).is_empty());
    }

    #[test]
    fn landscape_swaps_the_page_dimensions() {
        let mut p = a4();
        p.orientation = "Landscape".to_owned();
        let g = Geometry::new(&p, 100.0);
        assert!(g.paper[0] > g.paper[1]);
    }
}
