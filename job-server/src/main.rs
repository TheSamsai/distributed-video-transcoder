#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;
extern crate chrono;
extern crate rocket_contrib;
extern crate uuid;

#[macro_use] extern crate log;
extern crate simplelog;

use rocket::http::Status;
use rocket::request;
use rocket::request::FromRequest;
use rocket::request::Outcome;
use rocket::request::Request;
use std::net::SocketAddr; 
use rocket::State;
use rocket_contrib::json::Json;
use serde::Deserialize;

use uuid::Uuid;

use simplelog::*;

mod inotify_watcher;

use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use std::time::Duration;
use std::time::Instant;

use std::env;

/* NOTE: These are the data types used to hold the server state
 *
 * THEY MUST ALWAYS BE LOCKED IN THE FOLLOWING ORDER:
 * 1. LastCheckIn
 * 2. AssignedWork
 * 3. WorkPool
 *
 * Not doing so can cause the system to deadlock
 *
 * Locking any of them individually is safe
*/
type LastCheckIn = Arc<Mutex<HashMap<Uuid, Instant>>>;
type AssignedWork = Arc<Mutex<HashMap<Uuid, PathBuf>>>;
type WorkPool = Arc<Mutex<VecDeque<PathBuf>>>;

#[derive(Deserialize)]
#[derive(Debug)]
struct NodeFailure {
    uuid: NodeUuid,
    timestamp_utc: DateTime<Utc>,
    ffmepg_conversion:ProcessOutput,
    rsync_from:ProcessOutput,
    rsync_to:ProcessOutput,
}

#[derive(Deserialize)]
#[derive(Debug)]
struct ProcessOutput{
    exit_code:i32,
    stdout: String,
    stderr: String
}



#[derive(Debug, Deserialize)]
struct NodeUuid(String);

#[derive(Debug)]
enum NodeUuidError {
    Missing,
    Invalid,
}

impl<'a, 'r> FromRequest<'a, 'r> for NodeUuid {
    type Error = NodeUuidError;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, Self::Error> {
        let keys: Vec<_> = request.headers().get("uuid").collect();
        match keys.len() {
            0 => Outcome::Failure((Status::BadRequest, NodeUuidError::Missing)),
            1 if Uuid::parse_str(keys[0]).is_ok() => {
                Outcome::Success(NodeUuid(keys[0].to_string()))
            }
            _ => Outcome::Failure((Status::BadRequest, NodeUuidError::Invalid)),
        }
    }
}

#[post("/failure", format = "application/json", data = "<node_failure>")]
fn failure(
    node_failure: Json<NodeFailure>,
    remote_addr: SocketAddr,
    last_check_in: State<LastCheckIn>,
    assigned: State<AssignedWork>,
    pool: State<WorkPool>,
) -> String {
    let uuid = Uuid::parse_str(&node_failure.0.uuid.0).expect("UUID didn't parse correctly");
    let mut check_ins = last_check_in.lock().unwrap();
    let mut assigned_work = assigned.lock().unwrap();

    if let Some(_ci) = check_ins.get(&uuid) {
        check_ins.remove(&uuid);

        if let Some(path) = assigned_work.remove(&uuid) {
            let mut work_pool = pool.lock().unwrap();
            work_pool.push_back(path);
        }
       
        let ip = remote_addr.ip();
        warn!("Node failed: IP: {:?} UUID: {:?} TIME UTC: {:?}",ip,node_failure.0.uuid, node_failure.timestamp_utc);
        warn!("Failure info:\n FFMPEG: {:?}\n RSYNC FROM: {:?}\n RSYNC TO: {:?}"
            ,node_failure.0.ffmepg_conversion,node_failure.0.rsync_from, node_failure.0.rsync_to);
        return String::from("Ok");
    }
    return String::from("");
}

#[get("/info")]
fn info() -> Result<String, std::env::VarError> {
    let ffmpeg_command = std::env::var("FFMPEG_COMMAND")
        .unwrap_or(String::from("ffmpeg -i [input] -f webm [output].webm"));
    let file_extension = std::env::var("FILE_EXTENSION")?;
    let completed_files = std::env::var("COMPLETED_PATH")?;
    let rsync_user = std::env::var("RSYNC_USER")?;

    Ok(format!(
        "ffmpeg: {}
file_extension: {}
completed: {}
rsync_user: {}
",
        ffmpeg_command, file_extension, completed_files, rsync_user
    ))
}

