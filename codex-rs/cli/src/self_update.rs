use anyhow::Result;
use anyhow::anyhow;
use chrono::DateTime;
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::Path;

use codex_core::user_agent::get_codex_user_agent;

// Baked-in repository information - this will be the fallback/default repo
const DEFAULT_REPO_OWNER: &str = "iainlowe";
const DEFAULT_REPO_NAME: &str = "codex";

// Primary repository to always check
const PRIMARY_REPO_OWNER: &str = "openai";
const PRIMARY_REPO_NAME: &str = "codex";

#[derive(Deserialize, Debug, Clone)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)] // Not used in current logic
    name: String,
    #[allow(dead_code)] // Not used in current logic
    body: String,
    #[allow(dead_code)] // Not used in current logic
    draft: bool,
    prerelease: bool,
    published_at: DateTime<Utc>,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    #[allow(dead_code)] // Kept for completeness but not used in current logic
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub repo: String,
    pub is_prerelease: bool,
    pub published_at: DateTime<Utc>,
    pub assets: Vec<GitHubAsset>,
    #[allow(dead_code)] // Kept for future use but not displayed in current logic
    pub body: String,
}

pub async fn list_releases(repo_override: Option<&str>) -> Result<Vec<Release>> {
    let client = Client::new();
    let user_agent = get_codex_user_agent(None);

    let mut all_releases = Vec::new();

    // Try to check the primary OpenAI repo, but don't fail if it's not accessible
    if let Ok(primary_releases) =
        fetch_releases_from_repo(&client, &user_agent, PRIMARY_REPO_OWNER, PRIMARY_REPO_NAME).await
    {
        for release in primary_releases {
            all_releases.push(Release {
                version: parse_version_from_tag(&release.tag_name),
                repo: format!("{PRIMARY_REPO_OWNER}/{PRIMARY_REPO_NAME}"),
                is_prerelease: release.prerelease,
                published_at: release.published_at,
                assets: release.assets,
                body: release.body,
            });
        }
    } else {
        eprintln!(
            "Warning: Could not fetch releases from {PRIMARY_REPO_OWNER}/{PRIMARY_REPO_NAME} (API rate limit or network issue)"
        );
    }

    // Check the override repo or default repo
    let (repo_owner, repo_name) = if let Some(repo) = repo_override {
        parse_repo_string(repo)?
    } else {
        (DEFAULT_REPO_OWNER, DEFAULT_REPO_NAME)
    };

    // Only fetch from secondary repo if it's different from primary
    if repo_owner != PRIMARY_REPO_OWNER || repo_name != PRIMARY_REPO_NAME {
        let secondary_releases =
            fetch_releases_from_repo(&client, &user_agent, repo_owner, repo_name).await?;

        for release in secondary_releases {
            all_releases.push(Release {
                version: parse_version_from_tag(&release.tag_name),
                repo: format!("{repo_owner}/{repo_name}"),
                is_prerelease: release.prerelease,
                published_at: release.published_at,
                assets: release.assets,
                body: release.body,
            });
        }
    }

    if all_releases.is_empty() {
        return Err(anyhow!("No releases found from any repository"));
    }

    // Sort by version (semver) descending
    all_releases.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (
            semver::Version::parse(&a.version),
            semver::Version::parse(&b.version),
        ) {
            (Ok(v_a), Ok(v_b)) => v_b.cmp(&v_a),  // Descending order
            (Ok(_), Err(_)) => Ordering::Less,    // Valid versions come first
            (Err(_), Ok(_)) => Ordering::Greater, // Valid versions come first
            (Err(_), Err(_)) => a.version.cmp(&b.version).reverse(), // Fallback to string comparison
        }
    });

    Ok(all_releases)
}

async fn fetch_releases_from_repo(
    client: &Client,
    user_agent: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<GitHubRelease>> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases");

    let response = client
        .get(&url)
        .header("User-Agent", user_agent)
        .send()
        .await?
        .error_for_status()?;

    let releases: Vec<GitHubRelease> = response.json().await?;
    Ok(releases)
}

fn parse_repo_string(repo: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Repository must be in format 'owner/repo'"));
    }
    Ok((parts[0], parts[1]))
}

