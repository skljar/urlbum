# URLbum — CLAUDE.md

## Что это

Менеджер закладок (наследник классического URL Album, "духовный", не форк).
Чистый проект с нуля, заменяет URL-Album-3.

## Стек и таргет

- Rust + Slint 1.16 (winit backend, БЕЗ кастомного platform.rs)
- Target: x86_64-pc-windows-msvc, Windows 10/11 ТОЛЬКО
- SQLite через rusqlite 0.32 (bundled), crt-static → один portable exe ~16 МБ (arboard/rfd/serde_json добавили ~5 МБ)
- webbrowser 1.2 — открытие URL (ShellExecuteW, без shell-injection)
- НЕТ Win7/8 поддержки, НЕТ pe-patch, НЕТ compat.rs, НЕТ шимов
- Сборка: обычный cargo build

## Запуск и сборка

- **Канонический путь к exe:** `target\x86_64-pc-windows-msvc\debug\urlbum.exe` — из-за явного таргета в `.cargo/config.toml` сборки идут **не** в `target\debug\`, а в `target\x86_64-pc-windows-msvc\debug\`.
- **Запускать всегда с рабочей директорией на ту же папку:**
  ```powershell
  Start-Process "...\target\x86_64-pc-windows-msvc\debug\urlbum.exe" -WorkingDirectory "...\target\x86_64-pc-windows-msvc\debug"
  ```
- **Release без явной просьбы не собирать** — уходит в `target\x86_64-pc-windows-msvc\release\` (другая папка, легко перепутать сборки).
- **База данных в проекте не хранится** — она у пользователя отдельно, открывается через меню "Файл → Открыть базу данных...". При первом старте программа может создать пустую `album.db` рядом с exe — это не рабочая база.
- **Для теста:** debug-сборка + наполненная база, открытая через меню.

## Структура

- `src/main.rs` — State (Rc<RefCell>), UI callbacks, refresh функции
- `src/db.rs` — SQLite CRUD, единая таблица nodes
- `src/favicon.rs` — fetch_favicon (4-step fallback), dedup_by_domain, pure helpers
- `src/import.rs` — потоковый парсер ua.dat (Win-1251, parent_stack)
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
- `get_bookmarks_recursive(conn, folder_id)` — WITH RECURSIVE, только kind='bookmark'
- `get_favicons(conn)` — `HashMap<i64, String>`, только строки с непустым favicon
- `set_favicon(conn, id, filename)` — UPDATE nodes SET favicon=?1 WHERE id=?2
- `get_all_folder_ids(conn)` — `Vec<i64>`, все папки (для expand-all)
- `count_nodes(conn)` — `(i64, i64)` = (папок, ссылок), для диалога Свойства
- `backup(conn, dest_path)` — VACUUM INTO (SQLite 3.27+)

## State (main.rs)

```rust
struct State {
    db:                 rusqlite::Connection,
    db_path:            String,              // путь к текущей БД; берётся воркерами перед spawn
    data_dir:           PathBuf,             // директория БД; favicons → data_dir/favicons/
    exe_dir:            PathBuf,             // директория exe; recent_dbs.json живёт здесь
    selected_id:        Option<i64>,
    selected_is_folder: bool,
    current_folder:     Option<i64>,
    status_id:          Option<i64>,
    expanded:           HashSet<i64>,
}
// impl State { fn favicons_dir(&self) -> PathBuf { self.data_dir.join("favicons") } }
```

**Хелперы смены базы:**
- `reset_ui_state(&mut State, &AppWindow)` — очищает все поля выделения и expanded, вызывает refresh_tree + refresh_contents
- `apply_recent_dbs(&Path, &AppWindow)` — читает recent_dbs.json, строит Vec<RecentDb>, устанавливает в Slint-модель
- `load_recent_dbs(exe_dir)` / `push_recent_db(exe_dir, path)` — JSON read/write через serde_json, max 10 записей

**Важно:** воркеры фавиконов и импорта берут `s.db_path.clone()` в начале callback (до spawn), а не захватывают путь при создании замыкания. Иначе после смены базы воркеры пишут в старый файл.

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
| Папка | Новая папка \| Новая ссылка \| — \| Обновить favicon'ы \| — \| Свойства \| Удалить |
| Ссылка | Открыть \| — \| Загрузить favicon \| — \| Свойства \| Удалить |

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
| Двойной клик на ссылку | карточка + открыть в браузере + touch_visited |

## Режимы правой панели

1. **Список** (`show-card=false`): содержимое current_folder (папки + ссылки). Внизу статусбар при выбранной ссылке.
2. **Карточка** (`show-card=true`): заголовок крупно + URL + заглушка-превью. Свойства — через ПКМ.

## Что готово (2026-06-02)

- ✅ Смешанное дерево папок и ссылок (depth-отступ, раздельные зоны клика: значок/название)
- ✅ Список содержимого папки (папки + ссылки вместе, single/double click)
- ✅ Статусбар: URL, note, created, visited при одиночном клике на ссылку в списке
- ✅ Карточка ссылки: заголовок + URL + заглушка превью
- ✅ Диалог свойств: создание (prop-is-new=true) и редактирование (false)
- ✅ CRUD полный: создать папку/ссылку (через диалог), удалить (рекурсивно), изменить
- ✅ Контекстное меню (ПКМ) в дереве и списке, позиция у курсора
- ✅ Открытие ссылки в браузере: двойной клик в дереве, двойной клик в правом списке, ПКМ → Открыть. Единый хелпер `activate_bookmark(&Connection, &AppWindow)`. URL без схемы → `normalize_url` добавляет `https://` (исключения: `mailto:`, `file:`)
- ✅ touch_visited при открытии
- ✅ album.db рядом с exe, данные сохраняются между запусками
- ✅ Импорт ua.dat: кнопка "Import ua.dat" в тулбаре, `src/import.rs`; потоковый (`BufReader` + `read_until`), prepared statement, динамический `parent_stack`
- ✅ Неблокирующий импорт: фоновый поток (`std::thread`) + `invoke_from_event_loop`, прогресс в тулбаре ("Импортировано N..."), кнопка блокируется на время импорта, `busy_timeout=5000` на обоих соединениях
- ✅ Автосброс статуса импорта через 5 сек (`Timer::single_shot` + `mem::forget` — живёт до срабатывания)
- ✅ Иконки папок: PNG из url-album-2 (`assets/folder-closed.png`, `folder-open.png`), `image-rendering: pixelated`
- ✅ Перетаскиваемый разделитель панелей (`tree-width` 120–600px, `mouse-cursor: col-resize`); панели без рамок
- ✅ Нативный MenuBar: Файл / Ссылки / Поиск / Вид; рабочие пункты активны
- ✅ Правая панель: колонки "Название | Адрес" с заголовками и перетаскиваемым разделителем; статусбар окна "Записей: N | База: album.db"
- ✅ Иконки тулбара SVG, компонент `TBtn` (hover + tooltip)
- ✅ Favicon: 4-step fallback, 5 воркеров batch + single, `slint::Timer` 200 мс rebuild
- ✅ 25 тестов (`cargo test`): DB + import + favicon
- ✅ Меню: Выход (`quit_event_loop`), Копировать URL (`arboard`), Развернуть/Свернуть все папки (`db::get_all_folder_ids`), Резервная копия (`rfd` save + `VACUUM INTO`)
- ✅ Группа "База данных": Открыть базу (`pick_file` → new Connection), Создать базу (удалить файл + `db::open`), Закрыть базу (`:memory:`), Последние базы (JSON `recent_dbs.json` рядом с exe, подменю `for db in recent-dbs`), Свойства базы (путь + размер + кол-во папок/ссылок)
- ✅ Поиск (Поиск → Найти...): `db::search` LIKE по title/url, строка поиска под тулбаром, режим `search-visible` в правой панели, `×` закрывает, сброс при смене базы
- ✅ Найти дубликаты (Поиск → Найти дубликаты): полноценный диалог 820×500 — левая панель "Группы URL" с счётчиками, правая "Дубликаты (N)" с колонками Название/URL/Папка/Дата; перетягиваемые splitter'ы панелей и колонок; кнопки: "Удалить выбранный", "Оставить одну ссылку" (keep selected/min id + подтверждение), "Удалить все дубликаты" (`db::delete_all_duplicates` MIN(id) по url + подтверждение), "Закрыть"

