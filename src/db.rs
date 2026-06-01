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
    pub created: Option<String>,   // populated by get_node only
    pub visited: Option<String>,   // populated by get_node only
}

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
         CREATE TABLE IF NOT EXISTS nodes (
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
                created:  None,  // not fetched here — use get_node when needed
                visited:  None,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(nodes)
}

pub fn get_node(conn: &Connection, id: i64) -> Result<Node> {
    // columns:  0    1       2     3       4    5     6         7        8
    conn.query_row(
        "SELECT id, parent, kind, title, url, note, sort_idx, created, visited
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
            created:  row.get(7)?,
            visited:  row.get(8)?,
        }),
    )
}

pub fn insert_node(
    conn: &Connection,
    parent: Option<i64>,
    kind: &str,
    title: &str,
    url: Option<&str>,
    note: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO nodes (parent, kind, title, url, note) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![parent, kind, title, url, note],
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

pub fn touch_visited(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE nodes SET visited = datetime('now') WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn get_bookmarks_recursive(conn: &Connection, folder_id: i64) -> Result<Vec<Node>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE subtree(id) AS (
             SELECT id FROM nodes WHERE parent = ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN subtree s ON n.parent = s.id
         )
         SELECT id, parent, kind, title, url, note, sort_idx
         FROM nodes
         WHERE id IN (SELECT id FROM subtree) AND kind = 'bookmark'",
    )?;
    let nodes = stmt
        .query_map([folder_id], |row| {
            Ok(Node {
                id:       row.get(0)?,
                parent:   row.get(1)?,
                kind:     row.get(2)?,
                title:    row.get(3)?,
                url:      row.get(4)?,
                note:     row.get(5)?,
                sort_idx: row.get(6)?,
                created:  None,
                visited:  None,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(nodes)
}

pub fn get_favicons(conn: &Connection) -> std::collections::HashMap<i64, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, favicon FROM nodes WHERE kind='bookmark' AND favicon IS NOT NULL AND favicon != ''",
    ) {
        let _ = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map(|rows| {
                for row in rows.flatten() {
                    map.insert(row.0, row.1);
                }
            });
    }
    map
}

pub fn set_favicon(conn: &Connection, id: i64, filename: &str) -> Result<()> {
    conn.execute(
        "UPDATE nodes SET favicon = ?1 WHERE id = ?2",
        params![if filename.is_empty() { None::<&str> } else { Some(filename) }, id],
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
        let id = insert_node(&conn, None, "folder", "Test Folder", None, None).unwrap();
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
        let parent_id = insert_node(&conn, None, "folder", "Parent", None, None).unwrap();
        let child_id = insert_node(&conn, Some(parent_id), "folder", "Child", None, None).unwrap();

        let root = get_children(&conn, None).unwrap();
        assert_eq!(root.len(), 1, "root must contain only the parent");

        let children = get_children(&conn, Some(parent_id)).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child_id);
    }

    #[test]
    fn delete_removes_node_and_all_descendants() {
        let conn = test_db();
        let parent_id = insert_node(&conn, None, "folder", "Parent", None, None).unwrap();
        insert_node(&conn, Some(parent_id), "bookmark", "Link", Some("https://example.com"), None).unwrap();
        insert_node(&conn, Some(parent_id), "folder", "Sub", None, None).unwrap();

        delete_node(&conn, parent_id).unwrap();

        let all = get_children(&conn, None).unwrap();
        assert_eq!(all.len(), 0, "parent and all descendants must be gone");
    }

    #[test]
    fn insert_bookmark_stores_url() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Example", Some("https://example.com"), None).unwrap();
        let items = get_children(&conn, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn get_node_returns_correct_node() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Test", Some("https://test.com"), None).unwrap();
        let node = get_node(&conn, id).unwrap();
        assert_eq!(node.id, id);
        assert_eq!(node.title, "Test");
        assert_eq!(node.url.as_deref(), Some("https://test.com"));
        assert_eq!(node.kind, "bookmark");
        assert_eq!(node.note, None);
        // created is populated from DB (datetime('now'))
        assert!(node.created.is_some());
        assert_eq!(node.visited, None);
    }

    #[test]
    fn insert_with_note_stores_it() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Title", Some("https://x.com"), Some("my note")).unwrap();
        let node = get_node(&conn, id).unwrap();
        assert_eq!(node.note.as_deref(), Some("my note"));
        assert_eq!(node.title, "Title");
    }

    #[test]
    fn touch_visited_sets_visited_date() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Test", Some("https://test.com"), None).unwrap();
        assert_eq!(get_node(&conn, id).unwrap().visited, None, "visited должен быть NULL после вставки");
        touch_visited(&conn, id).unwrap();
        assert!(get_node(&conn, id).unwrap().visited.is_some(), "visited должен быть заполнен после touch_visited");
    }

    #[test]
    fn update_changes_title_url_note() {
        let conn = test_db();
        let id = insert_node(&conn, None, "bookmark", "Old Title", Some("https://old.com"), None).unwrap();
        update_node(&conn, id, "New Title", Some("https://new.com"), Some("my note")).unwrap();
        let node = get_node(&conn, id).unwrap();
        assert_eq!(node.title, "New Title");
        assert_eq!(node.url.as_deref(), Some("https://new.com"));
        assert_eq!(node.note.as_deref(), Some("my note"));
    }

    #[test]
    fn get_bookmarks_recursive_finds_all_depths() {
        let conn = test_db();
        let root   = insert_node(&conn, None,        "folder",   "Root",   None,                          None).unwrap();
        let sub    = insert_node(&conn, Some(root),  "folder",   "Sub",    None,                          None).unwrap();
        let bm1    = insert_node(&conn, Some(root),  "bookmark", "BM1",    Some("https://a.com"), None).unwrap();
        let bm2    = insert_node(&conn, Some(sub),   "bookmark", "BM2",    Some("https://b.com"), None).unwrap();
        let _deep  = insert_node(&conn, Some(sub),   "folder",   "Deep",   None,                          None).unwrap();
        let deep2  = insert_node(&conn, Some(_deep), "bookmark", "BM3",    Some("https://c.com"), None).unwrap();

        let bms = get_bookmarks_recursive(&conn, root).unwrap();
        let ids: Vec<i64> = bms.iter().map(|n| n.id).collect();
        assert!(ids.contains(&bm1), "direct child bookmark");
        assert!(ids.contains(&bm2), "bookmark in sub-folder");
        assert!(ids.contains(&deep2), "bookmark at depth 3");
        assert_eq!(bms.len(), 3, "only bookmarks, no folders");
    }

    #[test]
    fn set_favicon_and_get_favicons_roundtrip() {
        let conn = test_db();
        let id1 = insert_node(&conn, None, "bookmark", "A", Some("https://a.com"), None).unwrap();
        let id2 = insert_node(&conn, None, "bookmark", "B", Some("https://b.com"), None).unwrap();
        insert_node(&conn, None, "folder", "F", None, None).unwrap(); // folder — should NOT appear

        set_favicon(&conn, id1, "a.com.png").unwrap();
        set_favicon(&conn, id2, "b.com.png").unwrap();

        let map = get_favicons(&conn);
        assert_eq!(map.get(&id1).map(|s| s.as_str()), Some("a.com.png"));
        assert_eq!(map.get(&id2).map(|s| s.as_str()), Some("b.com.png"));
        assert_eq!(map.len(), 2, "no folder entries");
    }

    #[test]
    fn get_favicons_excludes_empty_and_null() {
        let conn = test_db();
        let id1 = insert_node(&conn, None, "bookmark", "A", Some("https://a.com"), None).unwrap();
        let id2 = insert_node(&conn, None, "bookmark", "B", Some("https://b.com"), None).unwrap();
        set_favicon(&conn, id1, "a.com.png").unwrap();
        // id2 has NULL favicon — should NOT appear in map

        let map = get_favicons(&conn);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&id1));
        assert!(!map.contains_key(&id2));
    }
}