#[get("/ping")]
fn ping(node_uuid: NodeUuid, check_ins: State<LastCheckIn>) -> String {
    let uuid = Uuid::parse_str(&node_uuid.0).expect("UUID didn't parse correctly");

    let mut ci = check_ins.lock().unwrap();

    ci.insert(uuid, Instant::now());

    return String::from("Ok");
}

#[get("/push")]
fn push(
    node_uuid: NodeUuid,
    assigned: State<AssignedWork>,
    pool: State<WorkPool>,
) -> Result<String, String> {
    let uuid = Uuid::parse_str(&node_uuid.0).expect("UUID didn't parse correctly");

    let file_extension = std::env::var("FILE_EXTENSION").expect("No FILE_EXTENSION given!");
    let completed_files = std::env::var("COMPLETED_PATH").expect("No COMPLETED_PATH given!");

    let mut assigned_work = assigned.lock().unwrap();

    if let Some(path) = assigned_work.remove(&uuid) {
        let completed_files_path = PathBuf::from(completed_files);
        let filename = path
            .file_name()
            .map(|fname| PathBuf::from(fname).with_extension(file_extension.replace(".", "")));

        if let Some(fname) = filename {
            if completed_files_path.join(fname.clone()).exists() {
                std::fs::remove_file(path);
                info!("File succesfully pushed {}", completed_files_path.join(fname).to_str().unwrap());
                return Ok(String::from("Thanks!"));
            } else {
                let mut work_pool = pool.lock().unwrap();

                work_pool.push_back(path);

                return Err(String::from("Failure, file not submitted"));
            }
        }
    }

    return Err(String::from("No work found."));
}

fn reallocate_job(
    last_check_in: &HashMap<Uuid, Instant>,
    assigned_work: &mut HashMap<Uuid, PathBuf>,
) -> Option<PathBuf> {
    let keys: Vec<Uuid> = assigned_work.keys().map(|k| k.clone()).collect();

    for key in keys.iter() {
        let instant = last_check_in.get(key).unwrap();

        if instant.elapsed() > Duration::new(60, 0) {
            warn!("Job reallocated {}", key.clone().to_string()); 
            return assigned_work.remove(key);
        }
    }

    return None;
}

#[get("/pull")]
fn pull(
    node_uuid: NodeUuid,
    check_ins: State<LastCheckIn>,
    assigned: State<AssignedWork>,
    pool: State<WorkPool>,
) -> String {
    let uuid = Uuid::parse_str(&node_uuid.0).expect("UUID didn't parse correctly");

    let mut ci = check_ins.lock().unwrap();

    if ci.contains_key(&uuid) {
        let mut assigned_work = assigned.lock().unwrap();
        let mut work_pool = pool.lock().unwrap();

        if let Some(path) = reallocate_job(&ci, &mut assigned_work) {
            assigned_work.insert(uuid, path.clone());
            info!("Job pulled by UUID: {}",&uuid.to_string());
            return String::from(path.to_str().unwrap());
        }

        if let Some(path) = work_pool.pop_front() {
            assigned_work.insert(uuid, path.clone());
            info!("Job pulled by UUID: {}",&uuid.to_string());
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

    info!("New node registerd: UUID: {}", uuid.to_string());

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

fn init_logging() {

    let term_level;
    let write_level;
    let file_name;

    if cfg!(debug_assertions) {

        term_level = simplelog::LevelFilter::Info;
        write_level = simplelog::LevelFilter::Debug;
        file_name = "DEBUG_job-server.log";
    }
    else {
        term_level = simplelog::LevelFilter::Warn;
        write_level = simplelog::LevelFilter::Info;
        file_name = "job-server.log";
    }

    CombinedLogger::init(
        vec![
            TermLogger::new(term_level, Config::default(), TerminalMode::Mixed).unwrap(),
            WriteLogger::new(write_level, Config::default(), 
                std::fs::File::create(std::env::current_dir().unwrap().join(file_name)).unwrap()),
        ]
    ).unwrap(); 

}

fn main() {
    init_logging();

    let work_pool: WorkPool = Arc::new(Mutex::new(VecDeque::new()));
    let assigned_work: AssignedWork = Arc::new(Mutex::new(HashMap::new()));
    let last_check_in: LastCheckIn = Arc::new(Mutex::new(HashMap::new()));

    inotify_watcher::init_watcher(
        env::current_dir().unwrap().join("incoming"),
        work_pool.clone(),
    );

    rocket::ignite()
        .manage(work_pool)
        .manage(assigned_work)
        .manage(last_check_in)
        .mount("/", routes![index, register, pull, push, ping, info, failure])
        .launch();
}
