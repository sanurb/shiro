//! Bounded URL acquisition with redirect evidence and SSRF-resistant DNS.

use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use shiro_core::ShiroError;
use shiro_index::FtsIndex;
use shiro_store::{Store, UrlAcquisitionRecord};
use url::Url;

use super::document_ingestion::{publish_staged_documents, stage_url_document_bytes};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionParser {
    Auto,
    Plaintext,
    Markdown,
    Pdf,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AcquireUrlInput {
    pub url: String,
    pub parser: AcquisitionParser,
    pub max_bytes: usize,
    pub timeout_ms: u64,
    pub max_redirects: usize,
    pub allow_http: bool,
}

impl Default for AcquireUrlInput {
    fn default() -> Self {
        Self {
            url: String::new(),
            parser: AcquisitionParser::Auto,
            max_bytes: 50 * 1024 * 1024,
            timeout_ms: 30_000,
            max_redirects: 5,
            allow_http: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AcquireUrlOutput {
    pub doc_id: String,
    pub status: String,
    pub changed: bool,
    pub requested_url: String,
    pub final_url: String,
    pub redirects: Vec<String>,
    pub content_type: Option<String>,
    pub signature: String,
    pub bytes: usize,
    pub content_hash: String,
    pub segments: usize,
}

pub fn execute(
    store: &Store,
    fts: &FtsIndex,
    input: &AcquireUrlInput,
) -> Result<AcquireUrlOutput, ShiroError> {
    let mut publish = |staged: &super::document_ingestion::StagedDocumentIngestion| {
        publish_staged_documents(store, fts, &[staged])
    };
    execute_with_publisher(store, input, &mut publish)
}

pub(crate) fn execute_with_publisher(
    store: &Store,
    input: &AcquireUrlInput,
    publish: &mut dyn FnMut(
        &super::document_ingestion::StagedDocumentIngestion,
    ) -> Result<(), ShiroError>,
) -> Result<AcquireUrlOutput, ShiroError> {
    validate_limits(input)?;
    let requested_url = normalize_and_validate_url(&input.url, input.allow_http)?;
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_millis(input.timeout_ms))
        .resolver(|netloc: &str| resolve_public_addresses(netloc))
        .build();
    let mut current_url = requested_url.clone();
    let mut redirects = Vec::new();

    let response = loop {
        let result = agent
            .get(current_url.as_str())
            .set("Accept", "application/pdf,text/markdown,text/plain;q=0.9")
            .call();
        match result {
            Ok(response) => break response,
            Err(ureq::Error::Status(status, response)) if (300..400).contains(&status) => {
                if redirects.len() >= input.max_redirects {
                    return Err(ShiroError::InvalidInput {
                        message: "URL acquisition exceeded redirect limit".to_string(),
                    });
                }
                let location =
                    response
                        .header("Location")
                        .ok_or_else(|| ShiroError::InvalidInput {
                            message: format!("redirect {status} omitted Location"),
                        })?;
                let next =
                    current_url
                        .join(location)
                        .map_err(|error| ShiroError::InvalidInput {
                            message: format!("invalid redirect target: {error}"),
                        })?;
                current_url = normalize_and_validate_url(next.as_str(), input.allow_http)?;
                redirects.push(current_url.as_str().to_string());
            }
            Err(ureq::Error::Status(status, _)) => {
                return Err(ShiroError::InvalidInput {
                    message: format!("URL acquisition returned HTTP status {status}"),
                });
            }
            Err(ureq::Error::Transport(error)) => {
                return Err(ShiroError::Io(std::io::Error::other(format!(
                    "URL acquisition failed: {error}"
                ))));
            }
        }
    };

    if let Some(length) = response
        .header("Content-Length")
        .and_then(|value| value.parse::<usize>().ok())
    {
        if length > input.max_bytes {
            return Err(ShiroError::InvalidInput {
                message: format!(
                    "URL content length {length} exceeds max_bytes {}",
                    input.max_bytes
                ),
            });
        }
    }
    let content_type = response.header("Content-Type").map(|value| {
        value
            .split(';')
            .next()
            .unwrap_or(value)
            .trim()
            .to_ascii_lowercase()
    });
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((input.max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > input.max_bytes {
        return Err(ShiroError::InvalidInput {
            message: format!("URL response exceeds max_bytes {}", input.max_bytes),
        });
    }
    let signature = detect_signature(&bytes, content_type.as_deref(), &current_url)?;
    let content_hash = blake3::hash(&bytes).to_hex().to_string();
    let parser: Box<dyn shiro_core::ports::Parser> = match input.parser {
        AcquisitionParser::Auto => parser_for_signature(&signature),
        AcquisitionParser::Plaintext => Box::new(shiro_parse::PlainTextParser),
        AcquisitionParser::Markdown => Box::new(shiro_parse::MarkdownParser),
        AcquisitionParser::Pdf => Box::new(shiro_parse::PdfParser),
    };
    if matches!(input.parser, AcquisitionParser::Pdf) && signature != "pdf" {
        return Err(ShiroError::InvalidInput {
            message: "requested PDF parser but response lacks a PDF signature".to_string(),
        });
    }

    let redirects_json =
        serde_json::to_string(&redirects).map_err(|error| ShiroError::StoreCorrupt {
            message: format!("failed to serialize redirect evidence: {error}"),
        })?;
    let acquisition = UrlAcquisitionRecord {
        requested_url: requested_url.as_str().to_string(),
        final_url: current_url.as_str().to_string(),
        redirects_json,
        content_type: content_type.clone(),
        signature: signature.clone(),
        byte_count: bytes.len(),
        content_hash: content_hash.clone(),
    };
    let staged = stage_url_document_bytes(
        store,
        parser.as_ref(),
        current_url.as_str(),
        &bytes,
        &acquisition,
    )?;
    if staged.changed {
        publish(&staged)?;
    }

    Ok(AcquireUrlOutput {
        doc_id: staged.doc_id.as_str().to_string(),
        status: "READY".to_string(),
        changed: staged.changed,
        requested_url: requested_url.as_str().to_string(),
        final_url: current_url.as_str().to_string(),
        redirects,
        content_type,
        signature,
        bytes: bytes.len(),
        content_hash,
        segments: staged.segments.len(),
    })
}

fn validate_limits(input: &AcquireUrlInput) -> Result<(), ShiroError> {
    if input.max_bytes == 0 || input.timeout_ms == 0 {
        return Err(ShiroError::InvalidInput {
            message: "URL max_bytes and timeout_ms must be positive".to_string(),
        });
    }
    Ok(())
}

fn normalize_and_validate_url(value: &str, allow_http: bool) -> Result<Url, ShiroError> {
    let mut url = Url::parse(value).map_err(|error| ShiroError::InvalidInput {
        message: format!("invalid acquisition URL: {error}"),
    })?;
    match url.scheme() {
        "https" => {}
        "http" if allow_http => {}
        "http" => {
            return Err(ShiroError::InvalidInput {
                message: "HTTP acquisition requires explicit allow_http".to_string(),
            });
        }
        scheme => {
            return Err(ShiroError::InvalidInput {
                message: format!("unsupported acquisition URL scheme: {scheme}"),
            });
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ShiroError::InvalidInput {
            message: "acquisition URLs must not contain credentials".to_string(),
        });
    }
    if url.host_str().is_none() {
        return Err(ShiroError::InvalidInput {
            message: "acquisition URL requires a host".to_string(),
        });
    }
    url.set_fragment(None);
    Ok(url)
}

fn resolve_public_addresses(netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
    let addresses = netloc.to_socket_addrs()?.collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(std::io::Error::other("DNS returned no addresses"));
    }
    if addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "DNS resolved to a private, local, or non-routable address",
        ));
    }
    Ok(addresses)
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_multicast()
                && !ip.is_unspecified()
                && octets[0] != 0
                && octets[0] != 127
                && !(octets[0] == 100 && (64..=127).contains(&octets[1]))
                && !(octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                && !(octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                && !(octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
                && !(octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                && !(octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
                && octets[0] < 224
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && (segments[0] & 0xfe00) != 0xfc00
                && (segments[0] & 0xffc0) != 0xfe80
                && (segments[0] & 0xffc0) != 0xfec0
                && !(segments[0] == 0x0064 && segments[1] == 0xff9b)
                && !(segments[0] == 0x2001 && segments[1] == 0x0000)
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && segments[0] != 0x2002
        }
    }
}

fn detect_signature(
    bytes: &[u8],
    content_type: Option<&str>,
    final_url: &Url,
) -> Result<String, ShiroError> {
    if bytes.starts_with(b"%PDF-") {
        return Ok("pdf".to_string());
    }
    std::str::from_utf8(bytes).map_err(|_| ShiroError::InvalidInput {
        message: "URL response is neither a PDF nor valid UTF-8 text".to_string(),
    })?;
    if content_type == Some("text/markdown")
        || final_url
            .path()
            .rsplit('.')
            .next()
            .is_some_and(|extension| matches!(extension, "md" | "markdown"))
    {
        Ok("markdown_utf8".to_string())
    } else if content_type.is_none()
        || content_type.is_some_and(|value| value == "text/plain" || value.starts_with("text/"))
    {
        Ok("plaintext_utf8".to_string())
    } else {
        Err(ShiroError::InvalidInput {
            message: format!(
                "unsupported URL content type without recognized signature: {}",
                content_type.unwrap_or("unknown")
            ),
        })
    }
}

fn parser_for_signature(signature: &str) -> Box<dyn shiro_core::ports::Parser> {
    match signature {
        "pdf" => Box::new(shiro_parse::PdfParser),
        "markdown_utf8" => Box::new(shiro_parse::MarkdownParser),
        _ => Box::new(shiro_parse::PlainTextParser),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_private_and_unsafe_url_targets() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("10.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("::1".parse().unwrap()));
        assert!(!is_public_ip("64:ff9b::7f00:1".parse().unwrap()));
        assert!(!is_public_ip("2002:7f00:1::".parse().unwrap()));
        assert!(normalize_and_validate_url("file:///etc/passwd", false).is_err());
        assert!(normalize_and_validate_url("http://example.com", false).is_err());
        assert!(normalize_and_validate_url("https://user:secret@example.com", false).is_err());
    }

    #[test]
    fn content_signature_is_authoritative_and_text_is_bounded_to_utf8() {
        let url = Url::parse("https://example.com/file.bin").unwrap();
        assert_eq!(
            detect_signature(b"%PDF-1.7", Some("text/plain"), &url).unwrap(),
            "pdf"
        );
        assert_eq!(
            detect_signature(b"hello", Some("text/plain"), &url).unwrap(),
            "plaintext_utf8"
        );
        assert!(detect_signature(&[0xff, 0xfe], Some("text/plain"), &url).is_err());
    }
}
