#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;

extern crate uuid;

use rocket::State;
use rocket::request;
use rocket::request::FromRequest;
use rocket::request::Request;
use rocket::request::Outcome;
use rocket::http::Status;

use uuid::Uuid;

mod inotify_watcher;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::path::PathBuf;

use std::time::Instant;
use std::time::Duration;

use std::env;

type WorkPool = Arc<Mutex<Vec<PathBuf>>>;
type AssignedWork = Arc<Mutex<HashMap<Uuid, PathBuf>>>;
type LastCheckIn = Arc<Mutex<HashMap<Uuid, Instant>>>;

struct NodeUuid(String);

#[derive(Debug)]
enum NodeUuidError {
    Missing,
    Invalid
}

impl<'a, 'r> FromRequest<'a, 'r> for NodeUuid {
    type Error = NodeUuidError;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, Self::Error> {
        let keys: Vec<_> = request.headers().get("uuid").collect();
        match keys.len() {
            0 => Outcome::Failure((Status::BadRequest, NodeUuidError::Missing)),
            1 if Uuid::parse_str(keys[0]).is_ok() => Outcome::Success(NodeUuid(keys[0].to_string())),
            _ => Outcome::Failure((Status::BadRequest, NodeUuidError::Invalid)),
        }
    }
}

#[get("/ping")]
fn ping(node_uuid: NodeUuid, check_ins: State<LastCheckIn>) -> String {
    let uuid = Uuid::parse_str(&node_uuid.0).expect("UUID didn't parse correctly");

    let mut ci = check_ins.lock().unwrap();

    ci.insert(uuid, Instant::now());

    return String::from("Ok");
}


#[get("/push")]
fn push(node_uuid: NodeUuid, assigned: State<AssignedWork>) -> String {
    let uuid = Uuid::parse_str(&node_uuid.0).expect("UUID didn't parse correctly");

    let mut assigned_work = assigned.lock().unwrap();

    assigned_work.remove(&uuid);

    return String::from("Thanks");
}

fn reallocate_job(last_check_in: &HashMap<Uuid, Instant>, assigned_work: &mut HashMap<Uuid, PathBuf>) -> Option<PathBuf> {
    let keys: Vec<Uuid> = assigned_work.keys().map(|k| k.clone()).collect();

    for key in keys.iter() {
        let instant = last_check_in.get(key).unwrap();

        if instant.elapsed() > Duration::new(60, 0) {
            return assigned_work.remove(key);
        }
    }

    return None;
}

#[get("/pull")]
fn pull(node_uuid: NodeUuid, check_ins: State<LastCheckIn>, assigned: State<AssignedWork>, pool: State<WorkPool>) -> String {
    let uuid = Uuid::parse_str(&node_uuid.0).expect("UUID didn't parse correctly");

    let mut ci = check_ins.lock().unwrap();

    if ci.contains_key(&uuid) {
        let mut assigned_work = assigned.lock().unwrap();
        let mut work_pool = pool.lock().unwrap();

        if let Some(path) = reallocate_job(&ci, &mut assigned_work) {
            assigned_work.insert(uuid, path.clone());
            return String::from(path.to_str().unwrap());
        }

        if let Some(path) = work_pool.pop() {
            assigned_work.insert(uuid, path.clone());
            return String::from(path.to_str().unwrap());
        }
    }

    return String::from("");
}

#[get("/register")]
fn register(check_ins: State<LastCheckIn>) -> String {
    let mut ci = check_ins.lock().unwrap();

    let uuid = Uuid::new_v4();

    ci.insert(uuid, Instant::now());

    return format!("{}", uuid.to_urn());
}

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
    let assigned_work: AssignedWork = Arc::new(Mutex::new(HashMap::new()));
    let last_check_in: LastCheckIn = Arc::new(Mutex::new(HashMap::new()));

    inotify_watcher::init_watcher(env::current_dir().unwrap().join("incoming"), work_pool.clone());

    rocket::ignite()
        .manage(work_pool)
        .manage(assigned_work)
        .manage(last_check_in)
        .mount("/", routes![index, register, pull, push, ping]).launch();
}