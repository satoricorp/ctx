use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use reqwest::blocking::Client;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use crate::cli::theme;
use crate::install::{load_config, save_config, Config, SignupConfig};

const DEFAULT_INGEST_URL: &str = "https://satori-collect-emails.vercel.app/v1/ingest";
const SOURCE_NAME: &str = "ctx";
const ENV_INGEST_URL: &str = "CTX_SIGNUP_INGEST_URL";
const ENV_API_KEY: &str = "CTX_SIGNUP_API_KEY";
const ENV_DISABLE_PROMPT: &str = "CTX_DISABLE_SIGNUP_PROMPT";
const BUNDLED_INGEST_API_KEY: &str = "satori-eng-co-random-token-808";

#[derive(Debug, Serialize)]
struct SignupPayload<'a> {
    name: &'a str,
    source: &'a str,
    email: &'a str,
    gx_version: &'a str,
}

struct SignupInput {
    name: String,
    email: String,
}

pub fn maybe_collect_signup() -> Result<()> {
    if !should_prompt() {
        return Ok(());
    }

    let mut config = load_config()?;
    if signup_completed(&config) || signup_skipped(&config) {
        return Ok(());
    }
    if let Some((name, email)) = pending_signup(&config)? {
        maybe_submit_pending_signup(&mut config, &name, &email, false)?;
        save_config(&config)?;
        return Ok(());
    }

    let Some(input) = collect_signup_input()? else {
        mark_signup_skipped(&mut config);
        save_config(&config)?;
        return Ok(());
    };

    let name = normalize_name(&input.name)?;
    let email = normalize_email(&input.email)?;
    mark_signup_pending(&mut config, &name, &email);
    maybe_submit_pending_signup(&mut config, &name, &email, true)?;
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
        .map(|signup| signup.submitted_at.is_some())
        .unwrap_or(false)
}

fn signup_skipped(config: &Config) -> bool {
    config
        .signup
        .as_ref()
        .map(|signup| signup.skipped_at.is_some())
        .unwrap_or(false)
}

fn pending_signup(config: &Config) -> Result<Option<(String, String)>> {
    let Some(signup) = config.signup.as_ref() else {
        return Ok(None);
    };
    let (Some(name), Some(email)) = (signup.name.as_deref(), signup.email.as_deref()) else {
        return Ok(None);
    };

    Ok(Some((normalize_name(name)?, normalize_email(email)?)))
}

fn mark_signup_skipped(config: &mut Config) {
    let now = Utc::now();
    let mut signup = config.signup.clone().unwrap_or_default();
    signup.skipped_at = Some(now);
    config.signup = Some(signup);
}

fn mark_signup_pending(config: &mut Config, name: &str, email: &str) {
    config.signup = Some(SignupConfig {
        name: Some(name.to_string()),
        email: Some(email.to_string()),
        submitted_at: None,
        skipped_at: None,
    });
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

fn ingest_api_key() -> String {
    std::env::var(ENV_API_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| BUNDLED_INGEST_API_KEY.to_string())
}

fn maybe_submit_pending_signup(
    config: &mut Config,
    name: &str,
    email: &str,
    verbose: bool,
) -> Result<()> {
    let api_key = ingest_api_key();

    if let Err(error) = submit_signup(name, email, &api_key) {
        if verbose {
            eprintln!(
                "{}",
                theme::warn(format!("signup submission failed: {error}"))
            );
        }
        return Ok(());
    }

    mark_signup_submitted(config, name, email);
    Ok(())
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

fn collect_signup_input() -> Result<Option<SignupInput>> {
    println!("{}", theme::section("Join the ctx early list"));
    println!(
        "{}",
        theme::muted(
            "Name + email helps us follow up with product updates. Type skip in Name to opt out."
        )
    );
    println!();

    let name = prompt_signup_line("Name").context("prompt for signup name")?;
    if is_skip(&name) {
        return Ok(None);
    }

    let email = prompt_signup_line("Email").context("prompt for signup email")?;
    println!();

    Ok(Some(SignupInput { name, email }))
}

fn prompt_signup_line(label: &str) -> Result<String> {
    print!("{}: ", theme::command(label));
    io::stdout().flush().context("flush signup prompt")?;
    read_input_line()
}

fn read_input_line() -> Result<String> {
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("read signup input")?;
    Ok(value.trim_end_matches(['\r', '\n']).to_string())
}

fn is_skip(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("skip")
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
