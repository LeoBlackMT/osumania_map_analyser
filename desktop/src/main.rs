// mma-shell 无窗口版入口（开发冒烟；窗口 wrapper 后续接管）。默认不 push。

use mma_shell::{config, server};
use std::time::Duration;

fn main() {
    let plugin_dir = config::plugin_dir();
    let tosu = config::probe_tosu_env();
    match &tosu {
        Some(info) => println!(
            "tosu: {}:{} (online={})",
            info.ip,
            info.port,
            config::tosu_online(info)
        ),
        None => println!("tosu: not found — offline mode (24061 serves plugin page)"),
    }
    println!("plugin dir: {}", plugin_dir.display());
    let _shared = server::start(plugin_dir, tosu);
    println!("bridge listening: 24060 (malody POST) / 24061 (static + /ws + /settings + /cover)");
    println!("press Ctrl+C to exit");
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}