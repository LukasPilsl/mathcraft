//! Resizing an image region by one of its corner handles. Pure geometry, no egui: the session turns
//! the result into one undo step, and the canvas draws the handles at the corners.

/// A corner handle of a selected image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// Every handle, in the order the canvas tests them.
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    /// True for the handles on the right edge: dragging them away from the image grows it.
    #[must_use]
    pub fn right(self) -> bool {
        matches!(self, Corner::TopRight | Corner::BottomRight)
    }

    /// True for the handles on the bottom edge.
    #[must_use]
    pub fn bottom(self) -> bool {
        matches!(self, Corner::BottomLeft | Corner::BottomRight)
    }
}

/// The smallest side an image can be resized to, in page points.
pub const MIN_SIDE_PT: f64 = 12.0;

/// The `(position, size)` of a box after its `corner` is dragged by `delta` (page points). The box
/// keeps its aspect ratio: the axis that moved more sets the scale. The opposite corner stays where
/// it is. Neither side gets below [`MIN_SIDE_PT`]. `size` must have two positive sides.
#[must_use]
pub fn resized(
    pos: (f64, f64),
    size: (f64, f64),
    corner: Corner,
    delta: [f64; 2],
) -> ((f64, f64), (f64, f64)) {
    let (x, y) = pos;
    let (w, h) = size;
    let grow_x = if corner.right() { delta[0] } else { -delta[0] };
    let grow_y = if corner.bottom() { delta[1] } else { -delta[1] };
    let scale_x = (w + grow_x) / w;
    let scale_y = (h + grow_y) / h;
    let scale = if (scale_x - 1.0).abs() >= (scale_y - 1.0).abs() {
        scale_x
    } else {
        scale_y
    };
    let scale = scale.max(MIN_SIDE_PT / w.min(h));
    let (new_w, new_h) = (w * scale, h * scale);
    let new_x = if corner.right() { x } else { x + w - new_w };
    let new_y = if corner.bottom() { y } else { y + h - new_h };
    ((new_x, new_y), (new_w, new_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn dragging_bottom_right_grows_and_keeps_the_top_left() {
        let (pos, size) = resized(
            (10.0, 20.0),
            (100.0, 50.0),
            Corner::BottomRight,
            [50.0, 0.0],
        );
        assert!(close(pos, (10.0, 20.0)));
        // Width grew by 50, so the scale is 1.5 and the height follows.
        assert!(close(size, (150.0, 75.0)));
    }

    #[test]
    fn dragging_top_left_outwards_grows_and_keeps_the_bottom_right() {
        let (pos, size) = resized((100.0, 100.0), (100.0, 50.0), Corner::TopLeft, [-20.0, 0.0]);
        assert!(close(size, (120.0, 60.0)));
        // The bottom-right corner (200, 150) stays put.
        assert!(close((pos.0 + size.0, pos.1 + size.1), (200.0, 150.0)));
    }

    #[test]
    fn dragging_inwards_shrinks() {
        let (_, size) = resized((0.0, 0.0), (100.0, 50.0), Corner::BottomRight, [-25.0, 0.0]);
        assert!(close(size, (75.0, 37.5)));
    }

    #[test]
    fn the_size_never_goes_below_the_minimum() {
        let (_, size) = resized(
            (0.0, 0.0),
            (100.0, 50.0),
            Corner::BottomRight,
            [-500.0, 0.0],
        );
        assert!(size.0 >= MIN_SIDE_PT - 1e-9);
        assert!(size.1 >= MIN_SIDE_PT - 1e-9);
        assert!(close(size, (MIN_SIDE_PT * 2.0, MIN_SIDE_PT)));
    }

    #[test]
    fn no_movement_changes_nothing() {
        for corner in Corner::ALL {
            let (pos, size) = resized((5.0, 6.0), (70.0, 30.0), corner, [0.0, 0.0]);
            assert!(close(pos, (5.0, 6.0)));
            assert!(close(size, (70.0, 30.0)));
        }
    }
}
