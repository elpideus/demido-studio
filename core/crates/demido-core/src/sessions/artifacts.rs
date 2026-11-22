//! Artifacts: files a model makes in a session and gives the person to keep (brief 4.24; DECISIONS Q10 to
//! Q14).
//!
//! An artifact is the session's own, not a file in its folder: a session may have no folder, and what a
//! model gives the person is not a change to their files. It is kept in Demido's storage, every version of
//! it, and goes when the session goes. The model names it, with the ending that says what it is
//! (`plan.md`, `page.html`, `rename.py`); the same name again is a new version of the same artifact.

use std::path::Path;

use rusqlite::OptionalExtension;
use serde::Serialize;
use utoipa::ToSchema;

use super::Sessions;
use crate::folder::Kind;
use crate::{Error, Result, now_ms};

/// The most text a version may hold: 1 MB. More than a model writes in one answer.
pub const MOST_TEXT: usize = 1_000_000;
/// The longest name.
const MOST_NAME: usize = 100;

/// One artifact, as it is now.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Artifact {
    pub id: String,
    pub session_id: String,
    /// As the model named it, with its ending.
    pub name: String,
    /// How the window shows it: `markdown`, `html`, `svg` or `text` (code and any other text).
    pub shown_as: String,
    /// How many versions it has. The last is the one shown first.
    pub versions: i64,
    /// Bytes, of the last version.
    pub size: i64,
    pub created_at: i64,
    /// When its last version was made.
    pub updated_at: i64,
}

/// One version of an artifact, with its text.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Version {
    /// 1 for the first.
    pub n: i64,
    pub text: String,
    pub made_at: i64,
    /// The answer that made it.
    pub message_id: Option<String>,
}

/// One version, without its text: for the list of them.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct VersionFacts {
    pub n: i64,
    pub size: i64,
    pub made_at: i64,
    pub message_id: Option<String>,
}

/// What making an artifact did.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub artifact: Artifact,
    /// The version that holds the text now.
    pub n: i64,
    /// The text was the same as the last version's, so no version was added.
    pub same: bool,
}

/// How an artifact of this name is shown; `Err` for a name that is not of text.
pub fn shown_as(name: &str) -> std::result::Result<&'static str, String> {
    match crate::folder::kind_by_name(Path::new(name)) {
        None | Some(Kind::Text) | Some(Kind::Other) => Ok("text"),
        Some(Kind::Markdown) => Ok("markdown"),
        Some(Kind::Html) => Ok("html"),
        Some(Kind::Svg) => Ok("svg"),
        Some(Kind::Image | Kind::Pdf | Kind::Video | Kind::Audio) => {
            Err(format!("\"{name}\" is the name of a picture, a PDF or a recording, and an artifact is text: a document, a page, a drawing in SVG, code"))
        }
    }
}

/// The name, as it is kept: one file name, with an ending. A name without one is taken as Markdown.
pub fn name_of(name: &str) -> std::result::Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("an artifact needs a name, with an ending that says what it is: plan.md, page.html".into());
    }
    if name.contains(['/', '\\']) || name.chars().any(char::is_control) || name == "." || name == ".." {
        return Err(format!("\"{name}\" is not a file name. An artifact has a name of its own, such as plan.md, not a path"));
    }
    if name.chars().count() > MOST_NAME {
        return Err(format!("the name is longer than {MOST_NAME} letters"));
    }
    Ok(if Path::new(name).extension().is_some() { name.to_string() } else { format!("{name}.md") })
}

/// A reason as a sentence of its own: with a capital, and a full stop.
fn said(why: &str) -> String {
    let mut letters = why.chars();
    letters.next().map(|first| format!("{}{}.", first.to_uppercase(), letters.as_str())).unwrap_or_default()
}

const ARTIFACT: &str = "SELECT a.id, a.session_id, a.name, a.created_at,
        (SELECT max(n) FROM artifact_versions v WHERE v.artifact_id = a.id),
        (SELECT length(CAST(text AS BLOB)) FROM artifact_versions v WHERE v.artifact_id = a.id ORDER BY n DESC LIMIT 1),
        (SELECT max(made_at) FROM artifact_versions v WHERE v.artifact_id = a.id)
    FROM artifacts a";

