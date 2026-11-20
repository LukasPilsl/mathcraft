//! Text rendering: the outcome column and the golden report as Markdown.

use mathcraft_core::eval::Outcome;
use mathcraft_import::compare::Category;
use mathcraft_import::describe::describe_value;

use crate::golden::GoldenRun;

/// Renders an outcome for the actual column, for example `Defined(x)` or `error: ...`.
pub fn describe_outcome(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Defined { name, .. } => format!("Defined({name})"),
        Outcome::Value(v) => describe_value(v),
        Outcome::Converted { value, unit, shown } => format!(
            "{} shown as {} in {}",
            describe_value(value),
            describe_value(shown),
            unit.terms()
                .iter()
                .map(|(u, p)| {
                    let (n, d) = (*p.numer(), *p.denom());
                    let mut s = u.symbol.to_string();
                    if n != 1 || d != 1 {
                        s.push_str(&format!("^{n}"));
                        if d != 1 {
                            s.push_str(&format!("/{d}"));
                        }
                    }
                    s
                })
                .collect::<Vec<_>>()
                .join("*")
        ),
        Outcome::Accepted => "constraint accepted".to_string(),
        Outcome::Error(e) => format!("error: {e}"),
    }
}

/// Markdown tables for a golden run, then the verbose details if asked.
pub fn render_run(run: &GoldenRun, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("## Per worksheet\n\n");
    out.push_str(
        "| sample | Pass | Mismatch | Unsupported | EngineError | EngineErrorNoRef | NoReference | PlotOk | Plot Pass | Plot Mismatch |\n",
    );
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for file in &run.files {
        let t = file.tally();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            file.name,
            t.pass,
            t.mismatch,
            t.unsupported_total(),
            t.engine_error,
            t.engine_error_no_ref,
            t.no_reference,
            t.plot_ok,
            t.plot_pass,
            t.plot_mismatch
        ));
    }
    for file in &run.files {
        if let Some(e) = &file.file_error {
            out.push_str(&format!("\nFILE ERROR {}: {e}\n", file.name));
        }
        for note in &file.notes {
            out.push_str(&format!("note {}: {note}\n", file.name));
        }
    }

    let total = run.tally();
    out.push_str("\n## Totals\n\n");
    out.push_str("| category | count |\n|---|---:|\n");
    out.push_str(&format!("| Pass | {} |\n", total.pass));
    out.push_str(&format!("| Mismatch | {} |\n", total.mismatch));
    out.push_str(&format!(
        "| Unsupported | {} |\n",
        total.unsupported_total()
    ));
    out.push_str(&format!("| EngineError | {} |\n", total.engine_error));
    out.push_str(&format!(
        "| EngineErrorNoRef | {} |\n",
        total.engine_error_no_ref
    ));
    out.push_str(&format!("| NoReference | {} |\n", total.no_reference));
    out.push_str(&format!("| PlotOk | {} |\n", total.plot_ok));
    out.push_str(&format!(
        "| of which plots: Pass | {} |\n| of which plots: Mismatch | {} |\n",
        total.plot_pass, total.plot_mismatch
    ));
    out.push_str(&format!(
        "\nweak error passes: {} (stored error code other than bad%_variable, any engine error accepted; included in Pass)\n",
        total.weak_pass
    ));
    if !total.unsupported.is_empty() {
        out.push_str("\n### Unsupported by reason\n\n| reason | count |\n|---|---:|\n");
        for (reason, count) in &total.unsupported {
            out.push_str(&format!("| {reason} | {count} |\n"));
        }
    }
    out.push_str(&format!(
        "\nFailures (Mismatch + EngineError + EngineErrorNoRef + file errors): {}\n",
        run.failure_count()
    ));

    if verbose {
        out.push_str("\n## Mismatches and engine errors (with and without a reference)\n");
        for file in &run.files {
            for row in &file.rows {
                let (kind, reason) = match &row.category {
                    Category::Mismatch(r) => ("MISMATCH", r),
                    Category::EngineError(r) => ("ENGINE ERROR", r),
                    Category::EngineErrorNoRef(r) => ("ENGINE ERROR (no reference)", r),
                    _ => continue,
                };
                out.push_str(&format!(
                    "\n{kind} {} region #{}\n  expression: {}\n  expected:   {}\n  actual:     {}\n  reason:     {reason}\n",
                    file.name,
                    row.region_id,
                    row.expr,
                    row.expected.as_deref().unwrap_or("n/a"),
                    row.actual.as_deref().unwrap_or("n/a"),
                ));
            }
        }
    }
    out
}
