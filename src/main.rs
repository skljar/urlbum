slint::include_modules!();

mod db;

extern crate webbrowser;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use slint::VecModel;

struct State {
    db: rusqlite::Connection,
    selected_id:        Option<i64>,   // подсвечен в дереве
    selected_is_folder: bool,
    current_folder:     Option<i64>,   // папка, чьё содержимое в правом списке
    status_id:          Option<i64>,   // строка, выделенная в списке (статусбар)
    expanded:           HashSet<i64>,
}

// ─── Обновление левого дерева ────────────────────────────────────────────────

fn build_tree_list(
    conn: &rusqlite::Connection,
    parent: Option<i64>,
    depth: i32,
    expanded: &HashSet<i64>,
    out: &mut Vec<TreeItem>,
) {
    let Ok(nodes) = db::get_children(conn, parent) else { return };
    for node in nodes {
        let is_folder = node.kind == "folder";
        let is_exp = is_folder && expanded.contains(&node.id);
        out.push(TreeItem {
            id:        node.id as i32,
            title:     node.title.clone().into(),
            depth,
            expanded:  is_exp,
            is_folder,
            url:       node.url.clone().unwrap_or_default().into(),
        });
        if is_exp {
            build_tree_list(conn, Some(node.id), depth + 1, expanded, out);
        }
    }
}

fn refresh_tree(state: &State, window: &AppWindow) {
    let mut items: Vec<TreeItem> = Vec::new();
    build_tree_list(&state.db, None, 0, &state.expanded, &mut items);
    window.set_tree_model(Rc::new(VecModel::from(items)).into());
}

// ─── Обновление правого списка ───────────────────────────────────────────────

fn refresh_contents(state: &State, window: &AppWindow) {
    let items: Vec<TreeItem> = match state.current_folder {
        None => vec![],
        Some(folder_id) => db::get_children(&state.db, Some(folder_id))
            .unwrap_or_default()
            .into_iter()
            .map(|n| TreeItem {
                id:        n.id as i32,
                title:     n.title.clone().into(),
                depth:     0,
                expanded:  false,
                is_folder: n.kind == "folder",
                url:       n.url.clone().unwrap_or_default().into(),
            })
            .collect(),
    };
    window.set_contents_model(Rc::new(VecModel::from(items)).into());
}

// ─── Заполнение статусбара ───────────────────────────────────────────────────

fn show_statusbar(id: i64, db: &rusqlite::Connection, window: &AppWindow) {
    window.set_status_id(id as i32);
    if let Ok(node) = db::get_node(db, id) {
        window.set_status_url(node.url.unwrap_or_default().into());
        window.set_status_note(node.note.unwrap_or_default().into());
        window.set_status_created(node.created.unwrap_or_default().into());
        window.set_status_visited(node.visited.unwrap_or_else(|| "—".into()).into());
    }
    window.set_status_visible(true);
}

// ─── main ────────────────────────────────────────────────────────────────────

