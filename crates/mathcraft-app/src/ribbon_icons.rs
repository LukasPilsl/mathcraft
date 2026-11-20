//! Vector icons of the ribbon, drawn with the egui painter. They are our own drawings: each icon
//! is a short list of strokes, dots and rings on a unit square (x to the right, y down), scaled to
//! the button's icon rectangle when painted. The geometry lives here, free of egui, so the tests
//! can check it without a window.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// One stroke of an icon, on the unit square.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mark {
    /// A polyline through these points.
    Line(&'static [[f32; 2]]),
    /// The outline of a rectangle `[left, top, right, bottom]`.
    Box([f32; 4]),
    /// A filled dot at the centre, with its radius.
    Dot([f32; 2], f32),
    /// A circle outline at the centre, with its radius.
    Ring([f32; 2], f32),
    /// Text in the middle of the icon (for the letters B, I, A, ? that are not drawn as strokes).
    Glyph(&'static str),
}

/// The icons of the ribbon. `Blank` is the icon of a button without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Blank,
    /// A short text drawn as the icon (for the ribbon buttons that have no drawing of their own).
    Text(&'static str),
    Region,
    TextRegion,
    Fraction,
    Power,
    Subscript,
    SquareRoot,
    Define,
    Equals,
    Arrow,
    Symbols,
    Sum,
    Product,
    Integral,
    Derivative,
    Function,
    Unit,
    Image,
    Area,
    AreaAround,
    Worksheet,
    Matrix,
    Matrix2x2,
    Table,
    Plus,
    Minus,
    Plot2d,
    Plot3d,
    Calculate,
    Recalculate,
    Options,
    Folder,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Grid,
    Borders,
    PageLayout,
    Globe,
    About,
    Keyboard,
    Help,
    New,
    Save,
    ExportPdf,
    Print,
    PageSetup,
    Bold,
    Italic,
    Font,
    AlignLeft,
    AlignRight,
    AlignTop,
    AlignBottom,
    AlignCentre,
    AlignMiddle,
    DistributeH,
    DistributeV,
    InsertLines,
    ResultFormat,
}

/// The icon of a ribbon button, by its key in `i18n.rs`.
pub fn icon_for(key: &str) -> Icon {
    match key {
        "btn.math_region" => Icon::Region,
        "btn.text_region" => Icon::TextRegion,
        "btn.fraction" => Icon::Fraction,
        "btn.power" => Icon::Power,
        "btn.square_root" => Icon::SquareRoot,
        "btn.subscript" => Icon::Subscript,
        "btn.define" => Icon::Define,
        "btn.evaluate" => Icon::Equals,
        "btn.symbolic" => Icon::Arrow,
        "btn.sum" => Icon::Sum,
        "btn.product" => Icon::Product,
        "btn.integral" => Icon::Integral,
        "btn.derivative" => Icon::Derivative,
        "btn.symbols" => Icon::Symbols,
        "btn.function" | "btn.all_functions" => Icon::Function,
        "btn.unit" => Icon::Unit,
        "btn.image" => Icon::Image,
        "btn.area" => Icon::Area,
        "btn.area_around" => Icon::AreaAround,
        "btn.worksheet" => Icon::Worksheet,
        "btn.matrix" => Icon::Matrix,
        "btn.matrix_2x2" => Icon::Matrix2x2,
        "btn.table" => Icon::Table,
        "btn.matrix_add_row" | "btn.matrix_add_column" => Icon::Plus,
        "btn.matrix_remove_row" | "btn.matrix_remove_column" => Icon::Minus,
        "btn.xy_plot" | "btn.polar_plot" | "btn.edit_plot" => Icon::Plot2d,
        "btn.plot_3d" => Icon::Plot3d,
        "btn.calculate" | "btn.auto_calc" => Icon::Calculate,
        "btn.recalculate" => Icon::Recalculate,
        "btn.worksheet_options" => Icon::Options,
        "btn.file_access" => Icon::Folder,
        "btn.zoom_in" => Icon::ZoomIn,
        "btn.zoom_out" => Icon::ZoomOut,
        "btn.zoom_reset" => Icon::ZoomReset,
        "btn.grid" => Icon::Grid,
        "btn.region_borders" => Icon::Borders,
        "btn.page_layout" => Icon::PageLayout,
        "btn.about" => Icon::About,
        "btn.shortcuts" => Icon::Keyboard,
        "btn.help_topics" => Icon::Help,
        "btn.new" => Icon::New,
        "btn.save" | "btn.save_as" => Icon::Save,
        "btn.export_pdf" => Icon::ExportPdf,
        "btn.print" => Icon::Print,
        "btn.page_setup" => Icon::PageSetup,
        "btn.open" => Icon::Folder,
        "btn.bold" => Icon::Bold,
        "btn.italic" => Icon::Italic,
        "btn.font" => Icon::Font,
        "btn.align_left" => Icon::AlignLeft,
        "btn.align_right" => Icon::AlignRight,
        "btn.align_top" => Icon::AlignTop,
        "btn.align_bottom" => Icon::AlignBottom,
        "btn.align_hcentre" => Icon::AlignCentre,
        "btn.align_vcentre" => Icon::AlignMiddle,
        "btn.distribute_h" => Icon::DistributeH,
        "btn.distribute_v" => Icon::DistributeV,
        "btn.insert_lines" => Icon::InsertLines,
        "btn.results_format" => Icon::ResultFormat,
        _ => Icon::Blank,
    }
}

/// The strokes of an icon on the unit square.
pub fn marks(icon: Icon) -> &'static [Mark] {
    use Mark::*;
    match icon {
        Icon::Blank | Icon::Text(_) => &[],
        Icon::Region => &[
            Box([0.12, 0.2, 0.88, 0.8]),
            Line(&[[0.3, 0.4], [0.7, 0.4]]),
            Line(&[[0.3, 0.6], [0.7, 0.6]]),
        ],
        Icon::TextRegion => &[
            Box([0.12, 0.2, 0.88, 0.8]),
            Line(&[[0.3, 0.35], [0.7, 0.35]]),
            Line(&[[0.5, 0.35], [0.5, 0.68]]),
        ],
        Icon::Fraction => &[
            Line(&[[0.12, 0.5], [0.88, 0.5]]),
            Box([0.34, 0.12, 0.66, 0.38]),
            Box([0.34, 0.62, 0.66, 0.88]),
        ],
        Icon::Power => &[
            Line(&[[0.08, 0.42], [0.5, 0.92]]),
            Line(&[[0.5, 0.42], [0.08, 0.92]]),
            Line(&[[0.6, 0.14], [0.9, 0.14], [0.6, 0.4], [0.9, 0.4]]),
        ],
        Icon::Subscript => &[
            Line(&[[0.08, 0.3], [0.5, 0.8]]),
            Line(&[[0.5, 0.3], [0.08, 0.8]]),
            Line(&[[0.6, 0.62], [0.9, 0.62], [0.6, 0.88], [0.9, 0.88]]),
        ],
        Icon::SquareRoot => &[Line(&[
            [0.08, 0.55],
            [0.28, 0.5],
            [0.46, 0.92],
            [0.72, 0.1],
            [0.92, 0.1],
        ])],
        Icon::Define => &[
            Dot([0.2, 0.36], 0.07),
            Dot([0.2, 0.64], 0.07),
            Line(&[[0.42, 0.3], [0.9, 0.3]]),
            Line(&[[0.42, 0.7], [0.9, 0.7]]),
        ],
        Icon::Equals => &[
            Line(&[[0.12, 0.38], [0.88, 0.38]]),
            Line(&[[0.12, 0.62], [0.88, 0.62]]),
        ],
        Icon::Arrow => &[
            Line(&[[0.12, 0.5], [0.86, 0.5]]),
            Line(&[[0.62, 0.26], [0.86, 0.5], [0.62, 0.74]]),
        ],
        Icon::Symbols => &[
            Dot([0.3, 0.3], 0.09),
            Dot([0.7, 0.3], 0.09),
            Dot([0.3, 0.7], 0.09),
            Dot([0.7, 0.7], 0.09),
        ],
        Icon::Sum => &[Line(&[
            [0.82, 0.14],
            [0.18, 0.14],
            [0.5, 0.5],
            [0.18, 0.86],
            [0.82, 0.86],
        ])],
        Icon::Product => &[
            Line(&[[0.12, 0.86], [0.12, 0.18], [0.88, 0.18], [0.88, 0.86]]),
            Line(&[[0.36, 0.18], [0.36, 0.86]]),
            Line(&[[0.64, 0.18], [0.64, 0.86]]),
        ],
        Icon::Integral => &[Line(&[
            [0.68, 0.1],
            [0.58, 0.08],
            [0.5, 0.2],
            [0.46, 0.5],
            [0.42, 0.8],
            [0.34, 0.92],
            [0.24, 0.9],
        ])],
        Icon::Derivative => &[
            Ring([0.3, 0.66], 0.16),
            Line(&[[0.46, 0.2], [0.46, 0.8]]),
            Line(&[[0.12, 0.5], [0.88, 0.5]]),
        ],
        Icon::Function => &[
            Line(&[[0.66, 0.12], [0.5, 0.1], [0.4, 0.24], [0.4, 0.88]]),
            Line(&[[0.24, 0.42], [0.6, 0.42]]),
        ],
        Icon::Unit => &[
            Box([0.1, 0.34, 0.9, 0.66]),
            Line(&[[0.25, 0.34], [0.25, 0.5]]),
            Line(&[[0.4, 0.34], [0.4, 0.56]]),
            Line(&[[0.55, 0.34], [0.55, 0.5]]),
            Line(&[[0.7, 0.34], [0.7, 0.56]]),
            Line(&[[0.85, 0.34], [0.85, 0.5]]),
        ],
        Icon::Image => &[
            Box([0.1, 0.15, 0.9, 0.85]),
            Line(&[
                [0.16, 0.74],
                [0.4, 0.44],
                [0.56, 0.6],
                [0.66, 0.5],
                [0.84, 0.74],
            ]),
            Dot([0.7, 0.3], 0.08),
        ],
        Icon::Area => &[
            Box([0.1, 0.18, 0.9, 0.86]),
            Line(&[[0.1, 0.3], [0.9, 0.3]]),
            Box([0.22, 0.42, 0.44, 0.6]),
            Box([0.52, 0.42, 0.78, 0.6]),
        ],
        Icon::AreaAround => &[
            Box([0.06, 0.14, 0.94, 0.9]),
            Box([0.2, 0.34, 0.42, 0.5]),
            Box([0.52, 0.6, 0.8, 0.76]),
        ],
        Icon::Worksheet => &[
            Box([0.2, 0.1, 0.8, 0.9]),
            Line(&[[0.3, 0.3], [0.7, 0.3]]),
            Line(&[[0.3, 0.45], [0.7, 0.45]]),
            Line(&[[0.3, 0.6], [0.7, 0.6]]),
            Line(&[[0.3, 0.75], [0.7, 0.75]]),
        ],
        Icon::Matrix => &[
            Line(&[[0.3, 0.1], [0.14, 0.1], [0.14, 0.9], [0.3, 0.9]]),
            Line(&[[0.7, 0.1], [0.86, 0.1], [0.86, 0.9], [0.7, 0.9]]),
            Dot([0.34, 0.3], 0.06),
            Dot([0.5, 0.3], 0.06),
            Dot([0.66, 0.3], 0.06),
            Dot([0.34, 0.5], 0.06),
            Dot([0.5, 0.5], 0.06),
            Dot([0.66, 0.5], 0.06),
            Dot([0.34, 0.7], 0.06),
            Dot([0.5, 0.7], 0.06),
            Dot([0.66, 0.7], 0.06),
        ],
        Icon::Matrix2x2 => &[
            Line(&[[0.3, 0.1], [0.14, 0.1], [0.14, 0.9], [0.3, 0.9]]),
            Line(&[[0.7, 0.1], [0.86, 0.1], [0.86, 0.9], [0.7, 0.9]]),
            Line(&[[0.5, 0.3], [0.5, 0.7]]),
            Line(&[[0.3, 0.5], [0.7, 0.5]]),
        ],
        Icon::Table => &[
            Box([0.1, 0.15, 0.9, 0.85]),
            Line(&[[0.1, 0.4], [0.9, 0.4]]),
            Line(&[[0.1, 0.62], [0.9, 0.62]]),
            Line(&[[0.5, 0.15], [0.5, 0.85]]),
        ],
        Icon::Plus => &[
            Line(&[[0.5, 0.18], [0.5, 0.82]]),
            Line(&[[0.18, 0.5], [0.82, 0.5]]),
        ],
        Icon::Minus => &[Line(&[[0.18, 0.5], [0.82, 0.5]])],
        Icon::Plot2d => &[
            Line(&[[0.14, 0.1], [0.14, 0.86], [0.9, 0.86]]),
            Line(&[[0.2, 0.7], [0.4, 0.3], [0.6, 0.6], [0.86, 0.2]]),
        ],
        Icon::Plot3d => &[
            Line(&[
                [0.14, 0.36],
                [0.5, 0.16],
                [0.86, 0.36],
                [0.5, 0.56],
                [0.14, 0.36],
            ]),
            Line(&[
                [0.14, 0.36],
                [0.14, 0.7],
                [0.5, 0.9],
                [0.86, 0.7],
                [0.86, 0.36],
            ]),
            Line(&[[0.5, 0.56], [0.5, 0.9]]),
        ],
        Icon::Calculate => &[
            Box([0.2, 0.1, 0.8, 0.9]),
            Box([0.3, 0.2, 0.7, 0.36]),
            Dot([0.36, 0.56], 0.05),
            Dot([0.64, 0.56], 0.05),
            Dot([0.36, 0.74], 0.05),
            Dot([0.64, 0.74], 0.05),
        ],
        Icon::Recalculate => &[
            Line(&[
                [0.16, 0.46],
                [0.22, 0.27],
                [0.4, 0.14],
                [0.6, 0.13],
                [0.8, 0.26],
                [0.86, 0.46],
            ]),
            Line(&[[0.66, 0.26], [0.86, 0.46], [0.92, 0.22]]),
            Line(&[
                [0.84, 0.54],
                [0.78, 0.73],
                [0.6, 0.86],
                [0.4, 0.87],
                [0.2, 0.74],
                [0.14, 0.54],
            ]),
            Line(&[[0.08, 0.78], [0.14, 0.54], [0.34, 0.64]]),
        ],
        Icon::Options => &[
            Ring([0.5, 0.5], 0.2),
            Line(&[[0.5, 0.06], [0.5, 0.2]]),
            Line(&[[0.5, 0.8], [0.5, 0.94]]),
            Line(&[[0.06, 0.5], [0.2, 0.5]]),
            Line(&[[0.8, 0.5], [0.94, 0.5]]),
        ],
        Icon::Folder => &[Line(&[
            [0.1, 0.24],
            [0.1, 0.84],
            [0.9, 0.84],
            [0.9, 0.36],
            [0.46, 0.36],
            [0.38, 0.24],
            [0.1, 0.24],
        ])],
        Icon::ZoomIn => &[
            Ring([0.42, 0.42], 0.3),
            Line(&[[0.64, 0.64], [0.9, 0.9]]),
            Line(&[[0.28, 0.42], [0.56, 0.42]]),
            Line(&[[0.42, 0.28], [0.42, 0.56]]),
        ],
        Icon::ZoomOut => &[
            Ring([0.42, 0.42], 0.3),
            Line(&[[0.64, 0.64], [0.9, 0.9]]),
            Line(&[[0.28, 0.42], [0.56, 0.42]]),
        ],
        Icon::ZoomReset => &[
            Ring([0.42, 0.42], 0.3),
            Line(&[[0.64, 0.64], [0.9, 0.9]]),
            Dot([0.42, 0.42], 0.07),
        ],
        Icon::Grid => &[
            Box([0.12, 0.12, 0.88, 0.88]),
            Line(&[[0.38, 0.12], [0.38, 0.88]]),
            Line(&[[0.62, 0.12], [0.62, 0.88]]),
            Line(&[[0.12, 0.38], [0.88, 0.38]]),
            Line(&[[0.12, 0.62], [0.88, 0.62]]),
        ],
        Icon::Borders => &[
            Box([0.16, 0.2, 0.84, 0.8]),
            Dot([0.16, 0.2], 0.06),
            Dot([0.84, 0.2], 0.06),
            Dot([0.16, 0.8], 0.06),
            Dot([0.84, 0.8], 0.06),
        ],
        Icon::PageLayout => &[
            Box([0.22, 0.1, 0.78, 0.9]),
            Line(&[[0.32, 0.3], [0.68, 0.3]]),
            Line(&[[0.32, 0.5], [0.68, 0.5]]),
            Line(&[[0.32, 0.7], [0.68, 0.7]]),
        ],
        Icon::Globe => &[
            Ring([0.5, 0.5], 0.4),
            Line(&[[0.5, 0.1], [0.3, 0.5], [0.5, 0.9], [0.7, 0.5], [0.5, 0.1]]),
            Line(&[[0.1, 0.5], [0.9, 0.5]]),
        ],
        Icon::About => &[
            Ring([0.5, 0.5], 0.4),
            Dot([0.5, 0.3], 0.05),
            Line(&[[0.5, 0.44], [0.5, 0.74]]),
        ],
        Icon::Keyboard => &[
            Box([0.08, 0.24, 0.92, 0.76]),
            Dot([0.26, 0.38], 0.05),
            Dot([0.5, 0.38], 0.05),
            Dot([0.74, 0.38], 0.05),
            Line(&[[0.3, 0.6], [0.7, 0.6]]),
        ],
        Icon::Help => &[Ring([0.5, 0.5], 0.4), Glyph("?")],
        Icon::New => &[
            Line(&[
                [0.2, 0.1],
                [0.6, 0.1],
                [0.8, 0.3],
                [0.8, 0.9],
                [0.2, 0.9],
                [0.2, 0.1],
            ]),
            Line(&[[0.38, 0.6], [0.62, 0.6]]),
            Line(&[[0.5, 0.48], [0.5, 0.72]]),
        ],
        Icon::Save => &[
            Line(&[
                [0.12, 0.12],
                [0.74, 0.12],
                [0.88, 0.26],
                [0.88, 0.88],
                [0.12, 0.88],
                [0.12, 0.12],
            ]),
            Box([0.28, 0.12, 0.7, 0.4]),
            Box([0.24, 0.6, 0.76, 0.88]),
        ],
        Icon::ExportPdf => &[
            Line(&[[0.2, 0.08], [0.6, 0.08], [0.78, 0.26], [0.78, 0.6]]),
            Line(&[[0.2, 0.08], [0.2, 0.6]]),
            Line(&[[0.5, 0.5], [0.5, 0.92]]),
            Line(&[[0.36, 0.78], [0.5, 0.92], [0.64, 0.78]]),
        ],
        Icon::Print => &[
            Box([0.26, 0.1, 0.74, 0.4]),
            Box([0.1, 0.4, 0.9, 0.74]),
            Box([0.26, 0.6, 0.74, 0.9]),
        ],
        Icon::PageSetup => &[
            Box([0.2, 0.1, 0.8, 0.9]),
            Line(&[[0.3, 0.3], [0.7, 0.3]]),
            Line(&[[0.3, 0.5], [0.7, 0.5]]),
            Line(&[[0.3, 0.7], [0.56, 0.7]]),
        ],
        Icon::Bold => &[Glyph("B")],
        Icon::Italic => &[Glyph("I")],
        Icon::Font => &[Glyph("A")],
        Icon::AlignLeft => &[
            Line(&[[0.12, 0.1], [0.12, 0.9]]),
            Box([0.22, 0.24, 0.8, 0.42]),
            Box([0.22, 0.58, 0.6, 0.76]),
        ],
        Icon::AlignRight => &[
            Line(&[[0.88, 0.1], [0.88, 0.9]]),
            Box([0.2, 0.24, 0.78, 0.42]),
            Box([0.4, 0.58, 0.78, 0.76]),
        ],
        Icon::AlignTop => &[
            Line(&[[0.1, 0.12], [0.9, 0.12]]),
            Box([0.22, 0.2, 0.42, 0.8]),
            Box([0.58, 0.2, 0.78, 0.6]),
        ],
        Icon::AlignBottom => &[
            Line(&[[0.1, 0.88], [0.9, 0.88]]),
            Box([0.22, 0.2, 0.42, 0.8]),
            Box([0.58, 0.4, 0.78, 0.8]),
        ],
        Icon::AlignCentre => &[
            Line(&[[0.5, 0.1], [0.5, 0.9]]),
            Box([0.2, 0.24, 0.8, 0.42]),
            Box([0.3, 0.58, 0.7, 0.76]),
        ],
        Icon::AlignMiddle => &[
            Line(&[[0.1, 0.5], [0.9, 0.5]]),
            Box([0.22, 0.2, 0.42, 0.42]),
            Box([0.58, 0.58, 0.78, 0.8]),
        ],
        Icon::DistributeH => &[
            Box([0.08, 0.3, 0.26, 0.7]),
            Box([0.42, 0.3, 0.58, 0.7]),
            Box([0.74, 0.3, 0.92, 0.7]),
        ],
        Icon::DistributeV => &[
            Box([0.3, 0.08, 0.7, 0.26]),
            Box([0.3, 0.42, 0.7, 0.58]),
            Box([0.3, 0.74, 0.7, 0.92]),
        ],
        Icon::InsertLines => &[
            Line(&[[0.14, 0.24], [0.86, 0.24]]),
            Line(&[[0.14, 0.76], [0.86, 0.76]]),
            Line(&[[0.5, 0.36], [0.5, 0.64]]),
        ],
        Icon::ResultFormat => &[
            Box([0.1, 0.2, 0.9, 0.8]),
            Line(&[[0.24, 0.4], [0.76, 0.4]]),
            Line(&[[0.24, 0.6], [0.56, 0.6]]),
        ],
    }
}

