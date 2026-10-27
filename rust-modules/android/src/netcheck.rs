//! The network harness (`--es net check [--es server https://<ip>.<id>.plex.direct:32400]`): three
//! fetches through the real `net` layer and the bundled libcurl, each logged and returned as a label
//! line. plex.tv must verify against the shipped Mozilla bundle, the server against the bundled Let's
//! Encrypt roots after that bundle refuses it, and an expired certificate must be refused (curl 60).

use plx_net::net::{self, RequestFailure};

use crate::skeleton::{log, LOG_INFO};

const PLEX_TV: &str = "https://plex.tv/api/v2/ping";
const BAD_CERT: &str = "https://expired.badssl.com/";
const CURLE_PEER_FAILED_VERIFICATION: i32 = 60;

fn get(url: &str) -> Result<net::Resp, RequestFailure> {
    net::request_evidence(url, &[], "GET", None, net::API, false, Some(64 * 1024), None)
}

fn why(failure: &RequestFailure) -> String {
    format!("{:?}, curl {:?}, verify {:?}", failure.cause, failure.curl_rc, failure.verify)
}

fn verdict(what: &str, ok: bool, detail: String) -> String {
    let line = format!("net {what}: {} ({detail})", if ok { "PASS" } else { "FAIL" });
    log(LOG_INFO, &line);
    line
}

pub fn harness(server: Option<&str>) -> Vec<String> {
    let mut lines = vec!["net check".to_string()];
    if !net::global_init() {
        lines.push(verdict("libcurl bound", false, "see the net: line above".into()));
        return lines;
    }
    lines.push(match get(PLEX_TV) {
        Ok(resp) => verdict("plex.tv verified", true, format!("HTTP {}", resp.status)),
        Err(failure) => verdict("plex.tv verified", false, why(&failure)),
    });
    lines.push(match server {
        Some(server) => match get(&format!("{}/identity", server.trim_end_matches('/'))) {
            Ok(resp) => verdict("server verified", resp.ok(), format!("HTTP {}", resp.status)),
            Err(failure) => verdict("server verified", false, why(&failure)),
        },
        None => verdict("server verified", false, "no --es server".into()),
    });
    lines.push(match get(BAD_CERT) {
        Ok(resp) => verdict("bad certificate refused", false, format!("HTTP {}", resp.status)),
        Err(failure) => {
            verdict("bad certificate refused", failure.curl_rc == Some(CURLE_PEER_FAILED_VERIFICATION), why(&failure))
        }
    });
    lines
}
