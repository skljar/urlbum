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
- `get_children` — `WHERE parent IS ?1` (IS для NULL); 7 колонок (created/visited = None)
- `get_node` — полная строка (9 колонок, включая created/visited)
- `insert_node` — принимает `note: Option<&str>` (сразу пишет в БД)
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

## Диалог свойств (props dialog) — два режима

Флаг `prop-is-new: bool` различает режимы:

| Флаг | Заголовок | on_save_properties |
|---|---|---|
| `true` (создание) | "Новая папка" / "Новая ссылка" | INSERT: kind из `prop-is-folder`, parent из `current_folder` |
| `false` (редактирование) | "Folder/Bookmark Properties" | UPDATE по `prop-id` |

- `on_new_folder/bookmark` открывают диалог с пустыми полями — узел НЕ создаётся до ОК
- Контекст создания: kind из `prop-is-folder`, parent из `state.current_folder` (стабилен, диалог модальный)
- Cancel — всегда без изменений

## Контекстное меню (ПКМ)

Обнаружение: `pointer-event(event)` с `event.button == PointerEventButton.right`.
Позиция: `self.absolute-position.x + self.mouse-x` / `...y` — точная позиция курсора в окне (Slint 1.16 поддерживает `absolute-position`).
Работает: в обоих панелях (дерево + список).

| Тип узла | Пункты меню |
|---|---|
| Папка | Новая папка \| Новая ссылка \| — \| Свойства \| Удалить |
| Ссылка | Открыть \| — \| Свойства \| Удалить |

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
2. **Карточка** (`show-card=true`): заголовок крупно + URL + заглушка-превью. Свойства — через ПКМ.

## Что готово (2026-05-31)

- ✅ Смешанное дерево папок и ссылок (depth-отступ, раздельные зоны клика: значок/название)
- ✅ Список содержимого папки (папки + ссылки вместе, single/double click)
- ✅ Статусбар: URL, note, created, visited при одиночном клике на ссылку в списке
- ✅ Карточка ссылки: заголовок + URL + заглушка превью
- ✅ Диалог свойств: создание (prop-is-new=true) и редактирование (false)
- ✅ CRUD полный: создать папку/ссылку (через диалог), удалить (рекурсивно), изменить
- ✅ Контекстное меню (ПКМ) в дереве и списке, позиция у курсора
- ✅ Открытие ссылки в браузере (двойной клик в дереве, ПКМ → Открыть)
- ✅ touch_visited при открытии
- ✅ album.db рядом с exe, данные сохраняются между запусками
- ✅ Импорт ua.dat: кнопка "Import ua.dat" в тулбаре, `src/import.rs`; потоковый (`BufReader` + `read_until`), prepared statement, динамический `parent_stack`
- ✅ Неблокирующий импорт: фоновый поток (`std::thread`) + `invoke_from_event_loop`, прогресс в тулбаре ("Импортировано N..."), кнопка блокируется на время импорта, `busy_timeout=5000` на обоих соединениях
- ✅ Автосброс статуса импорта через 5 сек (`Timer::single_shot` + `mem::forget` — живёт до срабатывания)
- ✅ 12 тестов (`cargo test`): 8 DB + 4 import (decode, date, url, real file)
- ✅ Иконки папок: PNG из url-album-2 (`assets/folder-closed.png`, `folder-open.png`), `image-rendering: pixelated`
- ✅ Перетаскиваемый разделитель панелей (`tree-width` 120–600px, `mouse-cursor: col-resize`); панели без рамок
- ✅ Нативный MenuBar: Файл (Импорт — подменю) / Ссылки / Поиск / Вид; рабочие пункты активны, будущие — `enabled: false`; вложенные Menu поддерживаются Slint 1.16
- ✅ Правая панель: колонки "Название | Адрес" с заголовками и перетаскиваемым разделителем (`col-name-width`, clamp 80px … right-panel.width−100px); статусбар окна "Записей: N | База: album.db"
- ✅ Иконки тулбара: SVG из url-album-2 (`assets/new-folder.svg`, `new-link.svg`, `delete.svg`, `import.svg`), `stroke="#444444"` (currentColor→фикс для resvg)

## Очередь фич (приоритет)

1. **Favicon** — логику взять из старого URL-Album-3 как справочника
2. **Открытие в браузере из списка** — двойной клик по ссылке в правой панели (сейчас только карточка)
3. **Скриншоты** — Edge/Chrome Win10/11, `--headless=new` + `--user-data-dir`
4. **Drag&drop, поиск** (Ctrl+F), sort_idx для порядка
5. **Импорт браузерных закладок** — JSON/HTML
6. **Диалог выбора файла для импорта** — сейчас путь захардкожен (`ua.dat`); нужен нативный file-picker или хотя бы поле ввода пути
7. **Задвоение при повторном импорте** — повторное нажатие "Import ua.dat" добавляет данные поверх существующих; нужна проверка (очистка таблицы перед импортом или дедупликация)

## Меню — план (по образцу url-album-3)

Slint 1.16 имеет нативный `MenuBar` (core-элемент, добавлен в 1.10.0).
MenuBar реализован (2026-06-01): меню Файл / Ссылки / Поиск / Вид. Перенос — подменю внутри Файл.
✅ = уже реализовано, остальное — будущие фичи.

### Файл
| Пункт | Шорткат | Статус |
|---|---|---|
| Импорт ► (подменю) | | ✅ (ua.dat активен; браузер/HTML/TXT/экспорт — disabled) |
| — | | |
| Создать базу данных... | | |
| Открыть базу данных... | | |
| Последние базы... | | |
| Закрыть базу | | |
| — | | |
| Резервная копия... | | |
| — | | |
| Свойства базы данных | | |
| — | | |
| Настройки... | | |
| — | | |
| Выход | Alt+F4 | |

