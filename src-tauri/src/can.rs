//! What the signed-in person may do in Writer: the kinds of work an admin
//! granted them at accounts.spheceo.com (relay/src/manifest.ts publishes the
//! list), read from their access token's `perms`. Admins may do everything.
//!
//! A script of a kind someone can't write is hidden from them: not listed,
//! not opened, not used as a source, no agent work on it. Making review
//! links, approving, fixing notes and exporting to Docs need `review.share`;
//! teaching Writer from feedback needs `review.lessons`. Leaving a comment on
//! a review stays open to everyone (that's on the review page, not here).

use std::collections::HashSet;

use crate::formats::Kind;
use crate::model::{Script, Summary};

pub const INTERROGATION: &str = "scripts.interrogation";
pub const INTERROGATION_COMPILATION: &str = "scripts.interrogation-compilation";
pub const FOIA: &str = "scripts.foia";
pub const COURTROOM: &str = "scripts.courtroom";
pub const COURTROOM_COMPILATION: &str = "scripts.courtroom-compilation";
pub const COMMENTARY: &str = "scripts.commentary";
pub const SHARE: &str = "review.share";
pub const LESSONS: &str = "review.lessons";

/// What someone was granted.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    admin: bool,
    perms: HashSet<String>,
}

impl Grants {
    pub fn new(admin: bool, perms: &[String]) -> Self {
        Self { admin, perms: perms.iter().cloned().collect() }
    }

    pub fn has(&self, id: &str) -> bool {
        self.admin || self.perms.contains(id)
    }

    /// May write (and so see) a script of this kind.
    pub fn may_write(&self, work: &str) -> bool {
        self.has(work)
    }

    /// The one-case formats this person may write, in the editor's order.
    pub fn single_kinds(&self) -> Vec<Kind> {
        [(Kind::Interrogation, INTERROGATION), (Kind::Foia, FOIA), (Kind::Courtroom, COURTROOM)].into_iter().filter(|(_, id)| self.has(id)).map(|(kind, _)| kind).collect()
    }

    /// The compilation formats this person may write (FOIA has none).
    pub fn compilation_kinds(&self) -> Vec<Kind> {
        [(Kind::Interrogation, INTERROGATION_COMPILATION), (Kind::Courtroom, COURTROOM_COMPILATION)].into_iter().filter(|(_, id)| self.has(id)).map(|(kind, _)| kind).collect()
    }
}

/// The signed-in person's grants (none when signed out). Unit tests, where
/// no one signs in, act as an admin; this module's own tests check grants.
pub fn grants() -> Grants {
    if cfg!(test) {
        return Grants::new(true, &[]);
    }
    crate::account::status().person.map(|person| Grants::new(person.admin, &person.perms)).unwrap_or_default()
}

/// The kind of work a script is.
pub fn work_of(source_kind: &str, compilation: bool, kind: Kind) -> &'static str {
    if source_kind == "commentary" {
        return COMMENTARY;
    }
    match (compilation, kind) {
        (true, Kind::Courtroom) => COURTROOM_COMPILATION,
        // FOIA compilations were never a format; any made before count as interrogation compilations.
        (true, _) => INTERROGATION_COMPILATION,
        (false, Kind::Interrogation) => INTERROGATION,
        (false, Kind::Foia) => FOIA,
        (false, Kind::Courtroom) => COURTROOM,
    }
}

pub fn script_work(script: &Script) -> &'static str {
    work_of(&script.source_kind, script.compilation, script.kind)
}

pub fn summary_work(summary: &Summary) -> &'static str {
    work_of(&summary.source_kind, summary.compilation, summary.kind)
}

/// Visible to this person (they may write its kind).
pub fn sees(grants: &Grants, script: &Script) -> bool {
    grants.may_write(script_work(script))
}

/// What a capability lets someone do, as a refusal says it.
pub fn doing(id: &str) -> &'static str {
    match id {
        INTERROGATION => "write interrogation scripts in Writer",
        INTERROGATION_COMPILATION => "write interrogation compilations in Writer",
        FOIA => "write FOIA scripts in Writer",
        COURTROOM => "write courtroom scripts in Writer",
        COURTROOM_COMPILATION => "write courtroom compilations in Writer",
        COMMENTARY => "write commentary scripts in Writer",
        SHARE => "share scripts for review in Writer",
        LESSONS => "teach Writer from feedback",
        _ => "do that in Writer",
    }
}

