#[macro_use] extern crate log;
extern crate simplelog;

extern crate reqwest;
extern crate tokio;
extern crate chrono;

use simplelog::*;
use reqwest::Result;
use reqwest::Url;
use std::env;

use std::time::Duration;
use tokio::time;

use serde::Serialize;
use chrono::{DateTime,Utc}; 


static PING_TIMEOUT_SEC: u64 = 30;


struct Info {
    ffmpeg_command: String,
    file_extension: String,
    completed_files_dir: String,
    rsync_user: String,
}

#[derive(Serialize)]
#[derive(Debug)]
struct NodeFailure {
    uuid: String,
    timestamp_utc: DateTime<Utc>,
    ffmepg_conversion:ProcessOutput,
    rsync_from:ProcessOutput,
    rsync_to:ProcessOutput,
}

#[derive(Serialize)]
#[derive(Debug)]
struct ProcessOutput{
    exit_code:i32,
    stdout: String,
    stderr: String
}

impl ProcessOutput {

    fn new(output: &std::process::Output) -> ProcessOutput {
        ProcessOutput {
            exit_code: output.status.code().unwrap().clone(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

}


async fn get_uuid(address: String) -> Result<String> {
    let body = reqwest::get(&address).await?.text().await?;

    if body.len() == 0 {
        error!("UUID can not be empty");
        std::process::exit(1);
    }
    return Ok(body);
}

async fn job_requests(action_adress: String, uuid: &String) -> Result<String> {
    let client = reqwest::Client::new();

    let path = client
        .get(&action_adress)
        .header("uuid", uuid)
        .send()
        .await?
        .text()
        .await;

    path
}

async fn ping_timeout(address: String, uuid: String) {
    let client = reqwest::Client::new();

    loop {
        client
            .get(&address)
            .header("uuid", &uuid)
            .send()
            .await
            .expect("Error while pinging server");

            debug!("PING");
        time::delay_for(Duration::from_secs(PING_TIMEOUT_SEC - 1)).await;
    }
}

async fn failure_request(address: String, failure: &NodeFailure) {
    let client = reqwest::Client::new();
    
    client
        .post(&address)
        .json(failure)
        .send()
        .await
        .expect("Error while sending failure message");
    

}

async fn get_job_info(address: String) -> Info {
    let client = reqwest::Client::new();

    let body = client
        .get(&address)
        .send()
        .await
        .expect("Error getting job info")
        .text()
        .await
        .expect("Error getting job info");

    let lines: Vec<&str> = body.split("\n").collect();

    Info {
        ffmpeg_command: lines[0].replace("ffmpeg: ", ""),
        file_extension: lines[1].replace("file_extension: ", ""),
        completed_files_dir: lines[2].replace("completed: ", ""),
        rsync_user: lines[3].replace("rsync_user: ", ""),
    }
}

async fn init_logging() {

    let term_level;
    let write_level;
    let file_name;

    if cfg!(debug_assertions) {

        term_level = simplelog::LevelFilter::Info;
        write_level = simplelog::LevelFilter::Debug;
        file_name = "DEBUG_node.log";
    }
    else {
        term_level = simplelog::LevelFilter::Warn;
        write_level = simplelog::LevelFilter::Info;
        file_name = "node.log";
    }

    CombinedLogger::init(
        vec![
            TermLogger::new(term_level, Config::default(), TerminalMode::Mixed).unwrap(),
            WriteLogger::new(write_level, Config::default(), 
                std::fs::File::create(std::env::current_dir().unwrap().join(file_name)).unwrap()),
        ]
    ).unwrap(); 

}

#[tokio::main]
async fn main() {

    // Setup logging
    init_logging().await;

    let args: Vec<String> = env::args().collect();
    let serv_address;

    if args.len() > 1 {
        serv_address = Url::parse(&args[1]).unwrap_or_else(|error| {
            panic!("Give proper url as a parameter: {:?}", error);
        });
    } else {
        panic!("Usage: {} <server's URL>", &args[0]);
    };
    //Create jobs-directory if it doesn't exist
    let jobs_dir = env::current_dir().unwrap().join("jobs");

    if let Err(_e) = std::fs::canonicalize(jobs_dir.clone()) {
        std::fs::create_dir(jobs_dir.clone()).unwrap()
    };

    let uuid = get_uuid(serv_address.join("/register").unwrap().into_string())
        .await
        .unwrap();

    info!("UUID: {}", uuid);
    tokio::task::spawn(ping_timeout(
        serv_address.join("/ping").unwrap().into_string().clone(),
        uuid.clone(),
    ));

    loop {
        let job = job_requests(serv_address.join("/pull").unwrap().to_string(), &uuid)
            .await
            .unwrap();


        if job.len() == 0 {
            debug!("No available job paths");
            //Sleep for PING_TIMEOUT_SEC -1 secs before asking for new jobs
            // -1 to give the scheduler some leeway so that the requset arrives surely on time
            time::delay_for(Duration::from_secs(PING_TIMEOUT_SEC - 1)).await;
        } else {

            info!("New job: {}", job);
            let job_pathbuf = std::path::Path::new(&job);

            let info = get_job_info(serv_address.join("/info").unwrap().to_string()).await;

            debug!("rsync -az -e ssh --protect-args {} {}", info.rsync_user.clone() + "@" + serv_address.host_str().unwrap() + ":\"" + job_pathbuf.to_str().unwrap() + "\"", jobs_dir.to_str().unwrap());

            let rsync_from_serv = tokio::process::Command::new("rsync")
                .arg("-az")
                .arg("-e")
                .arg("ssh")
                .arg("--protect-args")
                .arg(info.rsync_user.clone() + "@" + serv_address.host_str().unwrap() + ":" + job_pathbuf.to_str().unwrap())
                .arg(jobs_dir.to_str().unwrap())
                .output()
                .await
                .expect("Couldn't launch rsync"); 


            let input_file = String::from(jobs_dir.join(job_pathbuf.file_name().unwrap()).to_str().unwrap());
            let output_file = String::from(jobs_dir.join(job_pathbuf.file_stem().unwrap()).to_str().unwrap()) + &info.file_extension;

           

            let ffmpeg_command = info.ffmpeg_command.split(" ").map(|w| w.replace("[input]", &input_file))
                .map(|w| w.replace("[output]", &output_file));

            info!("Converion started: {}", ffmpeg_command.clone().collect::<String>());

            let conversion = tokio::process::Command::new("ffmpeg")
                .args(ffmpeg_command.skip(1))
                .output()
                .await
                .expect("Couldn't start ffmpeg");

            info!("Conversion ready. Output file: {}", output_file);

            let rsync_to_serv = tokio::process::Command::new("rsync")
                .arg("-az")
                .arg("--protect-args")
                .arg("-e")
                .arg("ssh")
                .arg(output_file.clone())
                .arg(
                   info.rsync_user.clone()
                        + "@"
                        + serv_address.host_str().unwrap()
                        + ":"
                        + &info.completed_files_dir
                )
                .output()
                .await
                .expect("Couldn't run rsync to server");


            if !std::path::Path::new(&output_file).exists() {


                let failure_info: NodeFailure = NodeFailure {
                    uuid:uuid.clone(),
                    timestamp_utc: Utc::now(),
                    ffmepg_conversion: ProcessOutput::new(&conversion), 
                    rsync_from: ProcessOutput::new(&rsync_from_serv),
                    rsync_to: ProcessOutput::new(&rsync_to_serv),

                };

                failure_request(serv_address.join("/failure").unwrap().to_string(), &failure_info).await;
                error!("Someting failed while converting\n Converion: {:?}\n Rsync form server: {:?} 
                        \n Rsync to server: {:?}"
                    ,&failure_info.ffmepg_conversion,&rsync_from_serv, &rsync_to_serv);
                std::process::exit(1);
            }



            job_requests(serv_address.join("/push").unwrap().to_string(), &uuid)
                .await
                .unwrap();

            info!("Job finished succesfully"); 

            std::fs::remove_file(&output_file).expect("Couldn't remove output file");
            std::fs::remove_file(&input_file).expect("Couldn't remove input file");
        }
    }
}
