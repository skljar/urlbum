slint::include_modules!();

mod db;
mod favicon;
mod import;
mod settings;

extern crate webbrowser;

use std::cell::RefCell;
use std::collections::{HashSet, HashMap};
use std::mem;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use slint::{Image, Model, VecModel};

struct State {
    db:                 rusqlite::Connection,
    db_path:            String,              // путь к текущей БД; для воркеров (не Send)
    data_dir:           PathBuf,             // директория БД; favicons → data_dir/favicons/
    exe_dir:            PathBuf,             // директория exe; recent_dbs.json живёт здесь
    selected_id:        Option<i64>,
    selected_is_folder: bool,
    current_folder:     Option<i64>,
    status_id:          Option<i64>,
    expanded:           HashSet<i64>,
    favicon_cancel:     Arc<AtomicBool>,     // сброс при смене базы → воркеры прекращают запись
    settings:           settings::Settings,
}

impl State {
    fn favicons_dir(&self) -> PathBuf {
        self.data_dir.join("favicons")
    }
}

// ─── Favicon helper ──────────────────────────────────────────────────────────

fn load_favicon(id: i64, favicons: &HashMap<i64, String>, dir: &std::path::Path) -> (Image, bool) {
    if let Some(fname) = favicons.get(&id) {
        if !fname.is_empty() {
            let p = dir.join(fname);
            if let Ok(img) = Image::load_from_path(&p) {
                return (img, true);
            }
        }
    }
    (Image::default(), false)
}

// ─── Обновление левого дерева ────────────────────────────────────────────────

fn build_tree_list(
    conn: &rusqlite::Connection,
    parent: Option<i64>,
    depth: i32,
    expanded: &HashSet<i64>,
    favicons: &HashMap<i64, String>,
    favicons_dir: &std::path::Path,
    out: &mut Vec<TreeItem>,
) {
    let Ok(nodes) = db::get_children(conn, parent) else { return };
    for node in nodes {
        let is_folder = node.kind == "folder";
        let is_exp = is_folder && expanded.contains(&node.id);
        let (fav, has_fav) = if !is_folder {
            load_favicon(node.id, favicons, favicons_dir)
        } else {
            (Image::default(), false)
        };
        out.push(TreeItem {
            id:          node.id as i32,
            title:       node.title.clone().into(),
            depth,
            expanded:    is_exp,
            is_folder,
            url:         node.url.clone().unwrap_or_default().into(),
            favicon:     fav,
            has_favicon: has_fav,
        });
        if is_exp {
            build_tree_list(conn, Some(node.id), depth + 1, expanded, favicons, favicons_dir, out);
        }
    }
}

fn refresh_status(db: &rusqlite::Connection, window: &AppWindow) {
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .unwrap_or(0);
    window.set_status_count(count as i32);
}

fn refresh_tree(state: &State, window: &AppWindow) {
    let favicons = db::get_favicons(&state.db);
    let favicons_dir = state.favicons_dir();
    let mut items: Vec<TreeItem> = Vec::new();
    build_tree_list(&state.db, None, 0, &state.expanded, &favicons, &favicons_dir, &mut items);
    window.set_tree_model(Rc::new(VecModel::from(items)).into());
    refresh_status(&state.db, window);
}

// ─── Обновление правого списка ───────────────────────────────────────────────