/// Ok when granted, or the plain refusal.
pub fn require(id: &str) -> Result<(), String> {
    if grants().has(id) { Ok(()) } else { Err(refusal(id)) }
}

pub fn refusal(id: &str) -> String {
    format!("Your account can't {} yet. Ask an admin at accounts.spheceo.com.", doing(id))
}

pub const NOT_AVAILABLE: &str = "This script isn't available to your account.";

/// Ok when this person may see the script, or "isn't available".
pub fn require_script(script: &Script) -> Result<(), String> {
    if sees(&grants(), script) { Ok(()) } else { Err(NOT_AVAILABLE.into()) }
}

/// The same, for a script by its folder.
pub fn require_id(data: &std::path::Path, id: &str) -> Result<(), String> {
    let dir = crate::store::script_dir(data, id)?;
    match crate::store::read(&dir) {
        Ok(script) => require_script(&script),
        // Missing or unreadable: let the caller say so in its own words.
        Err(_) => Ok(()),
    }
}

/// The format a new one-case import starts as: the first this person may write.
pub fn first_single_kind() -> Result<Kind, String> {
    grants().single_kinds().into_iter().next().ok_or_else(|| refusal(INTERROGATION))
}

/// The format a new compilation starts as: the first this person may write.
pub fn first_compilation_kind() -> Result<Kind, String> {
    grants().compilation_kinds().into_iter().next().ok_or_else(|| refusal(INTERROGATION_COMPILATION))
}

/// Building as `kind`: refused when the person can't write that kind, or for a FOIA compilation.
pub fn require_kind(script: &Script, kind: Kind) -> Result<(), String> {
    if script.compilation && kind == Kind::Foia {
        return Err("FOIA compilations aren't one of the formats. Build it as an interrogation or courtroom compilation.".into());
    }
    let work = work_of(&script.source_kind, script.compilation, kind);
    require(work)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_each_kind_of_script() {
        assert_eq!(work_of("youtube", false, Kind::Interrogation), INTERROGATION);
        assert_eq!(work_of("drive", false, Kind::Foia), FOIA);
        assert_eq!(work_of("file", false, Kind::Courtroom), COURTROOM);
        // One hearing across several videos is a one-case courtroom script.
        assert_eq!(work_of("compilation", false, Kind::Courtroom), COURTROOM);
        assert_eq!(work_of("compilation", true, Kind::Interrogation), INTERROGATION_COMPILATION);
        assert_eq!(work_of("compilation", true, Kind::Foia), INTERROGATION_COMPILATION);
        assert_eq!(work_of("compilation", true, Kind::Courtroom), COURTROOM_COMPILATION);
        assert_eq!(work_of("commentary", false, Kind::Interrogation), COMMENTARY);
    }

    #[test]
    fn grants_what_was_given_and_admins_everything() {
        let writer = Grants::new(false, &["use".into(), INTERROGATION_COMPILATION.into(), "cloud".into()]);
        assert!(writer.may_write(INTERROGATION_COMPILATION));
        assert!(!writer.may_write(INTERROGATION));
        assert!(!writer.has(SHARE));
        assert_eq!(writer.single_kinds(), Vec::<Kind>::new());
        assert_eq!(writer.compilation_kinds(), vec![Kind::Interrogation]);
        let foia = Grants::new(false, &[FOIA.into(), COURTROOM.into()]);
        assert_eq!(foia.single_kinds(), vec![Kind::Foia, Kind::Courtroom]);
        let admin = Grants::new(true, &[]);
        assert!(admin.has(LESSONS) && admin.may_write(COMMENTARY));
        assert!(!Grants::default().has(INTERROGATION));
    }

    #[test]
    fn refusals_say_what_and_who_to_ask() {
        assert_eq!(refusal(COURTROOM_COMPILATION), "Your account can't write courtroom compilations in Writer yet. Ask an admin at accounts.spheceo.com.");
    }
}
