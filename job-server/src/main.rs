#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;

use rocket::State;

mod inotify_watcher;

use std::sync::Arc;
use std::sync::Mutex;
use std::path::PathBuf;

type WorkPool = Arc<Mutex<Vec<PathBuf>>>;

#[get("/")]
fn index(work_pool: State<WorkPool>) -> String {
    let mut files = String::new();

    let mut pool = work_pool.lock().unwrap();

    for file in pool.iter() {
        files.push_str(file.to_str().unwrap());
        files.push_str("\n");
    }

    return files;
}

fn main() {
    let work_pool: WorkPool = Arc::new(Mutex::new(Vec::new()));

    inotify_watcher::init_watcher(PathBuf::from("incoming"), work_pool.clone());

    rocket::ignite()
        .manage(work_pool)
        .mount("/", routes![index]).launch();
}