fn refresh_contents(state: &State, window: &AppWindow) {
    let favicons = db::get_favicons(&state.db);
    let favicons_dir = state.favicons_dir();
    let items: Vec<TreeItem> = match state.current_folder {
        None => vec![],
        Some(folder_id) => db::get_children(&state.db, Some(folder_id))
            .unwrap_or_default()
            .into_iter()
            .map(|n| {
                let is_folder = n.kind == "folder";
                let (fav, has_fav) = if !is_folder {
                    load_favicon(n.id, &favicons, &favicons_dir)
                } else {
                    (Image::default(), false)
                };
                TreeItem {
                    id:          n.id as i32,
                    title:       n.title.clone().into(),
                    depth:       0,
                    expanded:    false,
                    is_folder,
                    url:         n.url.clone().unwrap_or_default().into(),
                    favicon:     fav,
                    has_favicon: has_fav,
                }
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

// ─── Recent databases helpers ────────────────────────────────────────────────

fn load_recent_dbs(exe_dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(exe_dir.join("recent_dbs.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn push_recent_db(exe_dir: &std::path::Path, new_path: &str) -> Vec<String> {
    let mut list = load_recent_dbs(exe_dir);
    list.retain(|p| p != new_path);
    list.insert(0, new_path.to_string());
    list.truncate(10);
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = std::fs::write(exe_dir.join("recent_dbs.json"), json);
    }
    list
}

// ─── UI reset при смене базы ─────────────────────────────────────────────────

fn apply_recent_dbs(exe_dir: &std::path::Path, w: &AppWindow) {
    let list = load_recent_dbs(exe_dir);
    let items: Vec<RecentDb> = list.iter().map(|p| {
        let title = std::path::Path::new(p)
            .file_name().and_then(|n| n.to_str()).unwrap_or(p).into();
        RecentDb { title, path: p.as_str().into() }
    }).collect();
    w.set_recent_dbs(Rc::new(VecModel::from(items)).into());
}

fn reset_ui_state(s: &mut State, w: &AppWindow) {
    s.selected_id        = None;
    s.selected_is_folder = true;
    s.current_folder     = None;
    s.status_id          = None;
    s.expanded.clear();
    w.set_selected_id(-1);
    w.set_status_id(-1);
    w.set_status_visible(false);
    w.set_show_card(false);
    w.set_search_visible(false);
    w.set_search_is_duplicates(false);
    w.set_search_query("".into());
    w.set_search_model(Rc::new(VecModel::from(vec![])).into());
    w.set_dup_visible(false);
    refresh_tree(s, w);
    refresh_contents(s, w);
}

// ─── Открытие URL в браузере ─────────────────────────────────────────────────

fn normalize_url(url: &str) -> String {
    if url.is_empty() { return url.to_string(); }
    if url.contains("://") || url.starts_with("mailto:") || url.starts_with("file:") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

// Показать карточку + открыть в браузере + touch_visited.
// Вызывается из on_tree_item_activated и on_list_item_activated (bookmark branch).
fn activate_bookmark(id: i64, db: &rusqlite::Connection, w: &AppWindow) {
    if let Ok(node) = db::get_node(db, id) {
        let url = normalize_url(node.url.as_deref().unwrap_or_default());
        w.set_card_id(id as i32);
        w.set_card_title(node.title.into());
        w.set_card_url(url.clone().into());
        if !url.is_empty() {
            let _ = webbrowser::open(&url);
            let _ = db::touch_visited(db, id);
        }
    }
    w.set_selected_id(id as i32);
    w.set_show_card(true);
    w.set_status_visible(false);
}

// ─── Сворачивание соседних папок при раскрытии ───────────────────────────────

fn collapse_siblings_if_needed(s: &mut State, expanding_id: i64) {
    if !s.settings.collapse_siblings { return; }
    if let Ok(node) = db::get_node(&s.db, expanding_id) {
        if let Ok(siblings) = db::get_children(&s.db, node.parent) {
            for sib in siblings {
                if sib.id != expanding_id && sib.kind == "folder" {
                    s.expanded.remove(&sib.id);
                }
            }
        }
    }
}

// ─── Удаление выбранного узла ────────────────────────────────────────────────

fn do_delete_selected(s: &mut State, w: &AppWindow) {
    if let Some(id) = s.selected_id {
        let _ = db::delete_node(&s.db, id);
        if s.current_folder == Some(id) { s.current_folder = None; }
        s.expanded.remove(&id);
        s.selected_id = None;
        s.status_id   = None;
    }
    w.set_selected_id(-1);
    w.set_status_id(-1);
    w.set_status_visible(false);
    w.set_show_card(false);
    refresh_tree(s, w);
    refresh_contents(s, w);
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
        db:                 db::open(db_path.to_str().unwrap()).expect("cannot open album.db"),
        db_path:            db_path.to_str().unwrap().to_string(),
        data_dir:           exe_dir.clone(),
        exe_dir:            exe_dir.clone(),
        selected_id:        None,
        selected_is_folder: true,
        current_folder:     None,
        status_id:          None,
        expanded:           HashSet::new(),
        favicon_cancel:     Arc::new(AtomicBool::new(false)),
        settings:           settings::load_settings(&exe_dir),
    }));

    let window = AppWindow::new().unwrap();

    {
        let db_file = db_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("album.db");
        window.set_db_name(db_file.into());

        let s = state.borrow();
        refresh_tree(&s, &window);
        refresh_contents(&s, &window);
        apply_recent_dbs(&s.exe_dir, &window);
        window.set_settings_toolbar_visible(s.settings.show_toolbar);
    }

    // ── Клик по узлу в левом дереве ──────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_tree_item_clicked(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();

        if is_folder {
            if s.expanded.contains(&id64) {
                s.expanded.remove(&id64);
            } else {
                collapse_siblings_if_needed(&mut s, id64);
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
            s.status_id = Some(id64);
            w.set_status_id(id);
            w.set_status_visible(false);
        } else {
            s.status_id = Some(id64);
            show_statusbar(id64, &s.db, &w);
        }
    });

    // ── Двойной клик в правом списке ─────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_list_item_activated(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();

        if is_folder {
            if !s.expanded.contains(&id64) {
                collapse_siblings_if_needed(&mut s, id64);
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
            s.selected_id        = Some(id64);
            s.selected_is_folder = false;
            activate_bookmark(id64, &s.db, &w);
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
            collapse_siblings_if_needed(&mut s, id64);
            s.expanded.insert(id64);
        }
        let w = ww.upgrade().unwrap();
        refresh_tree(&s, &w);
    });

    // ── Двойной клик по названию в дереве ────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_tree_item_activated(move |id, is_folder| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();

        if is_folder {
            if s.expanded.contains(&id64) {
                s.expanded.remove(&id64);
            } else {
                collapse_siblings_if_needed(&mut s, id64);
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
            s.selected_id        = Some(id64);
            s.selected_is_folder = false;
            activate_bookmark(id64, &s.db, &w);
        }
    });

    // ── Новая папка ───────────────────────────────────────────────────────────
    let ww = window.as_weak();
    window.on_new_folder(move || {
        let w = ww.upgrade().unwrap();
        w.set_prop_is_new(true);
        w.set_prop_is_folder(true);
        w.set_prop_id(-1);
        w.set_prop_title("".into());
        w.set_prop_url("".into());
        w.set_prop_note("".into());
        w.set_prop_error("".into());
        w.set_prop_visible(true);
    });

    // ── Новая ссылка ──────────────────────────────────────────────────────────
    let ww = window.as_weak();
    window.on_new_bookmark(move || {
        let w = ww.upgrade().unwrap();
        w.set_prop_is_new(true);
        w.set_prop_is_folder(false);
        w.set_prop_id(-1);
        w.set_prop_title("".into());
        w.set_prop_url("https://".into());
        w.set_prop_note("".into());
        w.set_prop_error("".into());
        w.set_prop_visible(true);
    });

    // ── Удалить выбранный узел (вызывается кнопкой "Да" в диалоге подтверждения) ──
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_delete_selected(move || {
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();
        do_delete_selected(&mut s, &w);
    });

    // ── Запросить удаление (меню / тулбар / ПКМ — с проверкой настройки) ─────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_request_delete(move || {
        let w = ww.upgrade().unwrap();
        let needs_confirm = {
            let s = sc.borrow();
            s.settings.confirm_delete && s.selected_id.is_some()
        };
        if needs_confirm {
            w.set_delete_confirm_visible(true);
        } else {
            let mut s = sc.borrow_mut();
            do_delete_selected(&mut s, &w);
        }
    });

    // ── Контекстное меню: ПКМ ────────────────────────────────────────────────
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
            let url = normalize_url(node.url.as_deref().unwrap_or_default());
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
            w.set_prop_error("".into());
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
            let kind   = if is_folder { "folder" } else { "bookmark" };
            let parent = s.current_folder;

            if !is_folder && s.settings.no_duplicate_urls {
                if let Some(u) = url {
                    if db::url_exists(&s.db, u) {
                        w.set_prop_error("URL уже существует в базе".into());
                        return;
                    }
                }
            }

            if let Ok(new_id) = db::insert_node(&s.db, parent, kind, title.as_str(), url, note) {
                if let Some(p) = parent { s.expanded.insert(p); }
                s.selected_id        = Some(new_id);
                s.selected_is_folder = is_folder;
            }
        } else {
            let id = w.get_prop_id() as i64;
            let _ = db::update_node(&s.db, id, title.as_str(), url, note);
        }

        w.set_prop_visible(false);
        w.set_prop_is_new(false);
        w.set_selected_id(s.selected_id.unwrap_or(-1) as i32);
        refresh_tree(&s, &w);
        refresh_contents(&s, &w);
    });

    // ── favicon-done: финальный refresh + очистка статуса ─────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_favicon_done(move || {
        let s = sc.borrow();
        let w = ww.upgrade().unwrap();
        refresh_tree(&s, &w);
        refresh_contents(&s, &w);
        let ww2 = ww.clone();
        slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
            if let Some(w) = ww2.upgrade() {
                w.set_import_status("".into());
            }
        });
    });

    // ── Загрузка favicon'ов для папки (ПКМ → Обновить favicon'ы) ─────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_ctx_load_favicons_folder(move |folder_id| {
        let (bms, favicons_dir, db_path_str, cancel) = {
            let s = sc.borrow();
            let bms = db::get_bookmarks_recursive(&s.db, folder_id as i64).unwrap_or_default();
            (bms, s.favicons_dir(), s.db_path.clone(), s.favicon_cancel.clone())
        };
        let deduped = favicon::dedup_by_domain(bms);
        let total = deduped.len();
        if total == 0 { return; }

        if let Some(w) = ww.upgrade() {
            w.set_import_status(format!("Favicon: 0 / {total}").into());
        }

        let queue        = Arc::new(Mutex::new(deduped));
        let needs_rebuild = Arc::new(AtomicBool::new(false));
        let done_count   = Arc::new(AtomicUsize::new(0));
        let active       = Arc::new(AtomicUsize::new(5));

        for _ in 0..5 {
            let queue         = queue.clone();
            let needs_rebuild = needs_rebuild.clone();
            let done_count    = done_count.clone();
            let active        = active.clone();
            let cancel        = cancel.clone();
            let db            = db_path_str.clone();
            let favicons_dir  = favicons_dir.clone();
            let ww2           = ww.clone();
            let total2        = total;
            std::thread::spawn(move || {
                if let Ok(conn) = rusqlite::Connection::open(&db) {
                    let _ = conn.execute_batch("PRAGMA busy_timeout = 5000");
                    loop {
                        let item = { queue.lock().unwrap().pop() };
                        let Some((bm, same_ids)) = item else { break };
                        if cancel.load(Ordering::Relaxed) { break; }
                        if let Some(url) = bm.url.as_deref() {
                            if let Some(fname) = favicon::fetch_favicon(url, &favicons_dir) {
                                if cancel.load(Ordering::Relaxed) { break; }
                                for id in &same_ids {
                                    let _ = db::set_favicon(&conn, *id, &fname);
                                }
                                needs_rebuild.store(true, Ordering::Relaxed);
                            }
                        }
                        done_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
                // Последний воркер сигнализирует о завершении
                if active.fetch_sub(1, Ordering::AcqRel) == 1 && !cancel.load(Ordering::Relaxed) {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ww2.upgrade() {
                            w.set_import_status(
                                format!("Favicon: {total2} / {total2}").into(),
                            );
                            w.invoke_favicon_done();
                        }
                    });
                }
            });
        }

        // Таймер на main thread: промежуточные rebuild каждые 200 мс
        let needs_rebuild2 = needs_rebuild.clone();
        let done2          = done_count.clone();
        let sc2            = Rc::clone(&sc);
        let ww2            = ww.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(200),
            move || {
                if needs_rebuild2.swap(false, Ordering::Relaxed) {
                    if let Some(w) = ww2.upgrade() {
                        let s = sc2.borrow();
                        refresh_tree(&s, &w);
                        refresh_contents(&s, &w);
                        let n = done2.load(Ordering::Relaxed).min(total);
                        w.set_import_status(format!("Favicon: {n} / {total}").into());
                    }
                }
            },
        );
        mem::forget(timer); // таймер живёт до завершения (idle после done)
    });

    // ── Загрузка favicon одной ссылки (ПКМ → Загрузить favicon) ──────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_ctx_load_favicon_bm(move |id| {
        let (url, favicons_dir, db_path_str, cancel) = {
            let s = sc.borrow();
            let url = db::get_node(&s.db, id as i64).ok().and_then(|n| n.url);
            (url, s.favicons_dir(), s.db_path.clone(), s.favicon_cancel.clone())
        };
        let Some(url) = url else { return; };
        let db  = db_path_str;
        let ww2 = ww.clone();
        std::thread::spawn(move || {
            if let Some(fname) = favicon::fetch_favicon(&url, &favicons_dir) {
                if !cancel.load(Ordering::Relaxed) {
                    if let Ok(conn) = rusqlite::Connection::open(&db) {
                        let _ = conn.execute_batch("PRAGMA busy_timeout = 5000");
                        let _ = db::set_favicon(&conn, id as i64, &fname);
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ww2.upgrade() {
                            w.invoke_favicon_done();
                        }
                    });
                }
            }
        });
    });

    // ── Завершение импорта ────────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_import_done(move || {
        let s = sc.borrow();
        let w = ww.upgrade().unwrap();
        refresh_tree(&s, &w);
        refresh_contents(&s, &w);
        let ww2 = ww.clone();
        slint::Timer::single_shot(std::time::Duration::from_secs(5), move || {
            if let Some(w) = ww2.upgrade() {
                w.set_import_status("".into());
            }
        });
    });

    // ── Импорт ua.dat — фоновый поток ─────────────────────────────────────────
    let (sc_imp, ww) = (Rc::clone(&state), window.as_weak());
    window.on_import_ua_dat(move || {
        let w = ww.upgrade().unwrap();
        w.set_import_in_progress(true);
        w.set_import_status("Импортирую...".into());

        let db   = sc_imp.borrow().db_path.clone();
        let ww2  = ww.clone();
        std::thread::spawn(move || {
            let conn = match rusqlite::Connection::open(&db) {
                Ok(c) => c,
                Err(e) => {
                    slint::invoke_from_event_loop(move || {
                        if let Some(w) = ww2.upgrade() {
                            w.set_import_status(format!("Ошибка БД: {e}").into());
                            w.set_import_in_progress(false);
                        }
                    }).ok();
                    return;
                }
            };
            let _ = conn.execute_batch("PRAGMA busy_timeout = 5000");

            let ww_progress = ww2.clone();
            let result = import::import_ua_dat(
                &conn,
                r"C:\Projects\url-album-2\ua.dat",
                move |count| {
                    let ww = ww_progress.clone();
                    slint::invoke_from_event_loop(move || {
                        if let Some(w) = ww.upgrade() {
                            w.set_import_status(format!("Импортировано {count}...").into());
                        }
                    }).ok();
                },
            );

            slint::invoke_from_event_loop(move || {
                if let Some(w) = ww2.upgrade() {
                    match result {
                        Ok(n)  => w.set_import_status(format!("Готово: {n} записей").into()),
                        Err(e) => w.set_import_status(format!("Ошибка: {e}").into()),
                    }
                    w.set_import_in_progress(false);
                    w.invoke_import_done();
                }
            }).ok();
        });
    });

    // ── Открыть базу данных ───────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_open_db(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Открыть базу данных")
            .add_filter("SQLite Database", &["db"])
            .add_filter("All Files", &["*"])
            .pick_file()
        else { return };
        let path_str = match path.to_str() { Some(s) => s.to_string(), None => return };
        let Ok(new_conn) = db::open(&path_str) else { return };
        let new_data_dir = path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let db_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("album.db")
            .to_string();

        let mut s = sc.borrow_mut();
        s.favicon_cancel.store(true, Ordering::Relaxed);
        s.favicon_cancel = Arc::new(AtomicBool::new(false));
        s.db       = new_conn;
        s.db_path  = path_str.clone();
        s.data_dir = new_data_dir;
        let exe_dir = s.exe_dir.clone();
        push_recent_db(&exe_dir, &path_str);

        let w = ww.upgrade().unwrap();
        w.set_db_name(db_name.into());
        reset_ui_state(&mut s, &w);
        apply_recent_dbs(&exe_dir, &w);
    });

    // ── Создать базу данных ───────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_create_db(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Создать новую базу данных")
            .add_filter("SQLite Database", &["db"])
            .set_file_name("album.db")
            .save_file()
        else { return };
        let path_str = match path.to_str() { Some(s) => s.to_string(), None => return };
        // Удалить существующий файл, чтобы создать пустую БД
        if path.exists() { let _ = std::fs::remove_file(&path); }
        let Ok(new_conn) = db::open(&path_str) else { return };
        let new_data_dir = path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let db_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("album.db")
            .to_string();

        let mut s = sc.borrow_mut();
        s.favicon_cancel.store(true, Ordering::Relaxed);
        s.favicon_cancel = Arc::new(AtomicBool::new(false));
        s.db       = new_conn;
        s.db_path  = path_str.clone();
        s.data_dir = new_data_dir;
        let exe_dir = s.exe_dir.clone();
        push_recent_db(&exe_dir, &path_str);

        let w = ww.upgrade().unwrap();
        w.set_db_name(db_name.into());
        reset_ui_state(&mut s, &w);
        apply_recent_dbs(&exe_dir, &w);
    });

    // ── Закрыть базу ──────────────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_close_db(move || {
        let Ok(empty) = db::open(":memory:") else { return };
        let mut s = sc.borrow_mut();
        s.favicon_cancel.store(true, Ordering::Relaxed);
        s.favicon_cancel = Arc::new(AtomicBool::new(false));
        s.db      = empty;
        s.db_path = ":memory:".into();
        s.data_dir = s.exe_dir.clone();

        let w = ww.upgrade().unwrap();
        w.set_db_name("(нет базы)".into());
        reset_ui_state(&mut s, &w);
    });

    // ── Последние базы: открыть по пути ──────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_open_recent(move |path_ss| {
        let path_str = path_ss.to_string();
        let path = std::path::Path::new(&path_str);
        let Ok(new_conn) = db::open(&path_str) else { return };
        let new_data_dir = path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let db_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("album.db")
            .to_string();

        let mut s = sc.borrow_mut();
        s.favicon_cancel.store(true, Ordering::Relaxed);
        s.favicon_cancel = Arc::new(AtomicBool::new(false));
        s.db       = new_conn;
        s.db_path  = path_str.clone();
        s.data_dir = new_data_dir;
        let exe_dir = s.exe_dir.clone();
        push_recent_db(&exe_dir, &path_str);

        let w = ww.upgrade().unwrap();
        w.set_db_name(db_name.into());
        reset_ui_state(&mut s, &w);
        apply_recent_dbs(&exe_dir, &w);
    });

    // ── Свойства базы данных ──────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_show_db_props(move || {
        let s = sc.borrow();
        let w = ww.upgrade().unwrap();
        let (folders, bookmarks) = db::count_nodes(&s.db);
        let size_str = if s.db_path == ":memory:" {
            "в памяти".to_string()
        } else {
            std::fs::metadata(&s.db_path)
                .map(|m| format!("{} байт", m.len()))
                .unwrap_or_else(|_| "—".to_string())
        };
        w.set_db_props_path(s.db_path.as_str().into());
        w.set_db_props_size(size_str.into());
        w.set_db_props_folders(folders as i32);
        w.set_db_props_bookmarks(bookmarks as i32);
        w.set_db_props_visible(true);
    });

    // ── Открыть диалог настроек ───────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_settings_open(move || {
        let s = sc.borrow();
        let w = ww.upgrade().unwrap();
        w.set_settings_show_toolbar(s.settings.show_toolbar);
        w.set_settings_collapse_siblings(s.settings.collapse_siblings);
        w.set_settings_confirm_delete(s.settings.confirm_delete);
        w.set_settings_no_duplicate_urls(s.settings.no_duplicate_urls);
        w.set_settings_db_path(s.db_path.as_str().into());
        w.set_settings_visible(true);
    });

    // ── Сохранить настройки ───────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_settings_save(move || {
        let w = ww.upgrade().unwrap();
        let mut s = sc.borrow_mut();
        s.settings.show_toolbar      = w.get_settings_show_toolbar();
        s.settings.collapse_siblings = w.get_settings_collapse_siblings();
        s.settings.confirm_delete    = w.get_settings_confirm_delete();
        s.settings.no_duplicate_urls = w.get_settings_no_duplicate_urls();
        settings::save_settings(&s.exe_dir, &s.settings);
        w.set_settings_toolbar_visible(s.settings.show_toolbar);
        w.set_settings_visible(false);
    });

    // ── Закрыть настройки без сохранения ─────────────────────────────────────
    let ww = window.as_weak();
    window.on_settings_cancel(move || {
        ww.upgrade().unwrap().set_settings_visible(false);
    });

    // ── Выход ────────────────────────────────────────────────────────────────
    window.on_quit_app(|| { let _ = slint::quit_event_loop(); });

    // ── Развернуть все папки ──────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_expand_all(move || {
        let mut s = sc.borrow_mut();
        for id in db::get_all_folder_ids(&s.db) {
            s.expanded.insert(id);
        }
        let w = ww.upgrade().unwrap();
        refresh_tree(&s, &w);
    });

    // ── Свернуть все папки ────────────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_collapse_all(move || {
        let mut s = sc.borrow_mut();
        s.expanded.clear();
        let w = ww.upgrade().unwrap();
        refresh_tree(&s, &w);
    });

    // ── Резервная копия ───────────────────────────────────────────────────────
    let sc = Rc::clone(&state);
    window.on_backup_db(move || {
        let path = rfd::FileDialog::new()
            .set_title("Резервная копия базы данных")
            .add_filter("SQLite Database", &["db"])
            .add_filter("All Files", &["*"])
            .set_file_name("album_backup.db")
            .save_file();
        if let Some(path) = path {
            let s = sc.borrow();
            let _ = db::backup(&s.db, path.to_str().unwrap_or_default());
        }
    });

    // ── Копировать URL ────────────────────────────────────────────────────────
    let sc = Rc::clone(&state);
    window.on_copy_url(move || {
        let s = sc.borrow();
        let Some(id) = s.selected_id else { return };
        if s.selected_is_folder { return; }
        if let Ok(node) = db::get_node(&s.db, id) {
            if let Some(url) = node.url {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(url);
                }
            }
        }
    });

    // ── Отменить диалог ───────────────────────────────────────────────────────
    let ww = window.as_weak();
    window.on_cancel_properties(move || {
        ww.upgrade().unwrap().set_prop_visible(false);
    });

    // ── Поиск: запрос изменился ───────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_search_changed(move |query| {
        let w = ww.upgrade().unwrap();
        let s = sc.borrow();
        if query.is_empty() {
            w.set_search_model(Rc::new(VecModel::from(vec![])).into());
            return;
        }
        let favicons     = db::get_favicons(&s.db);
        let favicons_dir = s.favicons_dir();
        let items: Vec<TreeItem> = db::search(&s.db, &query)
            .unwrap_or_default()
            .into_iter()
            .map(|n| {
                let (fav, has_fav) = load_favicon(n.id, &favicons, &favicons_dir);
                TreeItem {
                    id:          n.id as i32,
                    title:       n.title.into(),
                    depth:       0,
                    expanded:    false,
                    is_folder:   false,
                    url:         n.url.unwrap_or_default().into(),
                    favicon:     fav,
                    has_favicon: has_fav,
                }
            })
            .collect();
        w.set_search_model(Rc::new(VecModel::from(items)).into());
    });

    // ── Поиск: открыть результат (activate + закрыть поиск) ──────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_search_result_activated(move |id| {
        let id64 = id as i64;
        let mut s = sc.borrow_mut();
        let w = ww.upgrade().unwrap();
        s.selected_id        = Some(id64);
        s.selected_is_folder = false;
        activate_bookmark(id64, &s.db, &w);
        w.set_search_visible(false);
    });

    // ── Поиск: закрыть ───────────────────────────────────────────────────────
    let ww = window.as_weak();
    window.on_close_search(move || {
        let w = ww.upgrade().unwrap();
        w.set_search_visible(false);
        w.set_search_is_duplicates(false);
        w.set_search_query("".into());
        w.set_search_model(Rc::new(VecModel::from(vec![])).into());
    });

    // ── Найти дубликаты: открыть диалог ──────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_find_duplicates(move || {
        let w = ww.upgrade().unwrap();
        let s = sc.borrow();
        let groups = db::find_duplicate_groups(&s.db).unwrap_or_default();
        let groups_count = groups.len() as i32;
        let copies_count: i32 = groups.iter().map(|(_, c)| *c as i32).sum();
        let extras_count  = copies_count - groups_count;
        let dup_groups: Vec<DupGroup> = groups
            .into_iter()
            .map(|(url, count)| DupGroup { url: url.into(), count: count as i32 })
            .collect();
        w.set_dup_groups(Rc::new(VecModel::from(dup_groups)).into());
        w.set_dup_groups_count(groups_count);
        w.set_dup_copies_count(copies_count);
        w.set_dup_extras_count(extras_count);
        w.set_dup_items(Rc::new(VecModel::from(vec![])).into());
        w.set_dup_items_count(0);
        w.set_dup_selected_url("".into());
        w.set_dup_selected_item_id(-1);
        w.set_dup_confirm_visible(false);
        w.set_dup_visible(true);
    });

    // ── Группа выбрана в левой панели ────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_dup_group_clicked(move |url| {
        let w = ww.upgrade().unwrap();
        let s = sc.borrow();
        let items = db::find_duplicates_for_url(&s.db, &url).unwrap_or_default();
        let count = items.len() as i32;
        let slint_items: Vec<DupItem> = items
            .into_iter()
            .map(|d| DupItem {
                id:      d.id as i32,
                title:   d.title.into(),
                url:     d.url.into(),
                folder:  d.folder.into(),
                created: d.created.into(),
            })
            .collect();
        w.set_dup_items(Rc::new(VecModel::from(slint_items)).into());
        w.set_dup_items_count(count);
        w.set_dup_selected_url(url);
        w.set_dup_selected_item_id(-1);
    });

    // ── Удалить выбранную копию ───────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_dup_delete_selected(move || {
        let w = ww.upgrade().unwrap();
        let mut s = sc.borrow_mut();
        let item_id = w.get_dup_selected_item_id();
        if item_id == -1 { return; }
        let _ = db::delete_node(&s.db, item_id as i64);
        s.expanded.remove(&(item_id as i64));
        // Обновляем правую панель
        let url = w.get_dup_selected_url().to_string();
        let items = db::find_duplicates_for_url(&s.db, &url).unwrap_or_default();
        let count = items.len() as i32;
        if count < 2 {
            // Группа больше не дубликат — убираем из левой панели
            let remaining: Vec<DupGroup> = (0..w.get_dup_groups().row_count())
                .filter_map(|i| {
                    let g = w.get_dup_groups().row_data(i)?;
                    if g.url.as_str() != url { Some(g) } else { None }
                })
                .collect();
            let groups_count = remaining.len() as i32;
            let copies_count: i32 = remaining.iter().map(|g| g.count).sum();
            w.set_dup_groups(Rc::new(VecModel::from(remaining)).into());
            w.set_dup_groups_count(groups_count);
            w.set_dup_copies_count(copies_count);
            w.set_dup_extras_count(copies_count - groups_count);
            w.set_dup_items(Rc::new(VecModel::from(vec![])).into());
            w.set_dup_items_count(0);
            w.set_dup_selected_url("".into());
            w.set_dup_selected_item_id(-1);
        } else {
            let slint_items: Vec<DupItem> = items
                .into_iter()
                .map(|d| DupItem {
                    id:      d.id as i32,
                    title:   d.title.into(),
                    url:     d.url.into(),
                    folder:  d.folder.into(),
                    created: d.created.into(),
                })
                .collect();
            // Обновляем счётчик группы в левой панели
            let updated: Vec<DupGroup> = (0..w.get_dup_groups().row_count())
                .filter_map(|i| w.get_dup_groups().row_data(i))
                .map(|g| if g.url.as_str() == url { DupGroup { count: count, ..g } } else { g })
                .collect();
            let copies_count: i32 = updated.iter().map(|g| g.count).sum();
            let groups_count = updated.len() as i32;
            w.set_dup_groups(Rc::new(VecModel::from(updated)).into());
            w.set_dup_copies_count(copies_count);
            w.set_dup_extras_count(copies_count - groups_count);
            w.set_dup_items(Rc::new(VecModel::from(slint_items)).into());
            w.set_dup_items_count(count);
            w.set_dup_selected_item_id(-1);
        }
    });

    // ── Оставить одну копию (подтверждение уже получено) ─────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_dup_keep_one(move || {
        let w = ww.upgrade().unwrap();
        let mut s = sc.borrow_mut();
        let url = w.get_dup_selected_url().to_string();
        if url.is_empty() { return; }
        // keep_id: выбранный или min(id) = первый в списке (order by id)
        let keep_id = {
            let sel = w.get_dup_selected_item_id();
            if sel != -1 {
                sel as i64
            } else {
                // первый элемент (наименьший id) из правой панели
                w.get_dup_items().row_data(0).map(|d| d.id as i64).unwrap_or(-1)
            }
        };
        if keep_id == -1 { return; }
        let _ = db::delete_all_but_id(&s.db, &url, keep_id);
        s.expanded.retain(|id| {
            // очищаем expanded для удалённых (не критично, но аккуратно)
            let _ = id;
            true
        });
        // Убираем группу из левой панели
        let remaining: Vec<DupGroup> = (0..w.get_dup_groups().row_count())
            .filter_map(|i| {
                let g = w.get_dup_groups().row_data(i)?;
                if g.url.as_str() != url { Some(g) } else { None }
            })
            .collect();
        let groups_count = remaining.len() as i32;
        let copies_count: i32 = remaining.iter().map(|g| g.count).sum();
        w.set_dup_groups(Rc::new(VecModel::from(remaining)).into());
        w.set_dup_groups_count(groups_count);
        w.set_dup_copies_count(copies_count);
        w.set_dup_extras_count(copies_count - groups_count);
        w.set_dup_items(Rc::new(VecModel::from(vec![])).into());
        w.set_dup_items_count(0);
        w.set_dup_selected_url("".into());
        w.set_dup_selected_item_id(-1);
    });

    // ── Удалить все дубликаты (оставить MIN(id) по каждому URL) ─────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_dup_delete_all(move || {
        let w = ww.upgrade().unwrap();
        let s = sc.borrow();
        let _ = db::delete_all_duplicates(&s.db);
        // Очищаем диалог — дубликатов больше нет
        w.set_dup_groups(Rc::new(VecModel::from(vec![])).into());
        w.set_dup_items(Rc::new(VecModel::from(vec![])).into());
        w.set_dup_groups_count(0);
        w.set_dup_copies_count(0);
        w.set_dup_extras_count(0);
        w.set_dup_items_count(0);
        w.set_dup_selected_url("".into());
        w.set_dup_selected_item_id(-1);
        refresh_tree(&s, &w);
        refresh_contents(&s, &w);
    });

    // ── Закрыть диалог дубликатов ─────────────────────────────────────────────
    let (sc, ww) = (Rc::clone(&state), window.as_weak());
    window.on_dup_close(move || {
        let w = ww.upgrade().unwrap();
        w.set_dup_visible(false);
        let s = sc.borrow();
        refresh_tree(&s, &w);
        refresh_contents(&s, &w);
    });

    window.run().unwrap();
}
