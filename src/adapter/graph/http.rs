use reqwest::blocking::Response;
use std::io;

pub(crate) fn response_to_io(result: Result<Response, reqwest::Error>) -> io::Result<Response> {
    let response = result.map_err(io::Error::other)?;
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let url = response.url().to_string();
    let body = response.text().unwrap_or_default();
    let detail = if body.trim().is_empty() {
        format!("{status} requesting: {url}")
    } else {
        format!(
            "{status} requesting: {url}
Response: {body}"
        )
    };
    Err(io::Error::other(detail))
}
