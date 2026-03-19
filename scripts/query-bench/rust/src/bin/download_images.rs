use query::{execute_with_refresh, get_token, load_env_file};
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::{env, fs, process};

/// Search for issue by identifier (team key + number).
const ISSUE_SEARCH_QUERY: &str = r#"
query($filter: IssueFilter!) {
  issues(filter: $filter, first: 1) {
    nodes {
      id
      identifier
      title
      description
      comments {
        nodes {
          body
        }
      }
      attachments {
        nodes {
          url
          title
        }
      }
    }
  }
}
"#;

#[derive(Serialize)]
struct DownloadResult {
    issue_identifier: String,
    output_dir: String,
    images: Vec<ImageInfo>,
}

#[derive(Serialize)]
struct ImageInfo {
    url: String,
    local_path: String,
    source: String,
    filename: String,
}

/// Extract image URLs from markdown text.
/// Matches: ![alt](url) and bare https://uploads.linear.app/... URLs.
fn extract_image_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();

    // Match markdown images: ![...](url)
    let md_re = Regex::new(r"!\[[^\]]*\]\(([^)]+)\)").unwrap();
    for cap in md_re.captures_iter(text) {
        urls.push(cap[1].to_string());
    }

    // Match bare Linear upload URLs not already captured by markdown syntax
    let bare_re = Regex::new(r"https://uploads\.linear\.app/[^\s)\]]+").unwrap();
    for m in bare_re.find_iter(text) {
        let url = m.as_str().to_string();
        if !urls.contains(&url) {
            urls.push(url);
        }
    }

    urls
}

/// Determine file extension from URL or content-type header.
fn extension_from_url(url: &str) -> &str {
    let path = url.split('?').next().unwrap_or(url);
    if path.ends_with(".png") {
        "png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "jpg"
    } else if path.ends_with(".gif") {
        "gif"
    } else if path.ends_with(".webp") {
        "webp"
    } else if path.ends_with(".svg") {
        "svg"
    } else if path.ends_with(".pdf") {
        "pdf"
    } else {
        "png"
    }
}

/// Parse issue identifier like "ENG-123" into (team_key, number).
fn parse_identifier(identifier: &str) -> Option<(String, i64)> {
    let parts: Vec<&str> = identifier.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    let number: i64 = parts[1].parse().ok()?;
    Some((parts[0].to_string(), number))
}

fn print_usage() {
    eprintln!("Download images from a Linear issue to a local directory.\n");
    eprintln!("Usage:");
    eprintln!("  download_images <issue-identifier> [output-dir]\n");
    eprintln!("Examples:");
    eprintln!("  download_images ENG-123");
    eprintln!("  download_images ENG-123 /tmp/my-images");
    eprintln!("\nDefaults to /tmp/linear-images/<identifier>/");
}

#[tokio::main]
async fn main() {
    load_env_file();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let identifier = &args[1];
    let (team_key, number) = match parse_identifier(identifier) {
        Some(v) => v,
        None => {
            eprintln!("Error: Invalid issue identifier '{}'. Expected format: ENG-123", identifier);
            process::exit(1);
        }
    };

    let output_dir = if args.len() > 2 {
        PathBuf::from(&args[2])
    } else {
        PathBuf::from(format!("/tmp/linear-images/{}", identifier))
    };

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let client = Client::new();

    // Query Linear for the issue using team key + number filter
    let variables = json!({
        "filter": {
            "team": { "key": { "eq": team_key } },
            "number": { "eq": number }
        }
    });

    let data = match execute_with_refresh(
        &client,
        &token,
        ISSUE_SEARCH_QUERY,
        Some(variables),
    )
    .await
    {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error querying Linear: {}", e);
            process::exit(1);
        }
    };

    let issue = match data
        .get("issues")
        .and_then(|i| i.get("nodes"))
        .and_then(|n| n.as_array())
        .and_then(|a| a.first())
    {
        Some(i) => i,
        None => {
            eprintln!("Error: Issue '{}' not found", identifier);
            process::exit(1);
        }
    };

    // Collect all text content to scan for images
    let mut all_urls: Vec<(String, String)> = Vec::new(); // (url, source)
    let mut seen: HashSet<String> = HashSet::new();

    // From description
    if let Some(desc) = issue.get("description").and_then(|d| d.as_str()) {
        for url in extract_image_urls(desc) {
            if seen.insert(url.clone()) {
                all_urls.push((url, "description".to_string()));
            }
        }
    }

    // From comments
    if let Some(comments) = issue
        .get("comments")
        .and_then(|c| c.get("nodes"))
        .and_then(|n| n.as_array())
    {
        for (i, comment) in comments.iter().enumerate() {
            if let Some(body) = comment.get("body").and_then(|b| b.as_str()) {
                for url in extract_image_urls(body) {
                    if seen.insert(url.clone()) {
                        all_urls.push((url, format!("comment-{}", i)));
                    }
                }
            }
        }
    }

    // From attachments (direct URLs)
    if let Some(attachments) = issue
        .get("attachments")
        .and_then(|a| a.get("nodes"))
        .and_then(|n| n.as_array())
    {
        for (i, attachment) in attachments.iter().enumerate() {
            if let Some(url) = attachment.get("url").and_then(|u| u.as_str()) {
                if is_image_url(url) && seen.insert(url.to_string()) {
                    all_urls.push((url.to_string(), format!("attachment-{}", i)));
                }
            }
        }
    }

    if all_urls.is_empty() {
        let result = DownloadResult {
            issue_identifier: identifier.clone(),
            output_dir: output_dir.to_string_lossy().to_string(),
            images: vec![],
        };
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        eprintln!("[INFO] No images found in issue {}", identifier);
        return;
    }

    // Create output directory
    if let Err(e) = fs::create_dir_all(&output_dir) {
        eprintln!("Error creating directory {}: {}", output_dir.display(), e);
        process::exit(1);
    }

    eprintln!(
        "[INFO] Found {} image(s) in issue {}, downloading to {}",
        all_urls.len(),
        identifier,
        output_dir.display()
    );

    // Download each image with authentication
    let mut images: Vec<ImageInfo> = Vec::new();
    let current_token = get_token().unwrap_or_default();

    for (i, (url, source)) in all_urls.iter().enumerate() {
        let ext = extension_from_url(url);
        let filename = format!("image-{}.{}", i, ext);
        let local_path = output_dir.join(&filename);

        eprintln!("[INFO] Downloading {} -> {}", url, local_path.display());

        match download_image(&client, &current_token, url, &local_path).await {
            Ok(_) => {
                images.push(ImageInfo {
                    url: url.clone(),
                    local_path: local_path.to_string_lossy().to_string(),
                    source: source.clone(),
                    filename: filename.clone(),
                });
            }
            Err(e) => {
                eprintln!("[WARN] Failed to download {}: {}", url, e);
            }
        }
    }

    let result = DownloadResult {
        issue_identifier: identifier.clone(),
        output_dir: output_dir.to_string_lossy().to_string(),
        images,
    };

    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}

