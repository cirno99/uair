use std::{collections::HashSet, env, sync::OnceLock, thread, time};

use sysinfo::{Process, ProcessExt, System, SystemExt};

use crate::Error;

pub static mut PROC_KILLER_RUNNING: OnceLock<bool> = OnceLock::new();

pub fn run_proc_killer() -> Result<(), Error> {
    let secs = time::Duration::from_secs(10);
    let mut sys = System::new_all();

    let env_list = env::var("PROC_KILL_LIST").unwrap_or_default();
    let list = env_list.split(',').collect::<Vec<_>>();

    loop {
        thread::sleep(secs);
        match unsafe { PROC_KILLER_RUNNING.get_or_init(|| false) } {
            true => {
                // First we update all information of our `System` struct.
                sys.refresh_all();
                let name_set = sys
                    .processes()
                    .values()
                    .filter(|proc: &&Process| list.iter().any(|name| name.eq(&proc.name())))
                    .map(|proc| {
                        proc.kill();
                        proc.name()
                    })
                    .collect::<HashSet<&str>>();
                if name_set.len() != 0 {
                    let _ = std::process::Command::new("notify-send")
                        .arg(format!("Kill Processes: {:?}", name_set))
                        .output()
                        .expect("命令执行异常错误提示");
                }
            }
            false => {}
        };
    }
}