**Favicon-воркеры при смене базы (исправлен):** `State.favicon_cancel: Arc<AtomicBool>` — устанавливается в `true` при открытии/создании/закрытии/recent базы, после чего пересоздаётся в `false`. Воркеры batch (5 штук) проверяют флаг после `pop()` и после `fetch_favicon`; single-воркер — перед записью. Последний воркер пропускает `invoke_favicon_done` если флаг взведён.

**Известный недостаток (не исправлен):** каждый вызов `on_ctx_load_favicons_folder` создаёт новый `slint::Timer` через `mem::forget` — N запусков favicon-batch накапливают N вечных idle-таймеров. Каждый делает одну атомарную проверку каждые 200 мс — дёшево, но растёт. Решение: вынести `favicon_needs_rebuild`, `favicon_done`, `favicon_total` в `State` и создать один таймер в `main`. Отдельная задача.

## Очередь фич (приоритет)

Следующие группы меню (от простого к сложному — см. plan в memory/project_state.md):

1. **Поиск и обработка**: ✅ Найти (Ctrl+F); ✅ Найти дубликаты; Проверить ссылки (фоновый `ureq` HEAD)
2. **Импорт/экспорт**: HTML, Chrome/Firefox JSON, TXT — `db.rs` из ua-3 + `rfd` диалоги выбора файла
3. **Задвоение при повторном импорте** — очистка таблицы перед импортом или дедупликация
4. **Скриншоты** — Edge/Chrome Win10/11, `--headless=new` + `--user-data-dir`
5. **Drag&drop, sort_idx** для ручного порядка
6. **Тёмная тема / тулбар-toggle** (Slint CSS-переменные)

