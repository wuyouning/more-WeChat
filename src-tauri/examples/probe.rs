//! Read-only installation/instance/mutex probe. It never closes a remote handle.
use more_wechat_lib::platform;
fn main() {
    let adapter = platform::native();
    match adapter.detect(None) {
        Ok(Some(installation)) => {
            println!("{}", serde_json::to_string_pretty(&installation).unwrap());
            match adapter.instances(&installation) {
                Ok(instances) => println!("{}", serde_json::to_string_pretty(&instances).unwrap()),
                Err(error) => eprintln!("{error}"),
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&adapter.compatibility(&installation)).unwrap()
            );
            #[cfg(windows)]
            match platform::observe_windows_mutexes(&installation) {
                Ok(names) => {
                    for (_, name) in names {
                        println!("mutex: {name}");
                    }
                }
                Err(error) => eprintln!("{error}"),
            }
        }
        Ok(None) => println!("WeChat is not installed"),
        Err(error) => eprintln!("{error}"),
    }
}
