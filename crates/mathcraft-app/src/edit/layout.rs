//! Arranging regions on the page: align, distribute and insert vertical space. Pure geometry: the
//! session turns the targets into move commands, so one arrangement is one undo step. Positions
//! are page points, x to the right and y down, the same as `RegionModel::pos`.

/// An arrangement from the Format tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrange {
    AlignLeft,
    AlignRight,
    AlignTop,
    AlignBottom,
    /// Centres the regions on the middle of their group, horizontally.
    AlignCenterX,
    /// Centres the regions on the middle of their group, vertically.
    AlignCenterY,
    /// Equal horizontal gaps between regions, the outer two stay where they are.
    DistributeHorizontal,
    /// Equal vertical gaps between regions, the outer two stay where they are.
    DistributeVertical,
}

impl Arrange {
    /// Fewest regions the arrangement needs: two to align, three to distribute.
    pub fn min_regions(self) -> usize {
        match self {
            Arrange::DistributeHorizontal | Arrange::DistributeVertical => 3,
            _ => 2,
        }
    }
}

/// A region as the geometry sees it: its id, its top-left corner and its size. A size that is
/// not stored counts as zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box2 {
    pub id: u32,
    pub pos: (f64, f64),
    pub size: (f64, f64),
}

/// The new position of every box that `how` moves, as (id, position). Positions are never
/// negative (the page starts at 0).
pub fn arrange(items: &[Box2], how: Arrange) -> Vec<(u32, (f64, f64))> {
    if items.len() < how.min_regions() {
        return Vec::new();
    }
    match how {
        Arrange::AlignLeft => align_edge(items, Axis::X, Edge::Start),
        Arrange::AlignRight => align_edge(items, Axis::X, Edge::End),
        Arrange::AlignTop => align_edge(items, Axis::Y, Edge::Start),
        Arrange::AlignBottom => align_edge(items, Axis::Y, Edge::End),
        Arrange::AlignCenterX => align_centre(items, Axis::X),
        Arrange::AlignCenterY => align_centre(items, Axis::Y),
        Arrange::DistributeHorizontal => distribute(items, Axis::X),
        Arrange::DistributeVertical => distribute(items, Axis::Y),
    }
}

/// Moves every region at or below the line `y` down by `points`: Prime's insert lines. Returns
/// the new position of each region that moves.
pub fn insert_lines(items: &[Box2], y: f64, points: f64) -> Vec<(u32, (f64, f64))> {
    if !points.is_finite() || points <= 0.0 || !y.is_finite() {
        return Vec::new();
    }
    items
        .iter()
        .filter(|b| b.pos.1 >= y)
        .map(|b| (b.id, (b.pos.0, b.pos.1 + points)))
        .collect()
}

/// The empty space below the line `y`: the distance to the top of the nearest region that starts
/// at or below it. `None` when no region starts there.
pub fn gap_below(items: &[Box2], y: f64) -> Option<f64> {
    items
        .iter()
        .filter(|b| b.pos.1 >= y)
        .map(|b| b.pos.1 - y)
        .min_by(f64::total_cmp)
}

/// Removes up to `points` of the empty space below the line `y` (Prime's Remove Space): every
/// region at or below the line moves up by what the gap allows. `points` of `f64::INFINITY`
/// removes the whole gap. Returns the new positions.
pub fn remove_lines(items: &[Box2], y: f64, points: f64) -> Vec<(u32, (f64, f64))> {
    if points.is_nan() || points <= 0.0 || !y.is_finite() {
        return Vec::new();
    }
    let Some(gap) = gap_below(items, y) else {
        return Vec::new();
    };
    let shift = gap.min(points);
    if shift <= 0.0 {
        return Vec::new();
    }
    items
        .iter()
        .filter(|b| b.pos.1 >= y)
        .map(|b| (b.id, (b.pos.0, b.pos.1 - shift)))
        .collect()
}

/// Separates overlapping regions (Prime's Separate Regions): along `axis` each region, in order,
/// moves just past the end of the one before it when they overlap. The first one stays.
pub fn separate(items: &[Box2], vertical: bool, gap: f64) -> Vec<(u32, (f64, f64))> {
    let axis = if vertical { Axis::Y } else { Axis::X };
    let mut order: Vec<Box2> = items.to_vec();
    order.sort_by(|a, b| start(a, axis).total_cmp(&start(b, axis)));
    let mut out = Vec::new();
    let mut end = f64::NEG_INFINITY;
    for b in order {
        let mut s = start(&b, axis);
        if s < end && overlaps(&b, items) {
            s = end + gap;
            let pos = if vertical { (b.pos.0, s) } else { (s, b.pos.1) };
            out.push((b.id, pos));
        }
        end = end.max(s + extent(&b, axis));
    }
    out
}

