//! Pinned golden baseline: per-sample counts and the known-failure lists. Edited by hand after
//! an intended engine change (see `golden_samples.rs`).
#![allow(dead_code)]

/// Pinned counts of one sample.
pub struct Counts {
    pub pass: usize,
    pub mismatch: usize,
    pub engine_error: usize,
    pub engine_error_no_ref: usize,
    pub no_reference: usize,
    pub plot_ok: usize,
    pub unsupported: &'static [(&'static str, usize)],
}

/// Pinned counts per sample, in file-name order.
pub const BASELINE: &[(&str, Counts)] = &[
    (
        "10_rata_18_UMP_Dreieck.mcdx",
        Counts {
            pass: 4,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 17,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "11_rata_AA_eulersche_Gerade.mcdx",
        Counts {
            pass: 5,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 25,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "12_rata_Kreisdiagramme.mcdx",
        Counts {
            pass: 3,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 4,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "13_rata_plot1.mcdx",
        Counts {
            // The 3D plot's three traces (two curves, one surface) match Prime's stored grids.
            pass: 1,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 3,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "14_rata_plot2.mcdx",
        Counts {
            // The matrix surface A(i, j) is evaluated as the sheet defines it (z(0,0) = 1/41). Prime's
            // stored grid is not that: its x and y run from -10 to 45 (the axis is -15 to 45) and its
            // z(0,0) is 1/81, which is 1/(9 + x^2 + y^2) for x(i) = -6 + 0.275 i, not for the
            // displayed x(i) = -4 + 0.2 i. The stored data does not follow the sheet's definitions.
            pass: 0,
            mismatch: 1,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 9,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "15_rata_Inkreismittelpunkt.mcdx",
        Counts {
            pass: 5,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 19,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "16_rata_Dreieck_flaeche_winkel.mcdx",
        Counts {
            pass: 12,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 16,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "17_rata_golfball_donau_lsg.mcdx",
        Counts {
            pass: 3,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 7,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "17_rata_golfball_donau_lsg_2.mcdx",
        Counts {
            pass: 4,
            mismatch: 1,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 6,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "17_rata_schiefer_wurf.mcdx",
        Counts {
            pass: 4,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 7,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "17_rata_schiefer_wurf_2.mcdx",
        Counts {
            pass: 4,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 7,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "17_rata_schiefer_wurf_3.mcdx",
        Counts {
            pass: 4,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 7,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "1_rata_1.mcdx",
        Counts {
            pass: 8,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 6,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "2_rata_2.mcdx",
        Counts {
            pass: 7,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 1,
            no_reference: 12,
            plot_ok: 0,
            unsupported: &[("symbolic result: dimension mismatch: s*A and 1", 1)],
        },
    ),
    (
        "3_rata_3_Kurve_durch_3_Punkte.mcdx",
        Counts {
            pass: 1,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 1,
            no_reference: 2,
            plot_ok: 0,
            unsupported: &[
                ("recursive function `y` in a symbolic evaluation", 1),
                (
                    "unsupported operation: recursive function `y` in a symbolic evaluation",
                    1,
                ),
            ],
        },
    ),
    (
        "4_rata_3_Kurve_durch_3_Punkte_V2.mcdx",
        Counts {
            pass: 1,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 5,
            plot_ok: 0,
            unsupported: &[
                ("recursive function `y` in a symbolic evaluation", 1),
                (
                    "unsupported operation: recursive function `y` in a symbolic evaluation",
                    1,
                ),
            ],
        },
    ),
    (
        "5_rata_4_Gerade_schneidet_Kreis.mcdx",
        Counts {
            pass: 3,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 10,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "6_rata_18_gerade_durch_2_punkte.mcdx",
        Counts {
            pass: 2,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 14,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "7_rata_18_Dreieck.mcdx",
        Counts {
            pass: 11,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 17,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "8_rata_18_Schwerpunkt_Dreieck.mcdx",
        Counts {
            pass: 2,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 10,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "9_rata_18_HSP_Dreieck.mcdx",
        Counts {
            pass: 1,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 13,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "Mechanik_1_massen_ziehen.mcdx",
        Counts {
            pass: 4,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 10,
            no_reference: 10,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "Mechanik_2_Wellendurchbiegung.mcdx",
        Counts {
            pass: 5,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 22,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
    (
        "Mechanik_3_Spannungsberechnung.mcdx",
        Counts {
            pass: 9,
            mismatch: 0,
            engine_error: 0,
            engine_error_no_ref: 0,
            no_reference: 19,
            plot_ok: 0,
            unsupported: &[],
        },
    ),
];

/// Every Mismatch and EngineError in the baseline: (sample, region id, kind, one-line reason).
/// Each reason is the classification from the review of this baseline.
pub const KNOWN_FAILURES: &[(&str, u32, &str, &str)] = &[
    (
        "17_rata_golfball_donau_lsg_2.mcdx",
        4,
        "Mismatch",
        "sample artifact: stored result 3 is golfball_1's solution (48.886 m/s), not the 40 m/s guess; Prime's result.xml is stale here",
    ),
    (
        "14_rata_plot2.mcdx",
        9,
        "Mismatch",
        "sample artifact: the stored 3D surface does not follow the sheet (x(i) = -6 + 0.275 i stored, -4 + 0.2 i displayed; grid -10..45 stored, -15..45 axis); see the baseline note",
    ),
];

/// Every EngineErrorNoRef in the baseline: an engine error on a region with no stored
/// reference. Pinned separately from the failures above, with one-line reasons. These are
/// real gaps, not hidden: `--strict` fails on them too.
pub const KNOWN_NO_REFERENCE_ERRORS: &[(&str, u32, &str)] = &[
    ("2_rata_2.mcdx", 7, "importer gap: a text-like region without a position is read as math, so `text` is undefined"),
    ("3_rata_3_Kurve_durch_3_Punkte.mcdx", 0, "engine gap: the function `clear` is not implemented"),
    ("Mechanik_1_massen_ziehen.mcdx", 8, "undefined `F_sy` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 9, "undefined `F_x` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 10, "undefined `F_sx` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 11, "undefined `F_y` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 12, "undefined `F_R1` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 13, "undefined `F_R2` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 14, "undefined `F_Sx` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 15, "undefined `F_Sy` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 16, "undefined `F_x` on the left of `==`: no earlier definition is evaluated"),
    ("Mechanik_1_massen_ziehen.mcdx", 17, "undefined `F_N2` on the left of `==`: no earlier definition is evaluated"),
];
