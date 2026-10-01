//! Vectors of attached files' passages, for searching them by meaning (see
//! `attachments::meaning`), and the BM25 scores they are weighed against.

use std::collections::HashMap;

use rusqlite::{params, params_from_iter, types::Value};

use super::attachments::row_to_passage;
use super::{Db, Passage};

/// Passages' rows with their vectors.
pub type Vectors = Vec<(i64, Vec<f32>)>;

/// `?first, ?first+1, …` for a list of `n` values.
fn placeholders(first: usize, n: usize) -> String {
    (first..first + n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

impl Db {
    /// Up to `limit` passages without a vector of `model`, the newest file's first, each file's
    /// in order: a file just attached is searchable by meaning before the ones behind it.
    pub fn unindexed_passages(&self, model: &str, limit: usize) -> rusqlite::Result<Vec<(i64, String)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT p.id, p.text FROM attachment_passages p JOIN attachments a ON a.id = p.attachment_id
                 WHERE NOT EXISTS (SELECT 1 FROM passage_vectors v WHERE v.passage_id = p.id AND v.model = ?1)
                 ORDER BY a.created_at DESC, a.id, p.seq LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![model, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    /// Stores vectors of `model`. A passage whose file was removed meanwhile is skipped.
    pub fn store_vectors(&self, model: &str, vectors: &[(i64, Vec<f32>)]) -> rusqlite::Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO passage_vectors (passage_id, model, vector)
                     SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM attachment_passages WHERE id = ?1)",
                )?;
                for (id, vector) in vectors {
                    stmt.execute(params![id, model, to_blob(vector)])?;
                }
            }
            tx.commit()
        })
    }

    /// Passages with a vector of `model`, and passages in all.
    pub fn vector_progress(&self, model: &str) -> rusqlite::Result<(u64, u64)> {
        self.with(|c| {
            c.query_row(
                "SELECT (SELECT COUNT(*) FROM passage_vectors WHERE model = ?1),
                        (SELECT COUNT(*) FROM attachment_passages)",
                [model],
                |r| Ok((r.get::<_, i64>(0)?.max(0) as u64, r.get::<_, i64>(1)?.max(0) as u64)),
            )
        })
    }

    /// How many passages of the files `ids` have no vector of `model` yet.
    pub fn unindexed_count(&self, ids: &[String], model: &str) -> rusqlite::Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let sql = format!(
            "SELECT COUNT(*) FROM attachment_passages p WHERE p.attachment_id IN ({})
             AND NOT EXISTS (SELECT 1 FROM passage_vectors v WHERE v.passage_id = p.id AND v.model = ?1)",
            placeholders(2, ids.len())
        );
        let mut values: Vec<Value> = vec![model.to_string().into()];
        values.extend(ids.iter().map(|id| Value::from(id.clone())));
        self.with(|c| c.query_row(&sql, params_from_iter(values), |r| r.get::<_, i64>(0)))
            .map(|n| n.max(0) as u64)
    }

    /// The vector of `model` of every passage of the files `ids`, by passage row, or `None` while
    /// any of them has none: passages without one cannot be ranked against those with one.
    pub fn passage_vectors(&self, ids: &[String], model: &str) -> rusqlite::Result<Option<Vectors>> {
        if ids.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let sql = format!(
            "SELECT p.id, v.vector FROM attachment_passages p
             LEFT JOIN passage_vectors v ON v.passage_id = p.id AND v.model = ?1
             WHERE p.attachment_id IN ({})",
            placeholders(2, ids.len())
        );
        let mut values: Vec<Value> = vec![model.to_string().into()];
        values.extend(ids.iter().map(|id| Value::from(id.clone())));
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(values))?;
            let mut out = Vec::new();
            while let Some(r) = rows.next()? {
                let Some(blob) = r.get::<_, Option<Vec<u8>>>(1)? else {
                    return Ok(None);
                };
                out.push((r.get(0)?, from_blob(&blob)));
            }
            Ok(Some(out))
        })
    }

    /// The BM25 scores (higher is better) of the passages of the files `ids` that match the FTS5
    /// `query`, the best `limit`.
    pub fn lexical_scores(&self, ids: &[String], query: &str, limit: usize) -> rusqlite::Result<HashMap<i64, f64>> {
        if ids.is_empty() || query.trim().is_empty() {
            return Ok(HashMap::new());
        }
        let sql = format!(
            "SELECT p.id, -bm25(passages_fts) FROM passages_fts JOIN attachment_passages p ON p.id = passages_fts.rowid
             WHERE passages_fts MATCH ?1 AND p.attachment_id IN ({}) ORDER BY rank LIMIT ?2",
            placeholders(3, ids.len())
        );
        let mut values: Vec<Value> = vec![query.to_string().into(), (limit as i64).into()];
        values.extend(ids.iter().map(|id| Value::from(id.clone())));
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(values), |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    /// Passages by their rows, in the order given.
    pub fn passages_by_rowid(&self, rowids: &[i64]) -> rusqlite::Result<Vec<Passage>> {
        if rowids.is_empty() {
            return Ok(Vec::new());
        }
        let list = rowids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let sql = format!("SELECT attachment_id, seq, page, text, id FROM attachment_passages WHERE id IN ({list})");
        let found: HashMap<i64, Passage> = self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([], row_to_passage)?;
            rows.map(|r| r.map(|p| (p.rowid, p))).collect()
        })?;
        Ok(rowids.iter().filter_map(|id| found.get(id).cloned()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Attachment, AttachmentContent, new_id, now_ms};
    use demido_extract::{Chunk, Kind};

    fn attach(db: &Db, name: &str, created_at: i64, texts: &[&str]) -> String {
        let a = Attachment {
            id: new_id(),
            chat_id: None,
            message_id: None,
            name: name.into(),
            stored: format!("x/{name}"),
            file: None,
            path: String::new(),
            mime: "text/plain".into(),
            kind: Kind::Text,
            size: 10,
            pages: None,
            tokens: Some(40),
            width: None,
            height: None,
            note: None,
            created_at,
        };
        let chunks: Vec<Chunk> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| Chunk {
                seq: i as u32,
                page: None,
                text: t.to_string(),
            })
            .collect();
        db.insert_attachment(
            &a,
            AttachmentContent {
                text: Some("whole"),
                media: None,
                chunks: &chunks,
            },
        )
        .unwrap();
        a.id
    }

    #[test]
    fn passages_are_indexed_newest_file_first_and_once_per_model() {
        let db = Db::in_memory();
        let old = attach(&db, "old.txt", now_ms() - 1000, &["old one", "old two"]);
        let new = attach(&db, "new.txt", now_ms(), &["new one", "new two"]);
        let pending = db.unindexed_passages("m", 10).unwrap();
        let texts: Vec<&str> = pending.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(texts, ["new one", "new two", "old one", "old two"]);
        assert_eq!(db.unindexed_count(std::slice::from_ref(&new), "m").unwrap(), 2);
        assert_eq!(db.passage_vectors(std::slice::from_ref(&new), "m").unwrap(), None);

        let vectors: Vec<(i64, Vec<f32>)> = pending[..2].iter().map(|(id, _)| (*id, vec![0.5, -0.25])).collect();
        db.store_vectors("m", &vectors).unwrap();
        assert_eq!(db.unindexed_count(std::slice::from_ref(&new), "m").unwrap(), 0);
        assert_eq!(db.unindexed_count(&[new.clone(), old.clone()], "m").unwrap(), 2);
        assert_eq!(db.vector_progress("m").unwrap(), (2, 4));
        let stored = db.passage_vectors(std::slice::from_ref(&new), "m").unwrap().unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored.iter().all(|(_, v)| v == &[0.5, -0.25]));
        // Another model's vectors are its own.
        assert_eq!(db.unindexed_passages("other", 10).unwrap().len(), 4);
        assert_eq!(db.passage_vectors(&[], "m").unwrap(), Some(Vec::new()));
    }

    #[test]
    fn vectors_go_with_their_file() {
        let db = Db::in_memory();
        let id = attach(&db, "a.txt", now_ms(), &["alpha"]);
        let (rowid, _) = db.unindexed_passages("m", 1).unwrap().remove(0);
        db.delete_staged_attachment(&id).unwrap();
        // Stored after its file went: skipped, not an error.
        db.store_vectors("m", &[(rowid, vec![1.0])]).unwrap();
        assert_eq!(db.vector_progress("m").unwrap(), (0, 0));
    }

    #[test]
    fn words_are_scored_and_passages_found_by_row() {
        let db = Db::in_memory();
        let id = attach(
            &db,
            "a.txt",
            now_ms(),
            &[
                "the notice period is thirty days",
                "payment terms",
                "notice notice notice period",
            ],
        );
        let scores = db
            .lexical_scores(std::slice::from_ref(&id), "\"notice\" OR \"period\"", 10)
            .unwrap();
        assert_eq!(scores.len(), 2);
        assert!(scores.values().all(|s| *s > 0.0));
        let mut rows: Vec<i64> = scores.keys().copied().collect();
        rows.sort();
        rows.reverse();
        let passages = db.passages_by_rowid(&rows).unwrap();
        assert_eq!(passages.iter().map(|p| p.seq).collect::<Vec<_>>(), [2, 0]);
    }
}