fn main() {
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let db_path = exe_dir.join("album.db");

    let state = Rc::new(RefCell::new(State {
        db: db::open(db_path.to_str().unwrap()).expect("cannot open album.db"),
        selected_id:        None,
        selected_is_folder: true,
        current_folder:     None,
        status_id:          None,
        expanded:           HashSet::new(),
    }));

    let window = AppWindow::new().unwrap();

    {
        let s = state.borrow();
        refresh_tree(&s, &window);
        refresh_contents(&s, &window);
    }

    // ── Клик по узлу в левом дереве ──────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_tree_item_clicked(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();

        if is_folder {
            // Папка: toggle expand + navigate + сбросить статусбар
            if s.expanded.contains(&id64) {
                s.expanded.remove(&id64);
            } else {
                s.expanded.insert(id64);
            }
            s.selected_id        = Some(id64);
            s.selected_is_folder = true;
            s.current_folder     = Some(id64);
            s.status_id          = None;
            w.set_selected_id(id);
            w.set_status_id(-1);
            w.set_status_visible(false);
            w.set_show_card(false);
            refresh_tree(&s, &w);
            refresh_contents(&s, &w);
        } else {
            // Ссылка: показать карточку справа + подсветить в дереве
            s.selected_id        = Some(id64);
            s.selected_is_folder = false;
            if let Ok(node) = db::get_node(&s.db, id64) {
                w.set_card_id(id);
                w.set_card_title(node.title.into());
                w.set_card_url(node.url.unwrap_or_default().into());
            }
            w.set_selected_id(id);
            w.set_show_card(true);
            w.set_status_visible(false);
        }
    });

    // ── Одиночный клик в правом списке ───────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_list_item_clicked(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();

        if is_folder {
            // Папка: только подсветить строку, НЕ навигировать, НЕ менять current_folder
            s.status_id = Some(id64);
            w.set_status_id(id);          // реактивная подсветка через Slint
            w.set_status_visible(false);  // статусбар для папок не показываем
        } else {
            // Ссылка: highlight + статусбар, БЕЗ refresh_contents
            s.status_id = Some(id64);
            show_statusbar(id64, &s.db, &w);  // set_status_id реактивно подсвечивает строку
        }
    });

    // ── Двойной клик в правом списке ─────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_list_item_activated(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();

        if is_folder {
            // Папка: navigate (то же что одиночный клик)
            if !s.expanded.contains(&id64) {
                s.expanded.insert(id64);
            }
            s.selected_id        = Some(id64);
            s.selected_is_folder = true;
            s.current_folder     = Some(id64);
            s.status_id          = None;
            w.set_selected_id(id);
            w.set_status_id(-1);
            w.set_status_visible(false);
            w.set_show_card(false);
            refresh_tree(&s, &w);
            refresh_contents(&s, &w);
        } else {
            // Ссылка: карточка
            s.selected_id        = Some(id64);
            s.selected_is_folder = false;
            if let Ok(node) = db::get_node(&s.db, id64) {
                w.set_card_id(id);
                w.set_card_title(node.title.into());
                w.set_card_url(node.url.unwrap_or_default().into());
            }
            w.set_selected_id(id);
            w.set_show_card(true);
            w.set_status_visible(false);
            // set_selected_id реактивно обновит подсветку в дереве — refresh не нужен
        }
    });

    // ── Toggle expand по значку ▶/▼ ──────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_tree_toggle_expand(move |id| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        if s.expanded.contains(&id64) {
            s.expanded.remove(&id64);
        } else {
            s.expanded.insert(id64);
        }
        let w = ww.upgrade().unwrap();
        refresh_tree(&s, &w);   // только дерево — current_folder и выделение не трогаем
    });

    // ── Двойной клик по названию в дереве ────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_tree_item_activated(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();

        if is_folder {
            // Папка: toggle expand + select + navigate
            if s.expanded.contains(&id64) {
                s.expanded.remove(&id64);
            } else {
                s.expanded.insert(id64);
            }
            s.selected_id        = Some(id64);
            s.selected_is_folder = true;
            s.current_folder     = Some(id64);
            s.status_id          = None;
            w.set_selected_id(id);
            w.set_status_id(-1);
            w.set_status_visible(false);
            w.set_show_card(false);
            refresh_tree(&s, &w);
            refresh_contents(&s, &w);
        } else {
            // Ссылка: карточка + открыть в браузере + touch_visited
            s.selected_id        = Some(id64);
            s.selected_is_folder = false;
            if let Ok(node) = db::get_node(&s.db, id64) {
                let url = node.url.unwrap_or_default();
                w.set_card_id(id);
                w.set_card_title(node.title.into());
                w.set_card_url(url.clone().into());
                if !url.is_empty() {
                    let _ = webbrowser::open(&url);
                    let _ = db::touch_visited(&s.db, id64);
                }
            }
            w.set_selected_id(id);
            w.set_show_card(true);
            w.set_status_visible(false);
        }
    });

    // ── Новая папка — открыть диалог создания ────────────────────────────────
    let ww = window.as_weak();
    window.on_new_folder(move || {
        let w = ww.upgrade().unwrap();
        w.set_prop_is_new(true);
        w.set_prop_is_folder(true);
        w.set_prop_id(-1);
        w.set_prop_title("".into());
        w.set_prop_url("".into());
        w.set_prop_note("".into());
        w.set_prop_visible(true);
    });

    // ── Новая ссылка — открыть диалог создания ───────────────────────────────
    let ww = window.as_weak();
    window.on_new_bookmark(move || {
        let w = ww.upgrade().unwrap();
        w.set_prop_is_new(true);
        w.set_prop_is_folder(false);
        w.set_prop_id(-1);
        w.set_prop_title("".into());
        w.set_prop_url("https://".into());
        w.set_prop_note("".into());
        w.set_prop_visible(true);
    });

    // ── Удалить выбранный узел ────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_delete_selected(move || {
        let mut s = sc.borrow_mut();
        if let Some(id) = s.selected_id {
            let _ = db::delete_node(&s.db, id);
            if s.current_folder == Some(id) {
                s.current_folder = None;
            }
            s.expanded.remove(&id);
            s.selected_id = None;
            s.status_id   = None;
        }
        let w = ww.upgrade().unwrap();
        w.set_selected_id(-1);
        w.set_status_id(-1);
        w.set_status_visible(false);
        w.set_show_card(false);
        refresh_tree(&s, &w);
        refresh_contents(&s, &w);
    });

    // ── Контекстное меню: ПКМ ────────────────────────────────────────────────
    // ctx-x/ctx-y уже выставлены из Slint до вызова callback
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_ctx_request(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        s.selected_id        = Some(id64);
        s.selected_is_folder = is_folder;
        if is_folder {
            s.current_folder = Some(id64);
        }
        let w = ww.upgrade().unwrap();
        w.set_ctx_id(id);
        w.set_ctx_is_folder(is_folder);
        w.set_selected_id(id);
        w.set_ctx_visible(true);
    });

    // ── Открыть в браузере через ПКМ ─────────────────────────────────────────
    let sc = Rc::clone(&state);
    window.on_ctx_open_browser(move |id| {
        let id64 = id as i64;
        let s = sc.borrow();
        if let Ok(node) = db::get_node(&s.db, id64) {
            let url = node.url.unwrap_or_default();
            if !url.is_empty() {
                let _ = webbrowser::open(&url);
                let _ = db::touch_visited(&s.db, id64);
            }
        }
    });

    // ── Открыть диалог свойств ────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_open_properties(move |id| {
        let id64 = id as i64;
        let s = sc.borrow();
        if let Ok(node) = db::get_node(&s.db, id64) {
            let w = ww.upgrade().unwrap();
            w.set_prop_id(id);
            w.set_prop_is_new(false);
            w.set_prop_title(node.title.into());
            w.set_prop_url(node.url.unwrap_or_default().into());
            w.set_prop_note(node.note.unwrap_or_default().into());
            w.set_prop_is_folder(node.kind == "folder");
            w.set_prop_visible(true);
        }
    });

    // ── Сохранить свойства ────────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_save_properties(move || {
        let w = ww.upgrade().unwrap();
        let is_new    = w.get_prop_is_new();
        let is_folder = w.get_prop_is_folder();
        let title     = w.get_prop_title();
        let url_str   = w.get_prop_url();
        let note_str  = w.get_prop_note();
        let url  = if url_str.is_empty()  { None } else { Some(url_str.as_str())  };
        let note = if note_str.is_empty() { None } else { Some(note_str.as_str()) };

        let mut s = sc.borrow_mut();

        if is_new {
            // Режим создания: INSERT нового узла
            let kind   = if is_folder { "folder" } else { "bookmark" };
            let parent = s.current_folder;
            if let Ok(new_id) = db::insert_node(&s.db, parent, kind, title.as_str(), url, note) {
                if let Some(p) = parent { s.expanded.insert(p); }
                s.selected_id        = Some(new_id);
                s.selected_is_folder = is_folder;
            }
        } else {
            // Режим редактирования: UPDATE существующего
            let id = w.get_prop_id() as i64;
            let _ = db::update_node(&s.db, id, title.as_str(), url, note);
        }

        w.set_prop_visible(false);
        w.set_prop_is_new(false);
        w.set_selected_id(s.selected_id.unwrap_or(-1) as i32);
        refresh_tree(&s, &w);
        refresh_contents(&s, &w);
    });

    // ── Отменить диалог ───────────────────────────────────────────────────────
    let ww = window.as_weak();
    window.on_cancel_properties(move || {
        ww.upgrade().unwrap().set_prop_visible(false);
    });

    window.run().unwrap();
}
