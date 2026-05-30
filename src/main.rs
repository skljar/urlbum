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
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let db_path = exe_dir.join("album.db");

    let state = Rc::new(RefCell::new(State {
        db: db::open(db_path.to_str().unwrap()).expect("cannot open album.db"),
        selected_folder: None,
        expanded: HashSet::new(),
    }));

    let window = AppWindow::new().unwrap();

    {
        let s = state.borrow();
        refresh_folders(&s, &window);
        refresh_bookmarks(&s, &window);
    }

    // Folder clicked: toggle expand/collapse and select
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

    // New folder under selected folder (or at root if nothing selected)
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_new_folder(move || {
        let mut s = sc.borrow_mut();
        let parent = s.selected_folder;
        if let Ok(new_id) = db::insert_node(&s.db, parent, "folder", "New Folder", None) {
            if let Some(p) = parent {
                s.expanded.insert(p); // auto-expand parent so child is visible
            }
            s.selected_folder = Some(new_id);
        }
        let w = ww.upgrade().unwrap();
        w.set_selected_folder_id(s.selected_folder.unwrap_or(-1) as i32);
        refresh_folders(&s, &w);
        refresh_bookmarks(&s, &w);
    });

    // New bookmark under selected folder (or at root if nothing selected)
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_new_bookmark(move || {
        let s = sc.borrow();
        let _ = db::insert_node(&s.db, s.selected_folder, "bookmark", "New Bookmark", Some("https://"));
        let w = ww.upgrade().unwrap();
        refresh_bookmarks(&s, &w);
    });

    // Delete selected folder and all its descendants
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
