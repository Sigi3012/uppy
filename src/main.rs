use clipboard_win::{formats, set_clipboard};
use lazy_static::lazy_static;
use md5::{Digest, Md5};
use reqwest::blocking::{self, multipart};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

lazy_static! {
    static ref CONFIG_PATH: PathBuf = {
        let user_profile = std::env::var("USERPROFILE")
            .expect("Somehow your userprofile isnt set, congratulations on getting this error");
        let config_path_str = format!("{}\\.config\\uppy", user_profile);
        PathBuf::from(config_path_str)
    };
}

#[derive(Serialize, Deserialize)]
struct Configuration {
    host: String,
    token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JSONResponse {
    files: Vec<String>,
}

enum UploadResult {
    Success(blocking::Response),
    IOError(std::io::Error),
    ReqwestError(reqwest::Error),
    HTTPClientError(reqwest::StatusCode),
    HTTPServerError(reqwest::StatusCode),
}

enum DeletionChoice {
    Yes,
    No,
    InvalidChoice,
}

fn read_config() -> serde_json::Result<Configuration> {
    let json = fs::read_to_string(CONFIG_PATH.join("config.json")).expect("Failed to read file");
    let c: Configuration =
        serde_json::from_str(&json).expect("JSON file is not formatted properly");
    Ok(c)
}

fn construct_headers(config: &Configuration) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&config.token).expect("Failed to convert token to header"),
    );
    headers.insert("Format", HeaderValue::from_static("RANDOM"));
    headers.insert("Embed", HeaderValue::from_static("true"));
    headers
}

fn upload_file(path: &PathBuf, config: Configuration, headers: HeaderMap) -> UploadResult {
    let form = match multipart::Form::new().file("file", path) {
        Ok(form) => form,
        Err(err) => return UploadResult::IOError(err),
    };

    let client = reqwest::blocking::Client::new();
    let res = match client
        .post(format!("{}/api/upload", config.host))
        .multipart(form)
        .headers(headers)
        .send()
    {
        Ok(res) => {
            if res.status().is_client_error() {
                return UploadResult::HTTPClientError(res.status());
            } else if res.status().is_server_error() {
                return UploadResult::HTTPServerError(res.status());
            }
            res
        }
        Err(err) => return UploadResult::ReqwestError(err),
    };

    UploadResult::Success(res)
}

fn file_cleanup(file: PathBuf) {
    println!("Would you like to delete the file? (Y/N)");

    let mut buf: String = String::new();
    io::stdin().read_line(&mut buf).unwrap();

    let choice = match buf.trim().to_lowercase().as_str() {
        "yes" | "y" => DeletionChoice::Yes,
        "no" | "n" => DeletionChoice::No,
        _ => DeletionChoice::InvalidChoice,
    };

    match choice {
        DeletionChoice::Yes => {
            let mut f = fs::File::open(&file).unwrap();
            let mut buf = [0; 1024];
            let mut hash = Md5::new();

            loop {
                let bytes_read = match f.read(&mut buf) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        eprintln!("Error reading file: {}", err);
                        break;
                    }
                };
                if bytes_read == 0 {
                    break;
                }
                hash.update(&buf[..bytes_read]);
            }

            let result = hash.finalize();

            let temp_path = PathBuf::from(env::temp_dir().join(format!("{:x}.tmp", result)));
            match fs::rename(&file, temp_path) {
                Ok(_) => println!("File deleted!"),
                Err(err) => println!(
                    "Something went wrong while moving the file to the temp dir: {}",
                    err
                ),
            };
        }

        DeletionChoice::No => (),
        DeletionChoice::InvalidChoice => eprintln!("Invalid choice"),
    }
}

fn main() {
    let config_path_str = format!("{}\\.config\\uppy", std::env::var("USERPROFILE").unwrap());
    let config_path: &Path = Path::new(&config_path_str);

    let config: Configuration = match fs::create_dir(&config_path) {
        Ok(_) => {
            let template = json!({
                "host": "https://",
                "token": "",
            });
            let json = serde_json::to_string_pretty(&template).expect("Failed to serialise data");

            fs::write(CONFIG_PATH.join("config.json"), json)
                .expect("Failed to write to configuration file please fill it out manually");

            println!("Configuration directory created in .config");
            return;
        }

        Err(_) => match read_config() {
            // Configuration already exists, continue..
            Ok(c) => c,
            Err(err) => {
                eprintln!("Error reading configuration file: {}", err);
                return;
            }
        },
    };

    // I wish to turn this into a 1 liner
    let file_str: Vec<String> = env::args().collect();
    let file: &Path = Path::new(&file_str[1]);

    let executed_path = match env::current_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("Failed to get executed directory!\n{}", err);
            return;
        }
    };

    let target_file = executed_path.join(file);

    let headers = construct_headers(&config);
    match upload_file(&target_file, config, headers) {
        UploadResult::Success(res) => {
            let json = res.text().unwrap();
            let mut urls: JSONResponse =
                serde_json::from_str(&json).expect("Failed to deserialise JSON response");

            if urls.files.len() == 1 {
                let url: String = urls.files.pop().unwrap().to_string();
                println!("Uploaded URL: {}", url);
                match set_clipboard(formats::Unicode, url) {
                    Ok(_) => println!("Copied URL to clipboard!"),
                    Err(err) => {
                        eprintln!(
                            "Something went wrong while copying URL to clipboard: {}",
                            err
                        );
                        return;
                    }
                }
            }
        }
        UploadResult::IOError(err) => {
            println!(
                "Something went wrong while loading the targeted file: {}",
                err
            );
            return;
        }
        UploadResult::ReqwestError(err) => {
            println!(
                "Something went wrong while sending the HTTP request: {}",
                err
            );
            return;
        }
        UploadResult::HTTPClientError(code) => {
            println!("A HTTP client error occurred, code: {}", code);
            return;
        }
        UploadResult::HTTPServerError(code) => {
            println!("A HTTP server error occured, code: {}", code);
            return;
        }
    }
    file_cleanup(target_file)
}
