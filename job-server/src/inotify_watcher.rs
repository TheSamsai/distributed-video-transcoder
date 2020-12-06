extern crate inotify;
use inotify::*;

use crate::*;

use std::thread;
use std::{fs::read_dir, path::PathBuf};

pub fn init_watcher(path: PathBuf, pool: WorkPool) {
    thread::spawn(move || {
        // Check if directory exist. If not, create it
        if let Err(_e) = std::fs::canonicalize(path.clone()) {
            std::fs::create_dir(path.clone()).unwrap()
        };

        // Read existing files and add the, to work_pool
        for entry in std::fs::read_dir(&path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let mut work_pool = pool.lock().unwrap();
                work_pool.push_back(path)
            }
        }
        let mut inotify = Inotify::init().expect("Inotify couldn't be initialized.");
        inotify.add_watch(&path, WatchMask::CREATE | WatchMask::MODIFY);

        let mut buffer = [0; 1024];

        'watcher: loop {
            for event in inotify
                .read_events(&mut buffer)
                .expect("Failed to open event iterator")
            {
                if event.mask.contains(EventMask::CREATE) {
                    if let Some(name) = event.name {
                        let mut work_pool = pool.lock().unwrap();

                        work_pool.push_back(path.join(name));
                    }
                }
            }
        }
    });
}
