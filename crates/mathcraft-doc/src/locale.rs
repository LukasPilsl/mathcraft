//! Messages and the decimal comma. The document API speaks English. A front end that translates
//! (the GUI) installs a [`Translator`] and sets the decimal comma, and the same code then gives its
//! messages in the display language. Nothing here is needed for headless use.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Gives the text of message `key` with `{}` filled from the arguments, or `None` when it has no
/// translation for it.
pub type Translator = fn(&'static str, &[&str]) -> Option<String>;

static TRANSLATOR: OnceLock<Translator> = OnceLock::new();
static DECIMAL_COMMA: AtomicBool = AtomicBool::new(false);

/// Installs the translator. Only the first call has an effect.
pub fn set_translator(translator: Translator) {
    let _ = TRANSLATOR.set(translator);
}

/// Turns the decimal comma on (German): a comma inside a number is the decimal point, and lists
/// are separated by semicolons.
pub fn set_decimal_comma(on: bool) {
    DECIMAL_COMMA.store(on, Ordering::Relaxed);
}

/// True when the decimal comma is on.
pub fn decimal_comma() -> bool {
    DECIMAL_COMMA.load(Ordering::Relaxed)
}

/// The English templates of the messages this crate gives.
fn english(key: &str) -> &'static str {
    match key {
        "edit.not_found" => "region {} is not on the page",
        "edit.not_in_sync" => "the document changed: {}",
        "edit.cannot_read" => "Cannot read the expression: {}",
        "edit.plot_bad_limit" => "The limit {} is not valid: {}",
        "edit.plot_x_min" => "x start",
        "edit.plot_x_max" => "x end",
        "edit.plot_y_min" => "y start",
        "edit.plot_y_max" => "y end",
        "edit.too_many" => "The worksheet has too many regions.",
        "edit.not_editable" => "This element cannot be edited in this stage.",
        _ => "",
    }
}

/// Replaces the `{}` placeholders of `template` with `args`, in order.
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut args = args.iter();
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        match args.next() {
            Some(arg) => out.push_str(arg),
            None => out.push_str("{}"),
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// The message `key` with `args`, translated when a translator is installed.
pub fn message(key: &'static str, args: &[&str]) -> String {
    if let Some(text) = TRANSLATOR.get().and_then(|t| t(key, args)) {
        return text;
    }
    fill(english(key), args)
}
