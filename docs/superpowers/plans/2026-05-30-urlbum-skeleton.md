# URLbum Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a minimal working Rust + Slint bookmark manager with a nested folder tree and bookmark list, backed by SQLite, compiling to a single portable exe on Windows 10/11.

**Architecture:** Single-window Slint UI — toolbar on top, folder tree (left, 250px) and bookmark list (right) below. Folders and bookmarks both live in one SQLite `nodes` table. The Rust side flattens the folder tree into a depth-annotated list for the Slint ListView. App state (selected folder, expanded set) is `Rc<RefCell<State>>` shared across UI callbacks.

**Tech Stack:** Rust stable, x86_64-pc-windows-msvc, crt-static · Slint 1.7 (winit backend, no custom platform) · rusqlite 0.32 (bundled) · slint-build 1.7

---

## File Map

| File | Role |
|------|------|
| `Cargo.toml` | dependencies |
| `.cargo/config.toml` | target + crt-static |
| `build.rs` | Slint codegen |
| `ui/main.slint` | window, structs, callbacks, layout |
| `src/db.rs` | `Node`, `open`, `get_children`, `insert_node`, `delete_node` |
| `src/main.rs` | `State`, `refresh_*`, callback wiring, `main()` |

---

### Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `.cargo/config.toml`
- Create: `build.rs`
- Create: `ui/main.slint` (stub — AppWindow only)
- Create: `src/db.rs` (stub)
- Create: `src/main.rs` (stub)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "urlbum"
version = "0.1.0"
edition = "2021"

[dependencies]
slint = "1.7"
rusqlite = { version = "0.32", features = ["bundled"] }

[build-dependencies]
slint-build = "1.7"
```

- [ ] **Step 2: Create .cargo/config.toml**

```toml
[build]
target = "x86_64-pc-windows-msvc"

