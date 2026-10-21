//! Explicit real-process smoke check. Usage: cargo run --example launch_check -- 1
use more_wechat_lib::{model, platform};
#[path = "../src/engine.rs"]
mod engine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target: u8 = std::env::args()
        .nth(1)
        .ok_or("Pass an explicit target count (1..5)")?
        .parse()?;
    engine::validate_target(target)?;
    let adapter = platform::native();
    let installation = adapter.detect(None)?.ok_or("WeChat is not installed")?;
    let result = engine::launch_to_target(
        adapter.as_ref(),
        &installation,
        target,
        &|p| println!("{}", p.message),
        &std::thread::sleep,
    )?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if result.status == "failed" || result.status == "partial" {
        std::process::exit(1);
    }
    Ok(())
}
