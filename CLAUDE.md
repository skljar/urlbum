# URLbum — CLAUDE.md

## Что это

Менеджер закладок (наследник классического URL Album, "духовный", не форк).
Чистый проект с нуля, заменяет URL-Album-3.

## Стек и таргет

- Rust + Slint 1.16 (winit backend, БЕЗ кастомного platform.rs)
- Target: x86_64-pc-windows-msvc, Windows 10/11 ТОЛЬКО
- SQLite через rusqlite 0.32 (bundled), crt-static → один portable exe ~11 МБ
- webbrowser 1.2 — открытие URL (ShellExecuteW, без shell-injection)
- НЕТ Win7/8 поддержки, НЕТ pe-patch, НЕТ compat.rs, НЕТ шимов
- Сборка: обычный cargo build

## Структура

- `src/main.rs` — State (Rc<RefCell>), UI callbacks, refresh функции
- `src/db.rs` — SQLite CRUD, единая таблица nodes
- `ui/main.slint` — UI (тулбар, дерево слева, список/карточка/статусбар справа)
- `build.rs` — slint_build::compile
- `.cargo/config.toml` — target x64 + crt-static

## Схема БД (единая таблица nodes)

```sql
CREATE TABLE nodes (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    parent   INTEGER,
    kind     TEXT NOT NULL DEFAULT 'bookmark',  -- 'folder' | 'bookmark'
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

- `parent IS NULL` = корень; `kind` = `'folder'` | `'bookmark'`
- `get_children` — `WHERE parent IS ?1` (IS для NULL); возвращает 7 колонок
- `get_node` — полная строка (9 колонок, включая created/visited)
- `delete_node` — рекурсивный CTE (удаляет узел + всех потомков)
- `touch_visited` — `UPDATE SET visited=datetime('now')` при открытии в браузере

## State (main.rs)

```rust
struct State {
    db:                rusqlite::Connection,
    selected_id:       Option<i64>,   // подсвечен в дереве
    selected_is_folder: bool,
    current_folder:    Option<i64>,   // папка, чьё содержимое в правой панели
    status_id:         Option<i64>,   // выделенная строка в списке (статусбар)
    expanded:          HashSet<i64>,
}
```

## Модель кликов (финальная)

### Левое дерево
| Действие | Результат |
|---|---|
| Клик на значок ▶/▼ папки | toggle expand + refresh_tree |
| Одиночный клик на название папки | select + current_folder + refresh_contents |
| Двойной клик на название папки | toggle expand + select + navigate + refresh обеих |
| Одиночный клик на название ссылки | карточка справа (show-card=true) |
| Двойной клик на название ссылки | карточка + открыть в браузере + touch_visited |

### Правая панель (список)
| Действие | Результат |
|---|---|
| Одиночный клик на папку | только подсветить строку (status-id), без навигации |
| Двойной клик на папку | navigate (current_folder=id) + expand + refresh обеих |
| Одиночный клик на ссылку | подсветка строки + статусбар (URL/note/даты) |
| Двойной клик на ссылку | карточка (show-card=true) |

## Режимы правой панели

1. **Список** (`show-card=false`): содержимое current_folder (папки + ссылки). Внизу статусбар при выбранной ссылке.
2. **Карточка** (`show-card=true`): заголовок крупно + URL + заглушка-превью + кнопка "Edit…"

## Что готово (2026-05-31)

- ✅ Смешанное дерево папок и ссылок (depth-отступ, раздельные зоны клика)
- ✅ Список содержимого папки (папки + ссылки вместе)
- ✅ Статусбар: URL, note, created, visited при одиночном клике на ссылку в списке
- ✅ Карточка ссылки: заголовок + URL + заглушка превью + Edit…
- ✅ Диалог свойств: редактирование title/url/note (через Edit… в карточке)
- ✅ CRUD: создать папку/ссылку, удалить (рекурсивно), переименовать
- ✅ Открытие ссылки в браузере по умолчанию (двойной клик в дереве)
- ✅ touch_visited при открытии
- ✅ album.db рядом с exe, данные сохраняются между запусками
- ✅ 7 тестов DB-слоя (`cargo test`)

## Очередь фич (приоритет)

1. **Редактирование в списке** — контекстное меню (F2 переименование, F4 свойства, Delete)
2. **Импорт** — ua.dat Win-1251, браузерные закладки JSON/HTML
3. **Favicon** — логику взять из старого URL-Album-3 как справочника
4. **Скриншоты** — Edge/Chrome Win10/11, `--headless=new` + `--user-data-dir`
5. **Drag&drop, поиск** (Ctrl+F), sort_idx для порядка
6. **Открытие в браузере из списка** — двойной клик по ссылке в правой панели (сейчас только карточка)

## Справочник

Старый проект url-album-3 (репо github.com/skljar/url-album-2) — источник
проверенной логики для переноса (db, net/favicon, парсеры импорта). Переписывать
чисто, подсматривая, НЕ копировать целиком. Win7-слой не трогать.

## Уроки скриншотов (из старого проекта, для фазы 4)

- Edge headless ОБЯЗАТЕЛЬНО требует `--user-data-dir` (иначе "Missing headless
  user data directory")
- Chrome/Edge пишут PNG асинхронно → нужен поллинг файла, не проверка сразу
- На Win10/11 всё просто: `--headless=new` + `--user-data-dir`, уникальный профиль
  на запуск, гарантированный kill процесса

## Технические заметки

- Геттеры Slint для свойств с префиксом `prop-` генерируются как `get_prop_*()`,
  не `prop_*()` (особенность Slint 1.16)
- `webbrowser::open(url)` — ShellExecuteW на Windows, без shell-injection
- Highlight строк реактивный через `item.id == root.status-id` — refresh_contents
  при одиночном клике НЕ нужен
