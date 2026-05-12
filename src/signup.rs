use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use reqwest::blocking::Client;
use serde::Serialize;
use std::io::IsTerminal;
use std::time::Duration;

use crate::install::{load_config, save_config, Config, SignupConfig};

const DEFAULT_INGEST_URL: &str = "https://satori-collect-emails.vercel.app/v1/ingest";
const SOURCE_NAME: &str = "ctx";
const ENV_INGEST_URL: &str = "CTX_SIGNUP_INGEST_URL";
const ENV_API_KEY: &str = "CTX_SIGNUP_API_KEY";
const ENV_DISABLE_PROMPT: &str = "CTX_DISABLE_SIGNUP_PROMPT";

#[derive(Debug, Serialize)]
struct SignupPayload<'a> {
    name: &'a str,
    source: &'a str,
    email: &'a str,
    gx_version: &'a str,
}

pub fn maybe_collect_signup() -> Result<()> {
    if !should_prompt() {
        return Ok(());
    }

    let api_key = match ingest_api_key() {
        Some(value) => value,
        None => return Ok(()),
    };

    let mut config = load_config()?;
    if signup_completed(&config) {
        return Ok(());
    }

    let theme = ColorfulTheme::default();
    let share = Confirm::with_theme(&theme)
        .with_prompt("Share your name and email for ctx updates?")
        .default(false)
        .interact()
        .context("prompt for ctx signup")?;

    if !share {
        mark_signup_skipped(&mut config);
        save_config(&config)?;
        return Ok(());
    }

    let name: String = Input::with_theme(&theme)
        .with_prompt("Name")
        .validate_with(|value: &String| validate_name(value))
        .interact_text()
        .context("prompt for signup name")?;

    let email: String = Input::with_theme(&theme)
        .with_prompt("Email")
        .validate_with(|value: &String| validate_email(value))
        .interact_text()
        .context("prompt for signup email")?;

    let name = normalize_name(&name)?;
    let email = normalize_email(&email)?;
    submit_signup(&name, &email, &api_key)?;
    mark_signup_submitted(&mut config, &name, &email);
    save_config(&config)?;
    Ok(())
}

fn should_prompt() -> bool {
    if std::env::var(ENV_DISABLE_PROMPT)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
    {
        return false;
    }

    if std::env::var("CI").is_ok() {
        return false;
    }

    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn signup_completed(config: &Config) -> bool {
    config
        .signup
        .as_ref()
        .map(|signup| signup.submitted_at.is_some() || signup.skipped_at.is_some())
        .unwrap_or(false)
}

fn mark_signup_skipped(config: &mut Config) {
    let now = Utc::now();
    let mut signup = config.signup.clone().unwrap_or_default();
    signup.skipped_at = Some(now);
    config.signup = Some(signup);
}

fn mark_signup_submitted(config: &mut Config, name: &str, email: &str) {
    let now = Utc::now();
    config.signup = Some(SignupConfig {
        name: Some(name.to_string()),
        email: Some(email.to_string()),
        submitted_at: Some(now),
        skipped_at: None,
    });
}

fn ingest_url() -> String {
    std::env::var(ENV_INGEST_URL).unwrap_or_else(|_| DEFAULT_INGEST_URL.to_string())
}

fn ingest_api_key() -> Option<String> {
    std::env::var(ENV_API_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("CTX_SIGNUP_API_KEY").map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
}

fn submit_signup(name: &str, email: &str, api_key: &str) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .context("build signup http client")?;
    submit_signup_with_client(
        &client,
        &ingest_url(),
        name,
        email,
        api_key,
        env!("CARGO_PKG_VERSION"),
    )
}

fn submit_signup_with_client(
    client: &Client,
    url: &str,
    name: &str,
    email: &str,
    api_key: &str,
    version: &str,
) -> Result<()> {
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .header("x-api-key", api_key)
        .json(&SignupPayload {
            name,
            source: SOURCE_NAME,
            email,
            gx_version: version,
        })
        .send()
        .context("send signup request")?;

    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(());
    }

    bail!("signup request failed with status {}", response.status())
}

fn validate_name(value: &str) -> Result<(), String> {
    normalize_name(value)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn validate_email(value: &str) -> Result<(), String> {
    normalize_email(value)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn normalize_name(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("name is required");
    }
    if trimmed.chars().count() > 120 {
        bail!("name must be 120 characters or fewer");
    }
    Ok(trimmed.to_string())
}

fn normalize_email(value: &str) -> Result<String> {
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() {
        bail!("email is required");
    }
    if trimmed.len() > 320 {
        bail!("email must be 320 characters or fewer");
    }
    let (local, domain) = trimmed
        .split_once('@')
        .ok_or_else(|| anyhow!("enter a valid email"))?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        bail!("enter a valid email");
    }
    if trimmed.chars().any(char::is_whitespace) {
        bail!("enter a valid email");
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        let normalized = normalize_email("  Ada@Example.COM ").expect("normalize");
        assert_eq!(normalized, "ada@example.com");
    }

    #[test]
    fn normalize_email_rejects_invalid_values() {
        assert!(normalize_email("not-an-email").is_err());
        assert!(normalize_email("ada@example").is_err());
    }

    #[test]
    fn normalize_name_requires_content() {
        assert!(normalize_name("   ").is_err());
        assert!(normalize_name("Ada Lovelace").is_ok());
    }

    #[test]
    fn submit_signup_posts_expected_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_http_request(&mut stream);
            assert!(request.contains("POST /v1/ingest HTTP/1.1"));
            assert!(request.contains("x-api-key: secret-key"));
            assert!(request.contains("\"name\":\"Ada Lovelace\""));
            assert!(request.contains("\"source\":\"ctx\""));
            assert!(request.contains("\"email\":\"ada@example.com\""));
            assert!(request.contains("\"gx_version\":\"0.1.6-test\""));

            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .expect("write response");
        });

        let client = Client::builder().build().expect("client");
        submit_signup_with_client(
            &client,
            &format!("http://{}/v1/ingest", addr),
            "Ada Lovelace",
            "ada@example.com",
            "secret-key",
            "0.1.6-test",
        )
        .expect("submit");

        handle.join().expect("join");
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).expect("read request");
        String::from_utf8_lossy(&buf[..n]).to_string()
    }
}
