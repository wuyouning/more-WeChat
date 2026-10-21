//! Explicit normal-launch compatibility check: no mutex operations, no login automation.
#[cfg(windows)]
use more_wechat_lib::platform;
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() != Some("--launch") {
        return Err("Pass --launch to submit one normal WeChat launch".into());
    }
    let adapter = platform::native();
    let installation = adapter.detect(None)?.ok_or("WeChat is not installed")?;
    let before = adapter.instances(&installation)?;
    println!("before: {} main process(es)", before.len());
    // This probe deliberately tests normal OS launch, passing no remote processes
    // to the adapter. It never releases a lock or accepts authentication prompts.
    adapter.launch_one(&installation, &[], &|message| println!("{message}"))?;
    std::thread::sleep(std::time::Duration::from_secs(5));
    let after = adapter.instances(&installation)?;
    println!("after: {} main process(es)", after.len());
    println!(
        "original processes retained: {}",
        before.iter().all(|old| after
            .iter()
            .any(|new| old.pid == new.pid && old.started_at == new.started_at))
    );
    println!("A process is not proof of two independent logged-in accounts.");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "This normal-launch probe is Windows-only. Use launch_check for macOS clone launches."
    );
}
