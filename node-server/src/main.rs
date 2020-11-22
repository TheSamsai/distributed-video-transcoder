extern crate reqwest;
extern crate tokio;

use reqwest::Result;
use reqwest::Url;
use std::env;

use std::time::Duration;

async fn get_uuid(address: String) -> Result<String> {
    let body = reqwest::get(&address).await?.text().await?;

    if body.len() == 0 {
        panic!("UUID can not be empty");
    }
    return Ok(body);
}

async fn pull_job(address: String, uuid: &String) -> Result<String> {
    let client = reqwest::Client::new();

    let path = client
        .get(&address)
        .header("uuid", uuid)
        .send()
        .await?
        .text()
        .await?;

    return Ok(path);
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let serv_address;

    if args.len() > 1 {
        serv_address = Url::parse(&args[1]).unwrap_or_else(|error| {
            panic!("Give proper url as a parameter: {:?}", error);
        });
    } else {
        panic!("Usage: {} <server's URL>", &args[0]);
    };

    let uuid = get_uuid(serv_address.join("/register").unwrap().into_string())
        .await
        .unwrap();

    println!("{}", uuid);

    loop {
        let job = pull_job(
            serv_address.join("/pull").unwrap().to_string(),
            &uuid,
        )
        .await
        .unwrap();
        println!("{}", job);

        if job.len() == 0 {
            eprint!("No available job paths");
            //Sleep for 30 secs before asking for new jobs
            tokio::time::delay_for(Duration::from_secs(30));
        } else {
            let conversion = std::process::Command::new("ffmpeg")
                .arg("-i")
                .arg(job)
                .arg("-o")
                .arg("filename.webm")
                .spawn();
        }
    }

    return Ok(());
}
