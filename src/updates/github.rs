use super::{RELEASES_URL, Update};
use serde::Deserialize;
use std::time::Duration;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/gokcol/Seyir/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    state: String,
    size: u64,
}

fn version_number(value: &str) -> Option<[u64; 3]> {
    let mut parts = value.split('.');
    let mut version = [0; 3];
    for component in &mut version {
        let part = parts.next()?;
        if part.is_empty()
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        *component = part.parse().ok()?;
    }
    parts.next().is_none().then_some(version)
}

fn evaluate(json: &str, installed: &str, architecture: &str) -> Result<Update, String> {
    let release: Release = serde_json::from_str(json)
        .map_err(|_| "GitHub returned an unreadable release. Try again later.")?;
    if release.draft || release.prerelease {
        return Err("GitHub did not return a stable Seyir release.".into());
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .ok_or("The latest GitHub release has an unrecognized version.")?;
    let latest =
        version_number(version).ok_or("The latest GitHub release has an unrecognized version.")?;
    let current = version_number(installed).ok_or("This build has an unrecognized version.")?;
    if latest <= current {
        return Ok(Update::Current);
    }

    let arch = match architecture {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => return Err("No GitHub download is available for this Mac processor.".into()),
    };
    let compatible = ["dmg", "zip"].into_iter().any(|extension| {
        let name = format!("Seyir-{version}-macos-{arch}.{extension}");
        let url = format!("{RELEASES_URL}/download/{}/{name}", release.tag_name);
        release.assets.iter().any(|asset| {
            asset.name == name
                && asset.browser_download_url == url
                && asset.state == "uploaded"
                && asset.size > 0
        })
    });
    if !compatible {
        return Err("The latest GitHub release does not yet have a download for this Mac.".into());
    }
    Ok(Update::Available {
        version: version.into(),
        url: format!("{RELEASES_URL}/tag/{}", release.tag_name),
    })
}

pub(super) fn check() -> Result<Update, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(20)))
            .build(),
    );
    let mut response = match agent
        .get(LATEST_RELEASE_API)
        .header("User-Agent", "Seyir-update-check")
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(403 | 429)) => {
            return Err("GitHub is limiting update checks. Try again later.".into());
        }
        Err(ureq::Error::StatusCode(404)) => {
            return Err("GitHub did not return a stable Seyir release.".into());
        }
        Err(_) => {
            return Err("Couldn't reach GitHub. Check your connection and try again.".into());
        }
    };
    let json = response
        .body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_to_string()
        .map_err(|_| "Couldn't read the GitHub response. Try again later.")?;
    evaluate(&json, env!("CARGO_PKG_VERSION"), std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn release(version: &str, filename: &str) -> Value {
        json!({
            "tag_name": format!("v{version}"),
            "draft": false,
            "prerelease": false,
            "assets": [{
                "name": filename,
                "browser_download_url": format!("{RELEASES_URL}/download/v{version}/{filename}"),
                "state": "uploaded",
                "size": 42,
            }],
        })
    }

    #[test]
    fn checks_stable_versions_in_numeric_order() {
        let response = release("1.10.2", "Seyir-1.10.2-macos-arm64.dmg").to_string();
        assert!(matches!(
            evaluate(&response, "1.9.15", "aarch64"),
            Ok(Update::Available { version, url })
                if version == "1.10.2" && url == format!("{RELEASES_URL}/tag/v1.10.2")
        ));
        for current in ["1.10.2", "1.10.3", "2.0.0"] {
            assert!(matches!(
                evaluate(&response, current, "aarch64"),
                Ok(Update::Current)
            ));
        }
    }

    #[test]
    fn rejects_invalid_versions_and_nonpublic_releases() {
        for version in ["1.0", "1.0.2.3", "1.01.0", "1.+0.2", "1.0.2-rc1", "1.0.2/"] {
            let response = release(version, "Seyir-1.0.2-macos-arm64.dmg").to_string();
            assert!(evaluate(&response, "1.0.1", "aarch64").is_err());
        }
        for key in ["draft", "prerelease"] {
            let mut response = release("1.0.2", "Seyir-1.0.2-macos-arm64.dmg");
            response[key] = true.into();
            assert!(evaluate(&response.to_string(), "1.0.1", "aarch64").is_err());
        }
    }

    #[test]
    fn requires_an_uploaded_macos_asset_from_the_same_release() {
        let response = release("1.0.2", "Seyir-1.0.2-macos-arm64.zip");
        assert!(matches!(
            evaluate(&response.to_string(), "1.0.1", "aarch64"),
            Ok(Update::Available { .. })
        ));
        assert!(evaluate(&response.to_string(), "1.0.1", "x86_64").is_err());
        for (key, value) in [
            ("name", json!("Seyir-1.0.2-windows-x64-Setup.exe")),
            (
                "browser_download_url",
                json!("https://example.com/seyir.zip"),
            ),
            ("state", json!("new")),
            ("size", json!(0)),
        ] {
            let mut response = response.clone();
            response["assets"][0][key] = value;
            assert!(evaluate(&response.to_string(), "1.0.1", "aarch64").is_err());
        }
        assert!(evaluate("not JSON", "1.0.1", "aarch64").is_err());
    }

    #[test]
    #[ignore = "Contacts the public GitHub release API"]
    fn live_github_release_check() {
        assert!(matches!(
            check().expect("GitHub API returns a stable Seyir release"),
            Update::Current | Update::Available { .. }
        ));
    }
}