fn parse_version_from_tag(tag_name: &str) -> String {
    // Handle different tag formats:
    // rust-v0.27.0 -> 0.27.0
    // v0.27.0 -> 0.27.0
    // 0.27.0 -> 0.27.0
    tag_name
        .strip_prefix("rust-v")
        .or_else(|| tag_name.strip_prefix("v"))
        .unwrap_or(tag_name)
        .to_string()
}

pub fn get_current_target_triple() -> String {
    env::var("CODEX_TARGET_TRIPLE")
        .or_else(|_| env::var("TARGET"))
        .unwrap_or_else(|_| {
            // Fallback to a reasonable default based on the platform
            #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "musl"))]
            {
                return "x86_64-unknown-linux-musl".to_string();
            }

            #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
            {
                return "x86_64-unknown-linux-gnu".to_string();
            }

            #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "musl"))]
            {
                return "aarch64-unknown-linux-musl".to_string();
            }

            #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
            {
                return "aarch64-unknown-linux-gnu".to_string();
            }

            #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
            {
                return "x86_64-apple-darwin".to_string();
            }

            #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
            {
                return "aarch64-apple-darwin".to_string();
            }

            #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
            {
                return "x86_64-pc-windows-msvc".to_string();
            }

            // Generic fallback for other platforms
            format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS)
        })
}

pub fn find_suitable_asset<'a>(
    assets: &'a [GitHubAsset],
    target_triple: &str,
) -> Option<&'a GitHubAsset> {
    // Priority order: .zst, .tar.gz, .zip (for Windows)
    let preferred_extensions = if target_triple.contains("windows") {
        vec![".exe.zst", ".exe.zip", ".exe.tar.gz"]
    } else {
        vec![".zst", ".tar.gz"]
    };

    for ext in preferred_extensions {
        if let Some(asset) = assets
            .iter()
            .find(|asset| asset.name.contains(target_triple) && asset.name.ends_with(ext))
        {
            return Some(asset);
        }
    }

    None
}

pub async fn download_and_replace_binary(asset: &GitHubAsset, target_triple: &str) -> Result<()> {
    let client = Client::new();
    let user_agent = get_codex_user_agent(None);

    // Download the asset
    let response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", user_agent)
        .send()
        .await?
        .error_for_status()?;

    let bytes = response.bytes().await?;

    // Get current executable path
    let current_exe = env::current_exe()?;
    let temp_path = current_exe.with_extension("tmp");

    // Extract and write the binary
    if asset.name.ends_with(".zst") {
        // Handle zstd compression
        let decompressed = zstd::decode_all(&bytes[..])?;
        fs::write(&temp_path, decompressed)?;
    } else if asset.name.ends_with(".tar.gz") {
        // Handle tar.gz
        extract_tar_gz(&bytes, &temp_path, target_triple)?;
    } else if asset.name.ends_with(".zip") {
        // Handle zip (primarily for Windows)
        extract_zip(&bytes, &temp_path, target_triple)?;
    } else {
        return Err(anyhow!("Unsupported asset format: {}", asset.name));
    }

    // Make executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_path, perms)?;
    }

    // Atomic replace: move temp file to replace current executable
    #[cfg(windows)]
    {
        // On Windows, we can't replace a running executable directly
        let backup_path = current_exe.with_extension("old");
        fs::rename(&current_exe, &backup_path)?;
        fs::rename(&temp_path, &current_exe)?;
        let _ = fs::remove_file(&backup_path); // Best effort cleanup
    }

    #[cfg(not(windows))]
    {
        fs::rename(&temp_path, &current_exe)?;
    }

    println!(
        "✅ Successfully updated to version from {}",
        asset.browser_download_url
    );
    Ok(())
}

fn extract_tar_gz(bytes: &[u8], output_path: &Path, _target_triple: &str) -> Result<()> {
    use std::io::Read;

    let tar = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(tar);

    // Look for the binary in the archive
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Look for a file that looks like our binary
        if let Some(filename) = path.file_name()
            && let Some(filename_str) = filename.to_str()
            && (filename_str == "codex" || filename_str.starts_with("codex-"))
        {
            let mut buffer = Vec::new();
            entry.read_to_end(&mut buffer)?;
            fs::write(output_path, buffer)?;
            return Ok(());
        }
    }

    Err(anyhow!("Could not find suitable binary in tar.gz archive"))
}

