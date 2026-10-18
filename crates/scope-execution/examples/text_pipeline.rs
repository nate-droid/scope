#[allow(dead_code)]
#[path = "../tests/support/mod.rs"]
mod support;

use scope_execution::execute;
use scope_types::{Capabilities, Endpoint};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Hello component world".into());
    let cell = cell::Cell::new()?;
    let result = execute(
        &cell,
        &support::pipeline(),
        &support::bindings(),
        support::inputs(&text),
        &Capabilities::new(),
    );
    if !result.errors.is_empty() {
        return Err(format!("execution failed: {:?}", result.errors).into());
    }
    let uppercase = &result.artifacts[&Endpoint::new("z-uppercase", "out")];
    let output = result.output.as_ref().ok_or("no selected output")?;
    let count = u64::from_le_bytes(output.bytes.as_slice().try_into()?);
    println!("Input: {text:?}");
    println!("Uppercase: {:?}", std::str::from_utf8(&uppercase.bytes)?);
    println!("Word count: {count}");
    println!("Completed: {:?}", result.completed);
    Ok(())
}
