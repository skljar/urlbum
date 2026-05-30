# URLbum — CLAUDE.md

## Что это

Менеджер закладок (наследник классического URL Album, "духовный", не форк).
Чистый проект с нуля, заменяет URL-Album-3.

## Стек и таргет

- Rust + Slint (winit backend, БЕЗ кастомного platform.rs)
- Target: x86_64-pc-windows-msvc, Windows 10/11 ТОЛЬКО
- SQLite через rusqlite (bundled), crt-static → один portable exe ~11 МБ
- НЕТ Win7/8 поддержки, НЕТ pe-patch, НЕТ compat.rs, НЕТ шимов
- Сборка: обычный cargo build (минуты, не 18 как в старом проекте на i686)

## Структура

- `src/main.rs` — State (Rc<RefCell>), UI callbacks, refresh функции
- `src/db.rs` — SQLite CRUD, единая таблица nodes
- `ui/main.slint` — UI (тулбар, дерево слева, список справа)
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
- `get_children` использует `WHERE parent IS ?1` (IS, не =, для NULL)
- `delete_node` — рекурсивный CTE (удаляет узел + всех потомков)

## Что готово (скелет, 2026-05-30)

- Дерево вложенных папок (depth-отступ), список ссылок
- CRUD: создать папку/ссылку, удалить (рекурсивно), выбрать/раскрыть папку
- `album.db` рядом с exe, данные сохраняются между запусками
- 4 теста DB-слоя (`cargo test`)

## Очередь фич (приоритет)

1. Редактирование (переименование F2, изменить URL, свойства F4) + контекстные меню
2. Импорт (ua.dat Win-1251, браузерные закладки JSON/HTML)
3. Favicon (логику взять из старого URL-Album-3 как справочника)
4. Скриншоты — ЧИСТЫЙ случай: Edge/Chrome Win10/11, `--headless=new` + `--user-data-dir`
5. Drag&drop, поиск (Ctrl+F), detail-карточка

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
