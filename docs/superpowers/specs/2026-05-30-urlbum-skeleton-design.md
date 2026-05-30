---
name: urlbum-skeleton-design
description: URLbum skeleton — Rust + Slint bookmark manager, phase 1 scaffold
metadata:
  type: project
---

# URLbum Skeleton — Design Spec

## Goal

Minimal working Rust + Slint bookmark manager: create/delete folders and bookmarks, navigate folder tree. No favicon, no DnD, no import — scaffold only.

## Tech Stack

- **Rust** — stable, x86_64-pc-windows-msvc
- **Slint 1.7** — winit backend (default), NO custom platform.rs
- **rusqlite 0.32** — `bundled` feature → single portable exe
- **crt-static** — via `.cargo/config.toml` rustflags

## File Structure

```
urlbum/
├── .cargo/config.toml      # target + crt-static
├── src/
│   ├── main.rs             # State, UI callbacks, event loop
│   └── db.rs               # SQLite CRUD functions
├── ui/
│   └── main.slint          # Slint UI definition
├── build.rs                # slint_build::compile("ui/main.slint")
└── Cargo.toml
```

## Database Schema (single table)

```sql
CREATE TABLE nodes (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    parent   INTEGER,                              -- NULL = root
    kind     TEXT NOT NULL DEFAULT 'bookmark',     -- 'folder' | 'bookmark'
    title    TEXT NOT NULL,
    url      TEXT,
    thumb    TEXT,
    note     TEXT,
    created  TEXT DEFAULT (datetime('now')),
    visited  TEXT,
    sort_idx INTEGER DEFAULT 0,
    favicon  TEXT
);
```

All future fields (thumb, note, favicon, sort_idx) are included now to avoid migrations later.

## Application State

```rust
struct State {
    db: Connection,
    selected_folder: Option<i64>,   // currently open folder
    expanded: HashSet<i64>,         // expanded folder ids in tree
}
```

## UI Layout

```
┌─ toolbar ─────────────────────────────────────────┐
│  [New Folder]  [New Bookmark]  [Delete]            │
├─ left panel ──────────┬─ right panel ─────────────┤
│  Folder tree          │  Bookmark list             │
│  (ListView)           │  title + url               │
│  indent by depth      │  (ListView)                │
└───────────────────────┴────────────────────────────┘
```

- Left panel: recursive folder tree, click expands/collapses, click selects folder
- Right panel: shows bookmarks (kind='bookmark') whose parent = selected_folder
- Toolbar: three buttons trigger Rust callbacks

## CRUD Operations (Phase 1)

- `db::get_children(db, parent_id)` → `Vec<Node>` ordered by sort_idx
- `db::insert_node(db, parent, kind, title, url)` → `i64`
- `db::delete_node(db, id)` — also deletes all descendants (recursive CTE)

## Explicit Non-Scope (Phase 1)

Favicon, thumbnail, import/export, context menus, drag & drop, settings, search.
