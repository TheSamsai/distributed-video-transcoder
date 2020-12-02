extern crate reqwest;
extern crate tokio;

use reqwest::Result;
use reqwest::Url;
use std::env;

use std::time::Duration;
use tokio::time;

static PING_TIMEOUT_SEC: u64 = 30;
static FINISHED_SERV_DIR: &str = "ready";

async fn get_uuid(address: String) -> Result<String> {
    let body = reqwest::get(&address).await?.text().await?;

    if body.len() == 0 {
        panic!("UUID can not be empty");
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

        println!("PING");

        time::delay_for(Duration::from_secs(PING_TIMEOUT_SEC - 1)).await;
    }
}

struct Info {
    ffmpeg_command: String,
    file_extension: String,
    completed_files_dir: String,
    rsync_user: String,
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

#[tokio::main]
async fn main() {
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

    println!("{}", uuid);
    tokio::task::spawn(ping_timeout(
        serv_address.join("/ping").unwrap().into_string().clone(),
        uuid.clone(),
    ));

    loop {
        let job = job_requests(serv_address.join("/pull").unwrap().to_string(), &uuid)
            .await
            .unwrap();
        println!("{}", job);

        if job.len() == 0 {
            eprintln!("No available job paths");
            //Sleep for PING_TIMEOUT_SEC -1 secs before asking for new jobs
            // -1 to give the scheduler some leeway so that the requset arrives surely on time
            time::delay_for(Duration::from_secs(PING_TIMEOUT_SEC - 1)).await;
        } else {
            let job_pathbuf = std::path::Path::new(&job);

            let info = get_job_info(serv_address.join("/info").unwrap().to_string()).await;

            println!("rsync {} {}", info.rsync_user.clone() + "@" + serv_address.host_str().unwrap() + ":" + &job, jobs_dir.to_str().unwrap());

            let mut rsync_from_serv = std::process::Command::new("rsync")
                .arg("-az")
                .arg("-e")
                .arg("ssh")
                .arg(info.rsync_user.clone() + "@" + serv_address.host_str().unwrap() + ":" + &job)
                .arg(jobs_dir.to_str().unwrap())
                .spawn()
                .expect("Couldn't launch rsync");

            while let Ok(None) = rsync_from_serv.try_wait() {
                tokio::time::delay_for(Duration::from_millis(100)).await;
            }

            let input_file = String::from(jobs_dir.join(job_pathbuf.file_name().unwrap()).to_str().unwrap());
            let output_file = String::from(jobs_dir.join(job_pathbuf.file_stem().unwrap()).to_str().unwrap()) + &info.file_extension;

            println!("{}", output_file);

            let ffmpeg_command = info.ffmpeg_command.split(" ").map(|w| w.replace("[input]", &input_file)).map(|w| w.replace("[output]", &output_file));

            let mut conversion = std::process::Command::new("ffmpeg")
                .args(ffmpeg_command.skip(1))
                .spawn()
                .expect("Couldn't start ffmpeg");

            while let Ok(None) = conversion.try_wait() {
                tokio::time::delay_for(Duration::from_millis(100)).await;
            }

            let mut rsync_to_serv = std::process::Command::new("rsync")
                .arg("-az")
                .arg(output_file.clone)
                .arg(
                   info.rsync_user.clone()
                        + "@"
                        + serv_address.host_str().unwrap()
                        + ":"
                        + &info.completed_files_dir,
                )
                .spawn()
                .expect("Couldn't run rsync");

            while let Ok(None) = rsync_to_serv.try_wait() {
                tokio::time::delay_for(Duration::from_millis(100)).await;
            }

            job_requests(serv_address.join("/push").unwrap().to_string(), &uuid)
                .await
                .unwrap();

            std::fs::remove_file(&output_file).expect("Couldn't remove output file");
            std::fs::remove_file(&input_file).expect("Couldn't remove input file");
            // let resultt = std::process::Command::new("sleep").arg("20").status();
        }
    }
}