fn artifact_from(r: &rusqlite::Row) -> rusqlite::Result<Artifact> {
    let name: String = r.get(2)?;
    Ok(Artifact {
        id: r.get(0)?,
        session_id: r.get(1)?,
        shown_as: shown_as(&name).unwrap_or("text").to_string(),
        name,
        created_at: r.get(3)?,
        versions: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
        size: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
        updated_at: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
    })
}

impl Sessions {
    /// Gives the person an artifact: a new one, or a new version of the one of that name in the session.
    /// Text the same as its last version adds no version.
    pub fn make_artifact(&self, session_id: &str, name: &str, text: &str, message_id: Option<&str>) -> Result<Made> {
        let name = name_of(name).map_err(|why| Error::Invalid(said(&why)))?;
        shown_as(&name).map_err(|why| Error::Invalid(said(&why)))?;
        if text.trim().is_empty() {
            return Err(Error::Invalid("An artifact needs its text.".into()));
        }
        if text.len() > MOST_TEXT {
            return Err(Error::Invalid(format!("That is more than {} MB, the most an artifact holds.", MOST_TEXT / 1_000_000)));
        }
        self.get(session_id)?;
        let now = now_ms();
        let (id, n, same) = self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            let found: Option<String> = tx.query_row("SELECT id FROM artifacts WHERE session_id = ?1 AND name = ?2", rusqlite::params![session_id, name], |r| r.get(0)).optional()?;
            let id = match found {
                Some(id) => id,
                None => {
                    let id = uuid::Uuid::now_v7().to_string();
                    tx.execute("INSERT INTO artifacts (id, session_id, name, created_at) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![id, session_id, name, now])?;
                    id
                }
            };
            let last: Option<(i64, String)> = tx.query_row("SELECT n, text FROM artifact_versions WHERE artifact_id = ?1 ORDER BY n DESC LIMIT 1", [&id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            let (n, same) = match last {
                Some((n, was)) if was == text => (n, true),
                Some((n, _)) => (n + 1, false),
                None => (1, false),
            };
            if !same {
                tx.execute("INSERT INTO artifact_versions (artifact_id, n, text, made_at, message_id) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![id, n, text, now, message_id])?;
            }
            tx.commit()?;
            Ok((id, n, same))
        })?;
        Ok(Made { artifact: self.artifact(&id)?, n, same })
    }

    /// A session's artifacts, the first made first.
    pub fn artifacts(&self, session_id: &str) -> Result<Vec<Artifact>> {
        self.db.with(|c| c.prepare(&format!("{ARTIFACT} WHERE a.session_id = ?1 ORDER BY a.created_at, a.rowid"))?.query_map([session_id], artifact_from)?.collect())
    }

    pub fn artifact(&self, id: &str) -> Result<Artifact> {
        self.db
            .with(|c| c.prepare(&format!("{ARTIFACT} WHERE a.id = ?1"))?.query_map([id], artifact_from)?.next().transpose())?
            .ok_or_else(|| Error::NotFound(format!("There is no artifact \"{id}\".")))
    }

    /// The artifact of that name in a session.
    pub fn artifact_named(&self, session_id: &str, name: &str) -> Result<Option<Artifact>> {
        let Ok(name) = name_of(name) else { return Ok(None) };
        let id: Option<String> = self.db.with(|c| c.query_row("SELECT id FROM artifacts WHERE session_id = ?1 AND name = ?2", rusqlite::params![session_id, name], |r| r.get(0)).optional())?;
        id.map(|id| self.artifact(&id)).transpose()
    }

    /// Every version of an artifact, without their text, the first first.
    pub fn artifact_versions(&self, id: &str) -> Result<Vec<VersionFacts>> {
        self.artifact(id)?;
        self.db.with(|c| {
            c.prepare("SELECT n, length(CAST(text AS BLOB)), made_at, message_id FROM artifact_versions WHERE artifact_id = ?1 ORDER BY n")?
                .query_map([id], |r| Ok(VersionFacts { n: r.get(0)?, size: r.get(1)?, made_at: r.get(2)?, message_id: r.get(3)? }))?
                .collect()
        })
    }

    /// One version of an artifact, with its text; `None`: the last.
    pub fn artifact_version(&self, id: &str, n: Option<i64>) -> Result<Version> {
        self.artifact(id)?;
        let found = self.db.with(|c| {
            let read = |r: &rusqlite::Row| Ok(Version { n: r.get(0)?, text: r.get(1)?, made_at: r.get(2)?, message_id: r.get(3)? });
            match n {
                Some(n) => c.query_row("SELECT n, text, made_at, message_id FROM artifact_versions WHERE artifact_id = ?1 AND n = ?2", rusqlite::params![id, n], read).optional(),
                None => c.query_row("SELECT n, text, made_at, message_id FROM artifact_versions WHERE artifact_id = ?1 ORDER BY n DESC LIMIT 1", [id], read).optional(),
            }
        })?;
        found.ok_or_else(|| Error::NotFound(format!("The artifact has no version {}.", n.unwrap_or(0))))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::db::Db;

    fn sessions() -> (Sessions, String) {
        let s = Sessions::new(Arc::new(Db::memory().unwrap()));
        let id = s.create(Some("Bakery"), "small").unwrap().id;
        (s, id)
    }

    #[test]
    fn the_same_name_again_is_a_new_version_and_every_version_is_kept() {
        let (s, session) = sessions();
        let made = s.make_artifact(&session, "page.html", "<h1>Sol</h1>", Some("m1")).unwrap();
        assert_eq!((made.n, made.same, made.artifact.versions, made.artifact.shown_as.as_str()), (1, false, 1, "html"));
        let again = s.make_artifact(&session, " page.html ", "<h1>Sol, open 8 to 14</h1>", Some("m2")).unwrap();
        assert_eq!((again.artifact.id.as_str(), again.n, again.artifact.versions), (made.artifact.id.as_str(), 2, 2));
        // The same text adds nothing.
        let same = s.make_artifact(&session, "page.html", "<h1>Sol, open 8 to 14</h1>", Some("m3")).unwrap();
        assert_eq!((same.n, same.same, same.artifact.versions), (2, true, 2));

        assert_eq!(s.artifact_version(&made.artifact.id, Some(1)).unwrap().text, "<h1>Sol</h1>");
        assert_eq!(
            s.artifact_version(&made.artifact.id, None).unwrap(),
            Version { n: 2, text: "<h1>Sol, open 8 to 14</h1>".into(), made_at: again.artifact.updated_at, message_id: Some("m2".into()) }
        );
        let versions = s.artifact_versions(&made.artifact.id).unwrap();
        assert_eq!(versions.iter().map(|v| (v.n, v.size)).collect::<Vec<_>>(), [(1, 12), (2, 26)]);
        assert!(s.artifact_version(&made.artifact.id, Some(3)).is_err());

        // Another name is another artifact; a name without an ending is Markdown.
        let letter = s.make_artifact(&session, "Cover letter", "Dear team,", None).unwrap();
        assert_eq!((letter.artifact.name.as_str(), letter.artifact.shown_as.as_str()), ("Cover letter.md", "markdown"));
        assert_eq!(s.artifacts(&session).unwrap().iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["page.html", "Cover letter.md"]);
        assert_eq!(s.artifact_named(&session, "Cover letter").unwrap().map(|a| a.id), Some(letter.artifact.id));
        assert_eq!(s.get(&session).unwrap().artifacts, 2, "the session counts them");
    }

    #[test]
    fn an_artifact_is_text_with_a_name_of_its_own_and_goes_with_its_session() {
        let (s, session) = sessions();
        for (name, text) in [("", "x"), ("../etc/passwd", "x"), ("notes/plan.md", "x"), ("photo.png", "x"), ("plan.md", "  \n")] {
            assert!(matches!(s.make_artifact(&session, name, text, None), Err(Error::Invalid(_))), "{name:?}");
        }
        assert!(s.make_artifact(&session, "big.txt", &"a".repeat(MOST_TEXT + 1), None).is_err());
        assert!(s.make_artifact("no-such-session", "plan.md", "x", None).is_err());
        assert_eq!(s.make_artifact(&session, "drawing.svg", "<svg/>", None).unwrap().artifact.shown_as, "svg");
        assert_eq!(s.make_artifact(&session, "rename.py", "print(1)", None).unwrap().artifact.shown_as, "text");
        let made = s.make_artifact(&session, "plan.md", "# Plan", None).unwrap();
        s.delete(&session).unwrap();
        assert!(s.artifact(&made.artifact.id).is_err(), "gone with its session");
    }
}
