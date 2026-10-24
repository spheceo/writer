//! This computer, as accounts.spheceo.com's Fixing page knows it: a stable
//! id and its name. The id is a hash of the Mac's hardware UUID
//! (`IOPlatformUUID`), so Writer and Edit on one Mac are one computer; Edit
//! works it out the same way (`apps/server/src/machine.ts`). Linux uses
//! `/etc/machine-id`; failing both, the host name. Only the hash leaves it.

use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// The computer's id: the first 24 hex characters of SHA-256 of `spheceo-machine:<hardware id>`.
pub fn id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| hashed(&hardware_id()))
}

/// The computer's own name ("Sphe's MacBook Pro"), as Repack and the Fixing page show it.
pub fn name() -> String {
    crate::cloud::holder_name()
}

fn hashed(hardware: &str) -> String {
    let digest = Sha256::digest(format!("spheceo-machine:{hardware}").as_bytes());
    digest.iter().take(12).map(|byte| format!("{byte:02x}")).collect()
}

fn hardware_id() -> String {
    let ioreg = std::process::Command::new("ioreg").args(["-rd1", "-c", "IOPlatformExpertDevice"]).output().ok().and_then(|out| String::from_utf8(out.stdout).ok());
    if let Some(uuid) = ioreg.as_deref().and_then(platform_uuid) {
        return uuid;
    }
    if let Some(id) = std::fs::read_to_string("/etc/machine-id").ok().map(|text| text.trim().to_string()).filter(|id| !id.is_empty()) {
        return id;
    }
    std::process::Command::new("hostname").output().ok().and_then(|out| String::from_utf8(out.stdout).ok()).map(|name| name.trim().to_string()).filter(|name| !name.is_empty()).unwrap_or_else(|| "unknown".into())
}

/// `"IOPlatformUUID" = "…"` in ioreg's answer.
fn platform_uuid(ioreg: &str) -> Option<String> {
    let line = ioreg.lines().find(|line| line.contains("\"IOPlatformUUID\""))?;
    let value = line.split('=').nth(1)?.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_the_platform_uuid_as_edit_does() {
        let ioreg = "+-o J314sAP  <class IOPlatformExpertDevice>\n    \"IOPlatformSerialNumber\" = \"X\"\n    \"IOPlatformUUID\" = \"1B2C3D4E-0000-1111-2222-333344445555\"\n";
        assert_eq!(platform_uuid(ioreg).as_deref(), Some("1B2C3D4E-0000-1111-2222-333344445555"));
        // The same value Edit's machine.ts test expects.
        assert_eq!(hashed("1B2C3D4E-0000-1111-2222-333344445555"), "4a96f96fd1a3ae9208835d1c");
        assert_eq!(id().len(), 24);
    }
}