## Меню — план (по образцу url-album-3)

Slint 1.16 имеет нативный `MenuBar` (core-элемент, добавлен в 1.10.0).
MenuBar реализован (2026-06-01): меню Файл / Ссылки / Поиск / Вид. Перенос — подменю внутри Файл.
✅ = уже реализовано, остальное — будущие фичи.

### Файл
| Пункт | Шорткат | Статус |
|---|---|---|
| Импорт ► (подменю) | | ✅ ua.dat активен; остальное disabled |
| — | | |
| Создать базу данных... | | ✅ rfd save-диалог, удаляет файл перед db::open |
| Открыть базу данных... | | ✅ rfd pick_file, пересоздаёт State.db |
| Последние базы ► | | ✅ recent_dbs.json, подменю for в Slint |
| Закрыть базу | | ✅ переключает на :memory: |
| — | | |
| Резервная копия... | | ✅ rfd save + VACUUM INTO |
| — | | |
| Свойства базы данных | | ✅ диалог: путь, размер, папок/ссылок |
| — | | |
| Настройки... | | disabled |
| — | | |
| Выход | Alt+F4 | ✅ quit_event_loop |

### Ссылки
| Пункт | Шорткат | Статус |
|---|---|---|
| Новая папка | | ✅ меню + тулбар + ПКМ |
| Новая ссылка | Ctrl+N | ✅ меню + тулбар + ПКМ |
| Переименовать | F2 | ✅ через Свойства |
| Удалить | Del | ✅ меню + ПКМ |
| — | | |
| Открыть | Enter | ✅ меню + двойной клик дерево/список + ПКМ |
| Открыть с помощью... | | disabled |
| — | | |
| Проверить ссылки | | |
| Найти дубликаты | | |
| — | | |
| Обновить favicon'ы | | ✅ ПКМ папки → рекурсивно |
| — | | |
| Копировать URL | Ctrl+C | ✅ arboard |
| Свойства | F4 | ✅ меню + ПКМ |

