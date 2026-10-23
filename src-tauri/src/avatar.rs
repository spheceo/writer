//! The photos of the people who started scripts (their Google picture, from
//! the `createdBy` a script records). Writer's window loads no remote images,
//! so each is fetched once, here, into `<data>/avatars/`, and shown from disk:
//! no request to Google on every view, and it works offline. A photo is
//! fetched again only if its address changes.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

const MAX_BYTES: u64 = 1_000_000;

/// Where a person's photo is kept: one file per person and address.
fn file(data: &Path, sub: &str, picture: &str) -> PathBuf {
    let digest = Sha256::digest(format!("{sub}\n{picture}").as_bytes());
    let name: String = digest.iter().take(10).map(|byte| format!("{byte:02x}")).collect();
    data.join("avatars").join(format!("{name}.img"))
}

/// The photo on disk, fetched first if it isn't yet. None when there's no
/// photo to have (not https, not an image, too large, or unreachable): the
/// page then shows the person's initials.
pub fn local(data: &Path, sub: &str, picture: &str) -> Option<PathBuf> {
    if !picture.starts_with("https://") || picture.len() > 2_000 || picture.chars().any(char::is_control) {
        return None;
    }
    let path = file(data, sub, picture);
    if path.is_file() {
        return Some(path);
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(20)).user_agent("Writer").build();
    let response = agent.get(picture).call().ok()?;
    if !response.content_type().starts_with("image/") {
        return None;
    }
    let mut bytes = Vec::new();
    response.into_reader().take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path)).ok()?;
    Some(path)
}

/// The image's type from its first bytes, for serving it.
pub fn content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_one_file_per_person_and_address_and_refuses_odd_ones() {
        let data = Path::new("/tmp/w");
        assert_eq!(file(data, "p1", "https://a/1"), file(data, "p1", "https://a/1"));
        assert_ne!(file(data, "p1", "https://a/1"), file(data, "p1", "https://a/2"));
        assert!(local(data, "p1", "http://insecure/photo").is_none());
        assert!(local(data, "p1", "file:///etc/passwd").is_none());
        assert_eq!(content_type(&[0x89, b'P', b'N', b'G', 0]), "image/png");
        assert_eq!(content_type(&[0xff, 0xd8, 0xff]), "image/jpeg");
    }
}
