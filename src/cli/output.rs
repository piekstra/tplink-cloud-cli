use tabled::settings::Style;
use tabled::{Table, Tabled};

use crate::config::OutputMode;

pub fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

pub fn print_table<T: Tabled>(data: &[T]) {
    if data.is_empty() {
        println!("No results.");
        return;
    }
    let table = Table::new(data).with(Style::rounded()).to_string();
    println!("{}", table);
}

pub fn print_output(value: &serde_json::Value, mode: &OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Table => {
            // For table mode, if the value is an array of objects, display as table.
            // Otherwise fall back to JSON.
            print_json(value);
        }
    }
}

pub fn print_error(err: &crate::error::AppError) {
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&err.to_json()).unwrap_or_default()
    );
}