fn extract_zip(bytes: &[u8], output_path: &Path, _target_triple: &str) -> Result<()> {
    use std::io::Read;

    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    // Look for the binary in the archive
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;

        if let Some(filename) = file.name().split('/').next_back()
            && (filename == "codex.exe" || filename.starts_with("codex-"))
        {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            fs::write(output_path, buffer)?;
            return Ok(());
        }
    }

    Err(anyhow!("Could not find suitable binary in zip archive"))
}

pub fn print_releases_list(releases: &[Release]) {
    let current_version = env!("CARGO_PKG_VERSION");

    println!("Available releases (current: {current_version}):\n");

    for release in releases {
        let color = if release.is_prerelease {
            "\x1b[33m" // Yellow for prerelease
        } else if is_newer_version(&release.version, current_version) {
            "\x1b[32m" // Green for newer stable
        } else if release.version == current_version {
            "\x1b[36m" // Cyan for current
        } else {
            "\x1b[37m" // White for older
        };

        let reset = "\x1b[0m";
        let tag = if release.is_prerelease {
            " (prerelease)"
        } else {
            ""
        };

        println!(
            "{}v{}{} - {} - {}{}",
            color,
            release.version,
            tag,
            release.repo,
            release.published_at.format("%Y-%m-%d"),
            reset
        );
    }
}

fn is_newer_version(version: &str, current: &str) -> bool {
    match (
        semver::Version::parse(version),
        semver::Version::parse(current),
    ) {
        (Ok(v), Ok(c)) => v > c,
        _ => false,
    }
}

pub fn get_mock_releases() -> Vec<Release> {
    use chrono::prelude::*;

    vec![
        Release {
            version: "0.28.0".to_string(),
            repo: "openai/codex".to_string(),
            is_prerelease: false,
            published_at: Utc::now() - chrono::Duration::days(1),
            assets: vec![
                GitHubAsset {
                    name: "codex-x86_64-unknown-linux-musl.zst".to_string(),
                    browser_download_url: "https://github.com/openai/codex/releases/download/rust-v0.28.0/codex-x86_64-unknown-linux-musl.zst".to_string(),
                    size: 8_000_000,
                },
                GitHubAsset {
                    name: "codex-aarch64-apple-darwin.zst".to_string(),
                    browser_download_url: "https://github.com/openai/codex/releases/download/rust-v0.28.0/codex-aarch64-apple-darwin.zst".to_string(),
                    size: 6_000_000,
                },
            ],
            body: "Latest stable release".to_string(),
        },
        Release {
            version: "0.27.1-beta-auto-switch-auth".to_string(),
            repo: "iainlowe/codex".to_string(),
            is_prerelease: true,
            published_at: Utc::now() - chrono::Duration::days(2),
            assets: vec![],
            body: "Beta release with auto switching".to_string(),
        },
        Release {
            version: "0.27.0".to_string(),
            repo: "openai/codex".to_string(),
            is_prerelease: false,
            published_at: Utc::now() - chrono::Duration::days(5),
            assets: vec![
                GitHubAsset {
                    name: "codex-x86_64-unknown-linux-musl.zst".to_string(),
                    browser_download_url: "https://github.com/openai/codex/releases/download/rust-v0.27.0/codex-x86_64-unknown-linux-musl.zst".to_string(),
                    size: 8_000_000,
                },
            ],
            body: "Previous stable release".to_string(),
        },
        Release {
            version: "0.26.0".to_string(),
            repo: "openai/codex".to_string(),
            is_prerelease: false,
            published_at: Utc::now() - chrono::Duration::days(10),
            assets: vec![],
            body: "Older stable release".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_from_tag() {
        assert_eq!(parse_version_from_tag("rust-v0.27.0"), "0.27.0");
        assert_eq!(parse_version_from_tag("v0.27.0"), "0.27.0");
        assert_eq!(parse_version_from_tag("0.27.0"), "0.27.0");
        assert_eq!(
            parse_version_from_tag("rust-v0.27.0-alpha.1"),
            "0.27.0-alpha.1"
        );
    }

    #[test]
    fn test_parse_repo_string() {
        assert_eq!(parse_repo_string("owner/repo").unwrap(), ("owner", "repo"));
        assert!(parse_repo_string("invalid").is_err());
        assert!(parse_repo_string("too/many/parts").is_err());
    }
}