[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

- [ ] **Step 3: Create build.rs**

```rust
fn main() {
    slint_build::compile("ui/main.slint").unwrap();
}
```

- [ ] **Step 4: Create stub ui/main.slint**

```slint
import { Button } from "std-widgets.slint";

export component AppWindow inherits Window {
    title: "URLbum";
    min-width: 800px;
    min-height: 500px;
}
```

- [ ] **Step 5: Create stub src/db.rs**

```rust
// placeholder — implemented in Task 2
```

- [ ] **Step 6: Create stub src/main.rs**

```rust
slint::include_modules!();
mod db;

fn main() {
    let window = AppWindow::new().unwrap();
    window.run().unwrap();
}
```

- [ ] **Step 7: Verify scaffold compiles**

```
cargo check
```

Expected: no errors (warnings about unused `db` module are fine).

- [ ] **Step 8: Init git and commit**

```
git init
git add Cargo.toml .cargo/config.toml build.rs ui/main.slint src/db.rs src/main.rs
git commit -m "chore: project scaffold"
```

---

### Task 2: Database Layer (TDD)

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Write failing tests — replace src/db.rs**

```rust
use rusqlite::{Connection, Result, params};

#[derive(Debug, Clone)]
pub struct Node {
    pub id: i64,
    pub parent: Option<i64>,
    pub kind: String,
    pub title: String,
    pub url: Option<String>,
    pub sort_idx: i64,
}

pub fn open(_path: &str) -> Result<Connection> {
    todo!()
}

pub fn get_children(_conn: &Connection, _parent: Option<i64>) -> Result<Vec<Node>> {
    todo!()
}

pub fn insert_node(
    _conn: &Connection,
    _parent: Option<i64>,
    _kind: &str,
    _title: &str,
    _url: Option<&str>,
) -> Result<i64> {
    todo!()
}

pub fn delete_node(_conn: &Connection, _id: i64) -> Result<()> {
    todo!()
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
}
```

- [ ] **Step 2: Run tests — verify they fail**

```
cargo test --lib 2>&1
```

Expected: 4 tests panic with `not yet implemented`.

- [ ] **Step 3: Implement the four functions — replace the stubs in src/db.rs**

Replace the four `todo!()` stubs (keep `Node` struct and `#[cfg(test)]` block unchanged):

```rust
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
        "SELECT id, parent, kind, title, url, sort_idx
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
                sort_idx: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(nodes)
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
```

- [ ] **Step 4: Run tests — verify they pass**

```
cargo test --lib 2>&1
```

Expected: `test result: ok. 4 passed; 0 failed`.

- [ ] **Step 5: Commit**

```
git add src/db.rs
git commit -m "feat: SQLite database layer with CRUD and recursive delete"
```

---

### Task 3: Slint UI

**Files:**
- Modify: `ui/main.slint`

- [ ] **Step 1: Replace ui/main.slint with full layout**

```slint
import { Button, ListView, HorizontalBox, VerticalBox } from "std-widgets.slint";

struct FolderItem {
    id: int,
    title: string,
    depth: int,
    expanded: bool,
}

struct BookmarkItem {
    id: int,
    title: string,
    url: string,
}

export component AppWindow inherits Window {
    title: "URLbum";
    min-width: 800px;
    min-height: 500px;

    in property <[FolderItem]>   folder-model;
    in property <[BookmarkItem]> bookmark-model;
    in-out property <int>        selected-folder-id: -1;

    callback new-folder();
    callback new-bookmark();
    callback delete-selected();
    callback folder-clicked(int);

    VerticalBox {
        // Toolbar
        HorizontalBox {
            height: 40px;
            Button { text: "New Folder";   clicked => { root.new-folder();      } }
            Button { text: "New Bookmark"; clicked => { root.new-bookmark();    } }
            Button { text: "Delete";       clicked => { root.delete-selected(); } }
            Rectangle { horizontal-stretch: 1; }
        }

        // Main area
        HorizontalBox {
            // Left panel — folder tree
            Rectangle {
                width: 250px;
                border-color: #cccccc;
                border-width: 1px;

                ListView {
                    for folder in root.folder-model: Rectangle {
                        height: 28px;
                        background: folder.id == root.selected-folder-id
                            ? #d0e4ff : transparent;

                        HorizontalLayout {
                            padding-left: folder.depth * 16px;
                            padding-top: 4px;
                            Text {
                                text: (folder.expanded ? "▼ " : "▶ ") + folder.title;
                                vertical-alignment: center;
                            }
                        }

                        TouchArea {
                            clicked => { root.folder-clicked(folder.id); }
                        }
                    }
                }
            }

            // Right panel — bookmark list
            Rectangle {
                border-color: #cccccc;
                border-width: 1px;

                ListView {
                    for bm in root.bookmark-model: Rectangle {
                        height: 28px;

                        HorizontalLayout {
                            padding-left: 8px;
                            padding-top: 4px;
                            spacing: 12px;
                            Text {
                                width: 200px;
                                text: bm.title;
                                vertical-alignment: center;
                            }
                            Text {
                                text: bm.url;
                                color: #666666;
                                vertical-alignment: center;
                            }
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: Verify Slint compiles**

```
cargo check 2>&1
```

Expected: no errors. (`main.rs` is still the stub so it only uses `AppWindow::new()` — that still compiles.)

- [ ] **Step 3: Commit**

```
git add ui/main.slint
git commit -m "feat: Slint UI — toolbar, nested folder tree, bookmark list"
```

---

### Task 4: Application Logic

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace src/main.rs with full implementation**

```rust
slint::include_modules!();

mod db;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use slint::VecModel;

struct State {
    db: rusqlite::Connection,
    selected_folder: Option<i64>,
    expanded: HashSet<i64>,
}

fn build_folder_list(
    conn: &rusqlite::Connection,
    parent: Option<i64>,
    depth: i32,
    expanded: &HashSet<i64>,
    out: &mut Vec<FolderItem>,
) {
    let Ok(nodes) = db::get_children(conn, parent) else { return };
    for node in nodes {
        if node.kind == "folder" {
            let is_exp = expanded.contains(&node.id);
            out.push(FolderItem {
                id: node.id as i32,
                title: node.title.clone().into(),
                depth,
                expanded: is_exp,
            });
            if is_exp {
                build_folder_list(conn, Some(node.id), depth + 1, expanded, out);
            }
        }
    }
}

fn refresh_folders(state: &State, window: &AppWindow) {
    let mut items: Vec<FolderItem> = Vec::new();
    build_folder_list(&state.db, None, 0, &state.expanded, &mut items);
    window.set_folder_model(Rc::new(VecModel::from(items)).into());
}

fn refresh_bookmarks(state: &State, window: &AppWindow) {
    let items: Vec<BookmarkItem> = match state.selected_folder {
        None => vec![],
        Some(folder_id) => db::get_children(&state.db, Some(folder_id))
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.kind == "bookmark")
            .map(|n| BookmarkItem {
                id: n.id as i32,
                title: n.title.clone().into(),
                url: n.url.clone().unwrap_or_default().into(),
            })
            .collect(),
    };
    window.set_bookmark_model(Rc::new(VecModel::from(items)).into());
}

