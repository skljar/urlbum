use rusqlite::{Connection, Result, params};

#[derive(Debug, Clone)]
pub struct Node {
    pub id: i64,
    pub parent: Option<i64>,
    pub kind: String,
    pub title: String,
    pub url: Option<String>,
    pub note: Option<String>,
    pub sort_idx: i64,
}

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS nodes (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            parent   INTEGER,
            kind     TEXT NOT NULL DEFAULT 'bookmark',
            title    TEXT NOT NULL,
            url      TEXT,
            thumb    TEXT,
            note     TEXT,
            created  TEXT DEFAULT (datetime('now')),
            visited  TEXT,
            sort_idx INTEGER DEFAULT 0,
            favicon  TEXT
        );",
    )?;
    Ok(conn)
}

pub fn get_children(conn: &Connection, parent: Option<i64>) -> Result<Vec<Node>> {
    let mut stmt = conn.prepare(
        // columns:  0    1       2     3       4    5     6
        "SELECT id, parent, kind, title, url, note, sort_idx
         FROM nodes WHERE parent IS ?1
         ORDER BY sort_idx, id",
    )?;
    let nodes = stmt
        .query_map([parent], |row| {
            Ok(Node {
                id:       row.get(0)?,
                parent:   row.get(1)?,
                kind:     row.get(2)?,
                title:    row.get(3)?,
                url:      row.get(4)?,
                note:     row.get(5)?,
                sort_idx: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(nodes)
}

pub fn get_node(conn: &Connection, id: i64) -> Result<Node> {
    // columns:  0    1       2     3       4    5     6
    conn.query_row(
        "SELECT id, parent, kind, title, url, note, sort_idx
         FROM nodes WHERE id = ?1",
        [id],
        |row| Ok(Node {
            id:       row.get(0)?,
            parent:   row.get(1)?,
            kind:     row.get(2)?,
            title:    row.get(3)?,
            url:      row.get(4)?,
            note:     row.get(5)?,
            sort_idx: row.get(6)?,
        }),
    )
}

pub fn insert_node(
    conn: &Connection,
    parent: Option<i64>,
    kind: &str,
    title: &str,
    url: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO nodes (parent, kind, title, url) VALUES (?1, ?2, ?3, ?4)",
        params![parent, kind, title, url],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_node(
    conn: &Connection,
    id: i64,
    title: &str,
    url: Option<&str>,
    note: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE nodes SET title=?1, url=?2, note=?3 WHERE id=?4",
        params![title, url, note, id],
    )?;
    Ok(())
}

pub fn delete_node(conn: &Connection, id: i64) -> Result<()> {
    // safe: id is i64, not user-supplied string
    conn.execute_batch(&format!(
        "WITH RECURSIVE sub(id) AS (
            SELECT {id}
            UNION ALL
            SELECT n.id FROM nodes n JOIN sub s ON n.parent = s.id
        )
        DELETE FROM nodes WHERE id IN (SELECT id FROM sub);"
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE nodes (
                id       INTEGER PRIMARY KEY AUTOINCREMENT,
                parent   INTEGER,
                kind     TEXT NOT NULL DEFAULT 'bookmark',
                title    TEXT NOT NULL,
                url      TEXT,
                thumb    TEXT,
                note     TEXT,
                created  TEXT DEFAULT (datetime('now')),
                visited  TEXT,
                sort_idx INTEGER DEFAULT 0,
                favicon  TEXT
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn insert_root_folder_and_get_it() {
        let conn = test_db();
        let id = insert_node(&conn, None, "folder", "Test Folder", None).unwrap();
        let children = get_children(&conn, None).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, id);
        assert_eq!(children[0].title, "Test Folder");
        assert_eq!(children[0].kind, "folder");
        assert_eq!(children[0].parent, None);
    }

    #[test]
    fn nested_folder_under_parent_not_root() {
        let conn = test_db();
        let parent_id = insert_node(&conn, None, "folder", "Parent", None).unwrap();
        let child_id = insert_node(&conn, Some(parent_id), "folder", "Child", None).unwrap();

        let root = get_children(&conn, None).unwrap();
        assert_eq!(root.len(), 1, "root must contain only the parent");

        let children = get_children(&conn, Some(parent_id)).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child_id);
    }

    #[test]
    fn delete_removes_node_and_all_descendants() {
        let conn = test_db();
        let parent_id = insert_node(&conn, None, "folder", "Parent", None).unwrap();
        insert_node(&conn, Some(parent_id), "bookmark", "Link", Some("https://example.com")).unwrap();
        insert_node(&conn, Some(parent_id), "folder", "Sub", None).unwrap();

        delete_node(&conn, parent_id).unwrap();

        let all = get_children(&conn, None).unwrap();
        assert_eq!(all.len(), 0, "parent and all descendants must be gone");
    }

    #[test]
    fn insert_bookmark_stores_url() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Example", Some("https://example.com")).unwrap();
        let items = get_children(&conn, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn get_node_returns_correct_node() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Test", Some("https://test.com")).unwrap();
        let node = get_node(&conn, id).unwrap();
        assert_eq!(node.id, id);
        assert_eq!(node.title, "Test");
        assert_eq!(node.url.as_deref(), Some("https://test.com"));
        assert_eq!(node.kind, "bookmark");
        assert_eq!(node.note, None);
    }

    #[test]
    fn update_changes_title_url_note() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Old Title", Some("https://old.com")).unwrap();
        update_node(&conn, id, "New Title", Some("https://new.com"), Some("my note")).unwrap();
        let node = get_node(&conn, id).unwrap();
        assert_eq!(node.title, "New Title");
        assert_eq!(node.url.as_deref(), Some("https://new.com"));
        assert_eq!(node.note.as_deref(), Some("my note"));
    }
}