### Перенос
| Пункт | Статус |
|---|---|
| Импорт из браузера... | disabled |
| Импорт из HTML... | disabled |
| Импорт из TXT... | disabled |
| Импорт из ua.dat... | ✅ меню Файл→Импорт + кнопка тулбара, фоновый поток |
| — | |
| Экспорт в HTML... | disabled |
| Экспорт в TXT... | disabled |

### Поиск
| Пункт | Шорткат | Статус |
|---|---|---|
| Найти... | Ctrl+F | ✅ строка под тулбаром, db::search LIKE |
| Найти дубликаты | | ✅ диалог: группы + копии + удаление |

### Вид
| Пункт | Статус |
|---|---|
| Развернуть все папки | ✅ get_all_folder_ids → expanded |
| Свернуть все папки | ✅ expanded.clear() |
| — | |
| Все ссылки | disabled |
| — | |
| Скрыть/Показать тулбар | disabled |
| Светлая/Тёмная тема | disabled |
| — | |
| Настроить toolbar... | disabled |

## Favicon — архитектура

**Файлы:** `target/.../favicons/{domain}.png` рядом с `album.db`. В БД хранится только имя файла.

**`src/favicon.rs`:**
- `fetch_favicon(url, favicons_dir) -> Option<String>` — 4-step:
  1. `https://{domain}/favicon.ico` → `http://...` fallback
  2. HTML `<link rel="icon">` (raster first, SVG last)
  3. DuckDuckGo `https://icons.duckduckgo.com/ip3/{domain}.ico`
  4. Google S2 `?domain={domain}&sz=32` — отклоняет placeholder ≤ 68 байт
- `prepare_image(bytes)` — decode (`image` crate) → re-encode PNG; SVG отклоняется (Slint грузит растр)
- `is_valid_image(bytes)` — PNG magic bytes `\x89PNG` (кэш всегда PNG)
- `dedup_by_domain(nodes)` — один fetch на домен, остальные id в `same_ids`

**Механизм обновления (идентично url-album-3, адаптировано под winit):**
- Batch (ПКМ папки): 5 воркеров → `set_favicon(conn)` → `needs_rebuild.store(true)` → `slint::Timer` @ 200 мс проверяет флаг → `refresh_tree + refresh_contents` (полный пересброс обеих моделей). Последний воркер → `invoke_from_event_loop` → `invoke_favicon_done` → финальный refresh + очистка статуса через 3 с.
- Single (ПКМ ссылки): 1 поток → `invoke_from_event_loop` → `invoke_favicon_done`.
- url-album-3 использует `platform::set_frame_callback()` (кастомный Win32 бэкенд) — urlbum заменяет на `slint::Timer::default()` (стандартный winit).

**`TreeItem` в Slint:** добавлены `favicon: image, has_favicon: bool`. Нет favicon → показывает `new-link.svg`.

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
- Favicon воркеры открывают свой `rusqlite::Connection` (не State.db) — то же что импорт
- `slint::Timer` в favicon-batch: `mem::forget(timer)` — таймер живёт, idle после завершения (дёшево)
- `slint::Image::load_from_path(&path)` — загрузка PNG в `Image` для `TreeItem.favicon`
- `normalize_url(url)` — добавляет `https://` если нет `://` и не начинается с `mailto:` / `file:`
- `activate_bookmark(id, &Connection, &AppWindow)` — единый хелпер: нормализует URL, ставит карточку, открывает браузер, touch_visited. Используется из `on_tree_item_activated`, `on_list_item_activated`, `on_ctx_open_browser` (последний без карточки)
- rfd 0.15: метод `pick_file()` (НЕ `open_file()`), `save_file()` — без изменений
- Смена State.db: внутри одного `borrow_mut`, без вложенных захватов. `s.db = new_conn` дропает старое соединение
- `recent_dbs.json` рядом с exe (s.exe_dir), не рядом с db (s.data_dir)
- Слint dynamic menu: `for db in root.recent-dbs: MenuItem { ... }` работает в Slint 1.16
- Crates: добавлены `arboard = "3"`, `rfd = "0.15"`, `serde_json = "1"`