fn main() {
    let state = Rc::new(RefCell::new(State {
        db: db::open("urlbum.db").expect("cannot open urlbum.db"),
        selected_folder: None,
        expanded: HashSet::new(),
    }));

    let window = AppWindow::new().unwrap();

    {
        let s = state.borrow();
        refresh_folders(&s, &window);
        refresh_bookmarks(&s, &window);
    }

    // Folder clicked: toggle expand and select
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_folder_clicked(move |id| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        if s.expanded.contains(&id64) {
            s.expanded.remove(&id64);
        } else {
            s.expanded.insert(id64);
        }
        s.selected_folder = Some(id64);
        let w = ww.upgrade().unwrap();
        w.set_selected_folder_id(id);
        refresh_folders(&s, &w);
        refresh_bookmarks(&s, &w);
    });

    // New folder under currently selected folder (or root if none)
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_new_folder(move || {
        let mut s = sc.borrow_mut();
        let parent = s.selected_folder;
        if let Some(new_id) = db::insert_node(&s.db, parent, "folder", "New Folder", None).ok() {
            // auto-expand parent so the new folder is visible
            if let Some(p) = parent {
                s.expanded.insert(p);
            }
            s.selected_folder = Some(new_id);
        }
        let w = ww.upgrade().unwrap();
        w.set_selected_folder_id(s.selected_folder.unwrap_or(-1) as i32);
        refresh_folders(&s, &w);
        refresh_bookmarks(&s, &w);
    });

    // New bookmark under currently selected folder (or root if none)
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_new_bookmark(move || {
        let s = sc.borrow();
        let _ = db::insert_node(&s.db, s.selected_folder, "bookmark", "New Bookmark", Some("https://"));
        let w = ww.upgrade().unwrap();
        refresh_bookmarks(&s, &w);
    });

    // Delete selected folder (and all its descendants)
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_delete_selected(move || {
        let mut s = sc.borrow_mut();
        if let Some(id) = s.selected_folder {
            let _ = db::delete_node(&s.db, id);
            s.selected_folder = None;
            s.expanded.remove(&id);
        }
        let w = ww.upgrade().unwrap();
        w.set_selected_folder_id(-1);
        refresh_folders(&s, &w);
        refresh_bookmarks(&s, &w);
    });

    window.run().unwrap();
}
```

- [ ] **Step 2: Build debug**

```
cargo build 2>&1
```

Expected: `Finished dev [unoptimized + debuginfo] target(s)` — no errors.

- [ ] **Step 3: Commit**

```
git add src/main.rs
git commit -m "feat: app state and UI callbacks wired to CRUD"
```

---

### Task 5: Final Verification

- [ ] **Step 1: Run all tests**

```
cargo test 2>&1
```

Expected: `test result: ok. 4 passed; 0 failed`.

- [ ] **Step 2: Release build**

```
cargo build --release 2>&1
```

Expected: `Finished release [optimized] target(s)`.
Binary: `target\x86_64-pc-windows-msvc\release\urlbum.exe`

- [ ] **Step 3: Verify no CRT dependency**

```
dumpbin /dependents target\x86_64-pc-windows-msvc\release\urlbum.exe
```

Expected output contains only Windows system DLLs. Must NOT contain `VCRUNTIME140.dll` or `MSVCP140.dll`.

- [ ] **Step 4: Smoke test** — run the exe

```
target\x86_64-pc-windows-msvc\release\urlbum.exe
```

Verify:
- Window opens titled "URLbum" with 3 toolbar buttons
- Click "New Folder" → "New Folder" appears in left panel
- Click that folder → it highlights + shows ▼, right panel is empty
- Click "New Bookmark" → "New Bookmark" appears in right panel with "https://"
- Click "New Folder" again → child folder appears nested under selected, parent auto-expands
- Select root folder, click "Delete" → folder + its bookmark disappear
- Close window → `urlbum.db` exists in working directory

- [ ] **Step 5: Final commit**

```
git add -A
git commit -m "feat: URLbum skeleton complete — folder tree and bookmark CRUD working"
```