/// Paints `icon` inside `rect` in `colour`. `width` is the stroke width in points.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, colour: Color32, width: f32) {
    let stroke = Stroke::new(width, colour);
    let at = |p: [f32; 2]| {
        Pos2::new(
            rect.left() + p[0] * rect.width(),
            rect.top() + p[1] * rect.height(),
        )
    };
    let scale = rect.width().min(rect.height());
    if let Icon::Text(text) = icon {
        let size = if text.chars().count() > 2 { 0.62 } else { 0.9 };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(scale * size),
            colour,
        );
        return;
    }
    for mark in marks(icon) {
        match *mark {
            Mark::Line(points) => {
                let pts: Vec<Pos2> = points.iter().map(|&p| at(p)).collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
            Mark::Box([l, t, r, b]) => {
                let rect = Rect::from_min_max(at([l, t]), at([r, b]));
                painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Middle);
            }
            Mark::Dot(c, radius) => {
                painter.circle_filled(at(c), radius * scale, colour);
            }
            Mark::Ring(c, radius) => {
                painter.circle_stroke(at(c), radius * scale, stroke);
            }
            Mark::Glyph(text) => {
                painter.text(
                    at([0.5, 0.5]),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(scale * 0.9),
                    colour,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 60] = [
        Icon::Blank,
        Icon::Region,
        Icon::TextRegion,
        Icon::Fraction,
        Icon::Power,
        Icon::Subscript,
        Icon::SquareRoot,
        Icon::Define,
        Icon::Equals,
        Icon::Arrow,
        Icon::Symbols,
        Icon::Sum,
        Icon::Product,
        Icon::Integral,
        Icon::Derivative,
        Icon::Function,
        Icon::Unit,
        Icon::Image,
        Icon::Area,
        Icon::AreaAround,
        Icon::Worksheet,
        Icon::Matrix,
        Icon::Matrix2x2,
        Icon::Table,
        Icon::Plus,
        Icon::Minus,
        Icon::Plot2d,
        Icon::Plot3d,
        Icon::Calculate,
        Icon::Recalculate,
        Icon::Options,
        Icon::Folder,
        Icon::ZoomIn,
        Icon::ZoomOut,
        Icon::ZoomReset,
        Icon::Grid,
        Icon::Borders,
        Icon::PageLayout,
        Icon::Globe,
        Icon::About,
        Icon::Keyboard,
        Icon::Help,
        Icon::New,
        Icon::Save,
        Icon::ExportPdf,
        Icon::Print,
        Icon::PageSetup,
        Icon::Bold,
        Icon::Italic,
        Icon::Font,
        Icon::AlignLeft,
        Icon::AlignRight,
        Icon::AlignTop,
        Icon::AlignBottom,
        Icon::AlignCentre,
        Icon::AlignMiddle,
        Icon::DistributeH,
        Icon::DistributeV,
        Icon::InsertLines,
        Icon::ResultFormat,
    ];

    fn inside(p: [f32; 2]) -> bool {
        (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1])
    }

    #[test]
    fn every_stroke_lies_on_the_unit_square() {
        for icon in ALL {
            for mark in marks(icon) {
                match *mark {
                    Mark::Line(points) => {
                        assert!(points.len() >= 2, "{icon:?}: a line needs two points");
                        assert!(points.iter().all(|&p| inside(p)), "{icon:?}");
                    }
                    Mark::Box([l, t, r, b]) => {
                        assert!(l < r && t < b, "{icon:?}: empty box");
                        assert!(inside([l, t]) && inside([r, b]), "{icon:?}");
                    }
                    Mark::Dot(c, radius) | Mark::Ring(c, radius) => {
                        assert!(inside(c), "{icon:?}");
                        assert!(radius > 0.0 && radius < 0.5, "{icon:?}");
                        // The whole circle stays inside the square.
                        for side in [c[0] - radius, c[0] + radius, c[1] - radius, c[1] + radius] {
                            assert!((-0.01..=1.01).contains(&side), "{icon:?}");
                        }
                    }
                    Mark::Glyph(text) => assert!(!text.is_empty(), "{icon:?}"),
                }
            }
        }
    }

    #[test]
    fn only_blank_has_no_strokes() {
        for icon in ALL {
            assert_eq!(marks(icon).is_empty(), icon == Icon::Blank, "{icon:?}");
        }
    }

    #[test]
    fn ribbon_keys_have_their_own_icons() {
        assert_eq!(icon_for("btn.fraction"), Icon::Fraction);
        assert_eq!(icon_for("btn.integral"), Icon::Integral);
        assert_eq!(icon_for("btn.image"), Icon::Image);
        assert_eq!(icon_for("btn.area"), Icon::Area);
        assert_eq!(icon_for("btn.area_around"), Icon::AreaAround);
        assert_eq!(icon_for("btn.no_such_button"), Icon::Blank);
    }
}