/// True when `b` overlaps another box of `items` on both axes.
fn overlaps(b: &Box2, items: &[Box2]) -> bool {
    items.iter().any(|o| {
        o.id != b.id
            && b.pos.0 < o.pos.0 + o.size.0
            && o.pos.0 < b.pos.0 + b.size.0
            && b.pos.1 < o.pos.1 + o.size.1
            && o.pos.1 < b.pos.1 + b.size.1
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Start,
    End,
}

/// The coordinate of a box along `axis` (its left or top) and its extent along the axis.
fn start(b: &Box2, axis: Axis) -> f64 {
    match axis {
        Axis::X => b.pos.0,
        Axis::Y => b.pos.1,
    }
}

fn extent(b: &Box2, axis: Axis) -> f64 {
    match axis {
        Axis::X => b.size.0,
        Axis::Y => b.size.1,
    }
}

/// Puts a box at `at` along `axis`, keeping the other coordinate. Never negative.
fn placed(b: &Box2, axis: Axis, at: f64) -> (u32, (f64, f64)) {
    let at = at.max(0.0);
    let pos = match axis {
        Axis::X => (at, b.pos.1),
        Axis::Y => (b.pos.0, at),
    };
    (b.id, pos)
}

fn align_edge(items: &[Box2], axis: Axis, edge: Edge) -> Vec<(u32, (f64, f64))> {
    let first = items
        .iter()
        .map(|b| start(b, axis))
        .fold(f64::INFINITY, f64::min);
    let last = items
        .iter()
        .map(|b| start(b, axis) + extent(b, axis))
        .fold(f64::NEG_INFINITY, f64::max);
    items
        .iter()
        .map(|b| {
            let at = match edge {
                Edge::Start => first,
                Edge::End => last - extent(b, axis),
            };
            placed(b, axis, at)
        })
        .collect()
}

fn align_centre(items: &[Box2], axis: Axis) -> Vec<(u32, (f64, f64))> {
    let first = items
        .iter()
        .map(|b| start(b, axis))
        .fold(f64::INFINITY, f64::min);
    let last = items
        .iter()
        .map(|b| start(b, axis) + extent(b, axis))
        .fold(f64::NEG_INFINITY, f64::max);
    let middle = (first + last) / 2.0;
    items
        .iter()
        .map(|b| placed(b, axis, middle - extent(b, axis) / 2.0))
        .collect()
}

/// Equal gaps: the first box (by its coordinate) and the last keep their places; the boxes between
/// are spaced so every gap is the same. Ties keep the id order.
fn distribute(items: &[Box2], axis: Axis) -> Vec<(u32, (f64, f64))> {
    let mut sorted: Vec<Box2> = items.to_vec();
    sorted.sort_by(|a, b| {
        start(a, axis)
            .total_cmp(&start(b, axis))
            .then(a.id.cmp(&b.id))
    });
    let (Some(first), Some(last)) = (sorted.first().copied(), sorted.last().copied()) else {
        return Vec::new();
    };
    let span = start(&last, axis) + extent(&last, axis) - start(&first, axis);
    let total: f64 = sorted.iter().map(|b| extent(b, axis)).sum();
    let gaps = (sorted.len() - 1) as f64;
    let gap = (span - total) / gaps;
    let mut cursor = start(&first, axis);
    sorted
        .iter()
        .map(|b| {
            let moved = placed(b, axis, cursor);
            cursor += extent(b, axis) + gap;
            moved
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: u32, x: f64, y: f64, w: f64, h: f64) -> Box2 {
        Box2 {
            id,
            pos: (x, y),
            size: (w, h),
        }
    }

    fn pos_of(out: &[(u32, (f64, f64))], id: u32) -> (f64, f64) {
        out.iter().find(|(i, _)| *i == id).expect("moved").1
    }

    #[test]
    fn align_left_and_top_use_the_outermost_edge() {
        let items = [b(1, 30.0, 10.0, 50.0, 20.0), b(2, 10.0, 40.0, 5.0, 5.0)];
        let left = arrange(&items, Arrange::AlignLeft);
        assert_eq!(pos_of(&left, 1), (10.0, 10.0));
        assert_eq!(pos_of(&left, 2), (10.0, 40.0));
        let top = arrange(&items, Arrange::AlignTop);
        assert_eq!(pos_of(&top, 1), (30.0, 10.0));
        assert_eq!(pos_of(&top, 2), (10.0, 10.0));
    }

    #[test]
    fn align_right_and_bottom_line_up_the_far_edges() {
        let items = [b(1, 0.0, 0.0, 50.0, 20.0), b(2, 0.0, 30.0, 10.0, 5.0)];
        let right = arrange(&items, Arrange::AlignRight);
        assert_eq!(pos_of(&right, 1), (0.0, 0.0));
        assert_eq!(pos_of(&right, 2), (40.0, 30.0));
        // The lower edge of the group is at 35 (box 2 ends there).
        let bottom = arrange(&items, Arrange::AlignBottom);
        assert_eq!(pos_of(&bottom, 1), (0.0, 15.0));
        assert_eq!(pos_of(&bottom, 2), (0.0, 30.0));
    }

    #[test]
    fn centres_on_the_middle_of_the_group() {
        let items = [b(1, 0.0, 0.0, 10.0, 10.0), b(2, 40.0, 0.0, 20.0, 10.0)];
        let out = arrange(&items, Arrange::AlignCenterX);
        // Group spans 0..60, middle 30.
        assert_eq!(pos_of(&out, 1), (25.0, 0.0));
        assert_eq!(pos_of(&out, 2), (20.0, 0.0));
    }

    #[test]
    fn distribute_makes_equal_gaps_and_keeps_the_outer_boxes() {
        let items = [
            b(3, 100.0, 0.0, 10.0, 10.0),
            b(1, 0.0, 0.0, 10.0, 10.0),
            b(2, 20.0, 0.0, 30.0, 10.0),
        ];
        let out = arrange(&items, Arrange::DistributeHorizontal);
        // Span 0..110, widths total 50, two gaps of 30.
        assert_eq!(pos_of(&out, 1), (0.0, 0.0));
        assert_eq!(pos_of(&out, 2), (40.0, 0.0));
        assert_eq!(pos_of(&out, 3), (100.0, 0.0));
    }

    #[test]
    fn distribute_needs_three_regions_and_align_needs_two() {
        let two = [b(1, 0.0, 0.0, 1.0, 1.0), b(2, 5.0, 0.0, 1.0, 1.0)];
        assert!(arrange(&two, Arrange::DistributeVertical).is_empty());
        assert_eq!(arrange(&two, Arrange::AlignTop).len(), 2);
        assert!(arrange(&two[..1], Arrange::AlignTop).is_empty());
    }

    #[test]
    fn positions_never_go_negative() {
        // Centring a box wider than the group would start it left of the page edge.
        let items = [b(1, 0.0, 0.0, 100.0, 10.0), b(2, 0.0, 0.0, 10.0, 10.0)];
        let out = arrange(&items, Arrange::AlignCenterX);
        assert!(out.iter().all(|(_, p)| p.0 >= 0.0 && p.1 >= 0.0));
    }

    #[test]
    fn remove_lines_closes_the_gap_but_never_more() {
        let items = [b(1, 5.0, 10.0, 1.0, 1.0), b(2, 5.0, 50.0, 1.0, 1.0)];
        // The gap below y = 20 is 30 points.
        assert_eq!(remove_lines(&items, 20.0, 12.0), vec![(2, (5.0, 38.0))]);
        assert_eq!(remove_lines(&items, 20.0, 100.0), vec![(2, (5.0, 20.0))]);
        assert_eq!(
            remove_lines(&items, 20.0, f64::INFINITY),
            vec![(2, (5.0, 20.0))]
        );
        assert!(remove_lines(&items, 60.0, 12.0).is_empty());
        assert!(remove_lines(&items, 50.0, 12.0).is_empty());
    }

    #[test]
    fn separate_pushes_overlapping_regions_apart() {
        let items = [b(1, 0.0, 0.0, 100.0, 40.0), b(2, 10.0, 20.0, 100.0, 40.0)];
        assert_eq!(separate(&items, true, 6.0), vec![(2, (10.0, 46.0))]);
        assert_eq!(separate(&items, false, 6.0), vec![(2, (106.0, 20.0))]);
        let apart = [b(1, 0.0, 0.0, 10.0, 10.0), b(2, 0.0, 50.0, 10.0, 10.0)];
        assert!(separate(&apart, true, 6.0).is_empty());
    }

    #[test]
    fn insert_lines_moves_only_regions_at_or_below_the_line() {
        let items = [b(1, 5.0, 10.0, 1.0, 1.0), b(2, 5.0, 50.0, 1.0, 1.0)];
        let out = insert_lines(&items, 30.0, 12.0);
        assert_eq!(out, vec![(2, (5.0, 62.0))]);
        assert!(insert_lines(&items, 30.0, 0.0).is_empty());
        assert!(insert_lines(&items, f64::NAN, 12.0).is_empty());
    }
}