### Ссылки
| Пункт | Шорткат | Статус |
|---|---|---|
| Новая папка | | ✅ (меню + тулбар + ПКМ) |
| Новая ссылка | Ctrl+N | ✅ (меню + тулбар + ПКМ) |
| Переименовать | F2 | ✅ (через Свойства) |
| Удалить | Del | ✅ (меню + ПКМ) |
| — | | |
| Открыть | Enter | ✅ (меню + двойной клик в дереве + ПКМ) |
| Открыть с помощью... | | |
| — | | |
| Проверить ссылки | | |
| Найти дубликаты | | |
| — | | |
| Обновить favicon'ы | | |
| — | | |
| Копировать URL | Ctrl+C | |
| Свойства | F4 | ✅ (меню + ПКМ) |

### Перенос
| Пункт | Статус |
|---|---|
| Импорт из браузера... | |
| Импорт из HTML... | |
| Импорт из TXT... | |
| Импорт из ua.dat... | ✅ (меню Файл→Импорт + кнопка тулбара, фоновый поток) |
| — | |
| Экспорт в HTML... | |
| Экспорт в TXT... | |

### Поиск
| Пункт | Шорткат | Статус |
|---|---|---|
| Найти... | Ctrl+F | |

### Вид
| Пункт | Статус |
|---|---|
| Развернуть все папки | |
| Свернуть все папки | |
| — | |
| Все ссылки | |
| — | |
| Скрыть/Показать тулбар | |
| Светлая/Тёмная тема | |
| — | |
| Настроить toolbar... | |

## Импорт ua.dat — формат

**Файл:** `C:\Projects\url-album-2\ua.dat` (только читать). `ua.dat.bak` — оригинал 2024-06-06.

**Тип:** текстовый, Windows-1251, CRLF. ~573 строк: 55 папок, 517 ссылок, глубина до 4, 7 корневых папок.

**Поля через TAB (7 полей, индексы 0–6):**

| Индекс | Папка | Ссылка |
|---|---|---|
| `[0]` | title | title (если нет имени — совпадает с URL) |
| `[1]` | `#` (маркер папки) | URL |
| `[2]` | пусто | имя файла превью `YYMMDDHHMMSS.png` или пусто |
| `[3]` | note или пусто | note или пусто |
| `[4]` | created `DDMMYYHHMMSS` или пусто | created |
| `[5]` | visited или пусто | visited или пусто |
| `[6]` | `0` (флаг) | `0` |

**Иерархия:** число ведущих табов = глубина в дереве (`depth=0` — корень, `depth=1` — дочерний и т.д.).

**Нюансы:**
- `^^` в note = перенос строки
- URL бывает без схемы (`www.example.com` без `http://`)
- Дата: `DDMMYYHHMMSS` — `150905172253` = 15 сен 2005, 17:22:53
- Папки тоже могут иметь note

**Алгоритм парсера (стек родителей):**
```rust
let mut parent_stack: Vec<Option<i64>> = vec![None]; // растёт динамически

loop {
    // read_until(b'\n') → срезать CRLF байтами → decode_win1251
    let depth = buf.iter().take_while(|&&b| b == b'\t').count();
    let line = decode_win1251(&buf[depth..]);
    let parts: Vec<&str> = line.split('\t').collect();
    let is_folder = parts[1] == "#";
    while parent_stack.len() <= depth { parent_stack.push(None); }
    let parent = parent_stack[depth];           // parent на уровне depth
    let id = stmt.execute(...); conn.last_insert_rowid();
    parent_stack.truncate(depth + 1);          // обрезать глубже текущего
    parent_stack.push(Some(id));               // parent_stack[depth+1] = id
}
```

**Реализация (`src/import.rs`, 2026-05-31):**
- `WIN1251: [u32; 128]` — статическая таблица Win-1251→Unicode, байты 0x80..=0xFF
- `decode_win1251(bytes)` — байты в UTF-8 String через таблицу
- `parse_ua_date("150905172253")` → `"2005-09-15 17:22:53"` (DDMMYYHHMMSS → ISO)
- `normalize_url("www.x.com")` → `"http://www.x.com"` (добавляет схему если нет `://`)
- `import_ua_dat(conn, path)` — потоковое чтение (`BufReader` + `read_until`), один `prepare` вне цикла, `parent_stack` с `truncate` (произвольная глубина), одна транзакция (BEGIN/COMMIT), возвращает `Result<usize, String>`
- Кнопка "Import ua.dat" в тулбаре → `on_import_ua_dat` в main.rs
- 4 теста: `decode_cyrillic`, `date_converts_correctly`, `url_normalization`, `import_real_ua_dat`
  — инварианты без чисел: count>0, total==count, folders>0, bookmarks>0, folders+bookmarks==total, root_folders>0, orphans==0, notes_with_newline>0

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
- `absolute-position` доступен в Slint 1.16 как свойство элемента типа `LogicalPosition`
  с полями `.x`/`.y` — используется для точной позиции курсора в ПКМ
- `webbrowser::open(url)` — ShellExecuteW на Windows, без shell-injection
- Highlight строк реактивный через `item.id == root.status-id` — refresh_contents
  при одиночном клике НЕ нужен
- Контекст создания узла (kind, parent) хранится в `prop-is-folder` + `state.current_folder`,
  новых полей State не требует — диалог модальный, навигация заблокирована