/// Download a single image, trying with auth header first, then without.
async fn download_image(
    client: &Client,
    token: &str,
    url: &str,
    local_path: &PathBuf,
) -> Result<(), String> {
    // Try with auth header first (for Linear-hosted uploads)
    let response = client
        .get(url)
        .header("Authorization", token)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();

    // If auth header causes issues (e.g. for S3 signed URLs), retry without
    if !status.is_success() {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Network error (no auth): {}", e))?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("HTTP {}", status));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read body: {}", e))?;

        fs::write(local_path, &bytes)
            .map_err(|e| format!("Failed to write file: {}", e))?;

        return Ok(());
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;

    fs::write(local_path, &bytes)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(())
}

/// Check if a URL looks like an image based on extension or domain.
fn is_image_url(url: &str) -> bool {
    let path = url.split('?').next().unwrap_or(url).to_lowercase();
    let image_extensions = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp"];
    if image_extensions.iter().any(|ext| path.ends_with(ext)) {
        return true;
    }
    url.contains("uploads.linear.app")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_markdown_images() {
        let text = "Here is an image: ![screenshot](https://uploads.linear.app/abc/def.png)";
        let urls = extract_image_urls(text);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://uploads.linear.app/abc/def.png");
    }

    #[test]
    fn test_extract_multiple_images() {
        let text = "![a](https://uploads.linear.app/1.png) text ![b](https://example.com/2.jpg)";
        let urls = extract_image_urls(text);
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn test_extract_bare_linear_urls() {
        let text = "Check this: https://uploads.linear.app/org/file.png and more text";
        let urls = extract_image_urls(text);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("uploads.linear.app"));
    }

    #[test]
    fn test_no_duplicate_urls() {
        let text = "![img](https://uploads.linear.app/a.png) also https://uploads.linear.app/a.png";
        let urls = extract_image_urls(text);
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn test_no_images() {
        let text = "Just regular text with no images";
        let urls = extract_image_urls(text);
        assert!(urls.is_empty());
    }

    #[test]
    fn test_extension_from_url() {
        assert_eq!(extension_from_url("https://example.com/file.png"), "png");
        assert_eq!(extension_from_url("https://example.com/file.jpg"), "jpg");
        assert_eq!(extension_from_url("https://example.com/file.jpeg"), "jpg");
        assert_eq!(extension_from_url("https://example.com/file.gif"), "gif");
        assert_eq!(extension_from_url("https://example.com/file.webp"), "webp");
        assert_eq!(extension_from_url("https://example.com/file?query=1"), "png"); // default
    }

    #[test]
    fn test_extension_from_url_with_query_params() {
        assert_eq!(
            extension_from_url("https://example.com/file.jpg?token=abc"),
            "jpg"
        );
    }

    #[test]
    fn test_parse_identifier() {
        assert_eq!(parse_identifier("ENG-123"), Some(("ENG".to_string(), 123)));
        assert_eq!(parse_identifier("DP-1"), Some(("DP".to_string(), 1)));
        assert_eq!(parse_identifier("invalid"), None);
        assert_eq!(parse_identifier("ENG-abc"), None);
    }

    #[test]
    fn test_is_image_url() {
        assert!(is_image_url("https://example.com/file.png"));
        assert!(is_image_url("https://example.com/file.jpg"));
        assert!(is_image_url("https://uploads.linear.app/anything"));
        assert!(!is_image_url("https://example.com/file.txt"));
    }
}
