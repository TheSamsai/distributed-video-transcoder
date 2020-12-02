extern crate reqwest;
extern crate tokio;

use reqwest::Result;
use reqwest::Url;
use std::env;

use std::time::Duration;
use tokio::time;

static PING_TIMEOUT_SEC: u64 = 30;
static RSYNC_USER: &str = "mina";
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

            let rsync_result = std::process::Command::new("rsync")
                .arg("-az")
                .arg(RSYNC_USER.to_string() + "@" + serv_address.host_str().unwrap() + ":" + &job)
                .arg(jobs_dir.to_str().unwrap())
                .status();
            match rsync_result {
                Ok(f) => f,
                Err(e) => panic!("Error calling rsync for job file {:?}", e),
            };

            let input_file = jobs_dir.to_str().unwrap().to_string()
                + job_pathbuf.file_name().unwrap().to_str().unwrap();
            let output_file = jobs_dir.to_str().unwrap().to_string()
                + job_pathbuf.file_stem().unwrap().to_str().unwrap()
                + ".webm";

            let conversion = std::process::Command::new("ffmpeg")
                .arg("-i")
                .arg(&input_file)
                .arg("-o")
                .arg(&output_file)
                .status();
            match conversion {
                Ok(c) => c,
                Err(e) => panic!("Error calling ffmpeg {:?}", e),
            };

            let rsync_to_serv = std::process::Command::new("rsync")
                .arg("-az")
                .arg(&output_file)
                .arg(
                    RSYNC_USER.to_string()
                        + "@"
                        + serv_address.host_str().unwrap()
                        + ":"
                        + FINISHED_SERV_DIR,
                )
                .status();
            match rsync_to_serv {
                Ok(f) => f,
                Err(e) => panic!("Error calling rsync back to server {:?}", e),
            };

            job_requests(serv_address.join("/push").unwrap().to_string(), &uuid)
                .await
                .unwrap();

            std::fs::remove_file(&output_file).expect("Couldn't remove output file");
            std::fs::remove_file(&input_file).expect("Couldn't remove input file");
            //let resultt = std::process::Command::new("sleep").arg("20").status();
        }
    }
}
