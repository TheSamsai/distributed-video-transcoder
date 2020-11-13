
extern crate inotify;
use inotify::*;

use crate::*;

use std::path::PathBuf;
use std::thread;

pub fn init_watcher(path: PathBuf, pool: WorkPool) {
    thread::spawn(move || {
        let mut inotify = Inotify::init().expect("Inotify couldn't be initialized.");
        inotify.add_watch(
            path,
            WatchMask::CREATE | WatchMask::MODIFY,
        );

        let mut buffer = [0; 1024];

        'watcher: loop {
            for event in inotify.read_events(&mut buffer).expect("Failed to open event iterator") {
                if event.mask.contains(EventMask::CREATE) {
                    if let Some(name) = event.name {
                        let mut work_pool = pool.lock().unwrap();

                        work_pool.push(PathBuf::from(name));
                    }
                }
            }
        }
    });
}
