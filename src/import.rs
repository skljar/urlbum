use std::fs::File;
use std::io::{BufRead, BufReader};
use rusqlite::{Connection, params};

// Windows-1251 → Unicode: байты 0x80..=0xFF
const WIN1251: [u32; 128] = [
    0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021,  // 0x80-0x87
    0x20AC, 0x2030, 0x0409, 0x2039, 0x040A, 0x040C, 0x040B, 0x040F,  // 0x88-0x8F
    0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,  // 0x90-0x97
    0x0000, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F,  // 0x98-0x9F
    0x00A0, 0x040E, 0x045E, 0x0408, 0x00A4, 0x0490, 0x00A6, 0x00A7,  // 0xA0-0xA7
    0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407,  // 0xA8-0xAF
    0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7,  // 0xB0-0xB7
    0x0451, 0x2116, 0x0454, 0x00BB, 0x0458, 0x0405, 0x0455, 0x0457,  // 0xB8-0xBF
    0x0410, 0x0411, 0x0412, 0x0413, 0x0414, 0x0415, 0x0416, 0x0417,  // 0xC0-0xC7 А-З
    0x0418, 0x0419, 0x041A, 0x041B, 0x041C, 0x041D, 0x041E, 0x041F,  // 0xC8-0xCF И-П
    0x0420, 0x0421, 0x0422, 0x0423, 0x0424, 0x0425, 0x0426, 0x0427,  // 0xD0-0xD7 Р-Ч
    0x0428, 0x0429, 0x042A, 0x042B, 0x042C, 0x042D, 0x042E, 0x042F,  // 0xD8-0xDF Ш-Я
    0x0430, 0x0431, 0x0432, 0x0433, 0x0434, 0x0435, 0x0436, 0x0437,  // 0xE0-0xE7 а-з
    0x0438, 0x0439, 0x043A, 0x043B, 0x043C, 0x043D, 0x043E, 0x043F,  // 0xE8-0xEF и-п
    0x0440, 0x0441, 0x0442, 0x0443, 0x0444, 0x0445, 0x0446, 0x0447,  // 0xF0-0xF7 р-ч
    0x0448, 0x0449, 0x044A, 0x044B, 0x044C, 0x044D, 0x044E, 0x044F,  // 0xF8-0xFF ш-я
];

fn decode_win1251(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        if b < 0x80 {
            s.push(b as char);
        } else {
            let cp = WIN1251[(b - 0x80) as usize];
            s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
        }
    }
    s
}

// "DDMMYYHHMMSS" → "YYYY-MM-DD HH:MM:SS"
fn parse_ua_date(s: &str) -> Option<String> {
    if s.len() != 12 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let dd = &s[0..2];
    let mm = &s[2..4];
    let yy: u32 = s[4..6].parse().ok()?;
    let hh = &s[6..8];
    let mi = &s[8..10];
    let ss = &s[10..12];
    Some(format!("{:04}-{}-{} {}:{}:{}", 2000 + yy, mm, dd, hh, mi, ss))
}

// www.example.com → http://www.example.com (если нет схемы)
fn normalize_url(u: &str) -> String {
    if u.is_empty() || u.contains("://") {
        u.to_string()
    } else {
        format!("http://{}", u)
    }
}

/// Импортирует ua.dat в таблицу nodes.
/// Возвращает число импортированных записей или описание ошибки.
pub fn import_ua_dat(conn: &Connection, path: &str) -> Result<usize, String> {
    let file = File::open(path)
        .map_err(|e| format!("Cannot open {path}: {e}"))?;
    let mut reader = BufReader::new(file);
    let mut buf: Vec<u8> = Vec::new();

    // Стек родителей: parent_stack[depth] = id родителя для узлов глубины depth
    let mut parent_stack: Vec<Option<i64>> = vec![None];
    let mut count = 0usize;

    conn.execute_batch("BEGIN TRANSACTION")
        .map_err(|e| format!("BEGIN failed: {e}"))?;

    let mut stmt = conn.prepare(
        "INSERT INTO nodes (parent, kind, title, url, thumb, note, created, visited)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    ).map_err(|e| format!("Prepare failed: {e}"))?;

    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)
            .map_err(|e| format!("Read error: {e}"))?;
        if n == 0 { break; }

        // Срезаем CRLF/LF байтами до декодирования (\n = 0x0A, \r = 0x0D — ASCII-safe)
        if buf.ends_with(b"\n") { buf.pop(); }
        if buf.ends_with(b"\r") { buf.pop(); }

        if buf.is_empty() { continue; }

        // Глубина = число ведущих табов (0x09)
        let depth = buf.iter().take_while(|&&b| b == b'\t').count();
        if depth == buf.len() { continue; } // строка из одних табов

        let line = decode_win1251(&buf[depth..]);
        let parts: Vec<&str> = line.split('\t').collect();
        let title = parts[0];
        if title.is_empty() { continue; }

        let field1 = parts.get(1).copied().unwrap_or("");
        let is_folder = field1 == "#";

        let url: Option<String> = if is_folder || field1.is_empty() {
            None
        } else {
            Some(normalize_url(field1))
        };

        let thumb: Option<&str> = parts.get(2).copied().filter(|s| !s.is_empty());

        let note: Option<String> = parts.get(3)
            .copied()
            .filter(|s| !s.is_empty())
            .map(|s| s.replace("^^", "\n"));

        let created: Option<String> = parts.get(4)
            .copied()
            .filter(|s| !s.is_empty())
            .and_then(parse_ua_date);

        let visited: Option<String> = parts.get(5)
            .copied()
            .filter(|s| !s.is_empty())
            .and_then(parse_ua_date);

        let kind = if is_folder { "folder" } else { "bookmark" };

        while parent_stack.len() <= depth { parent_stack.push(None); }
        let parent = parent_stack[depth];

        if let Err(e) = stmt.execute(
            params![parent, kind, title, url, thumb, note, created, visited],
        ) {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(format!("Insert failed at line {count}: {e}"));
        }

        parent_stack.truncate(depth + 1);
        parent_stack.push(Some(conn.last_insert_rowid()));
        count += 1;
        if count % 10_000 == 0 {
            eprintln!("Imported {count}...");
        }
    }

    conn.execute_batch("COMMIT")
        .map_err(|e| format!("COMMIT failed: {e}"))?;

    eprintln!("Imported {count} records total.");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

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
                created  TEXT,
                visited  TEXT,
                sort_idx INTEGER DEFAULT 0,
                favicon  TEXT
            );",
        ).unwrap();
        conn
    }

    #[test]
    fn decode_cyrillic() {
        // "Варез" в Win-1251: C2 E0 F0 E5 E7
        let bytes = [0xC2u8, 0xE0, 0xF0, 0xE5, 0xE7];
        assert_eq!(decode_win1251(&bytes), "Варез");
    }

    #[test]
    fn date_converts_correctly() {
        assert_eq!(
            parse_ua_date("150905172253"),
            Some("2005-09-15 17:22:53".to_string())
        );
        assert_eq!(parse_ua_date(""), None);
        assert_eq!(parse_ua_date("bad"), None);
    }

    #[test]
    fn url_normalization() {
        assert_eq!(normalize_url("www.example.com"), "http://www.example.com");
        assert_eq!(normalize_url("http://ya.ru"), "http://ya.ru");
        assert_eq!(normalize_url("ftp://files.ru"), "ftp://files.ru");
        assert_eq!(normalize_url(""), "");
    }

    #[test]
    fn import_real_ua_dat() {
        let path = r"C:\Projects\url-album-2\ua.dat";
        if !std::path::Path::new(path).exists() { return; }

        let conn = test_db();
        let count = import_ua_dat(&conn, path).expect("import failed");
        assert!(count > 0, "import produced no records");

        // Итого в БД совпадает с возвращённым count
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nodes", [], |r| r.get(0),
        ).unwrap();
        assert_eq!(total as usize, count, "returned count doesn't match DB total");

        // Есть и папки, и ссылки
        let folders: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE kind='folder'", [], |r| r.get(0),
        ).unwrap();
        let bookmarks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE kind='bookmark'", [], |r| r.get(0),
        ).unwrap();
        assert!(folders > 0,    "no folders imported");
        assert!(bookmarks > 0,  "no bookmarks imported");
        assert_eq!(folders + bookmarks, total, "kind must be 'folder' or 'bookmark'");

        // Корневые папки есть
        let root_folders: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE parent IS NULL AND kind='folder'",
            [], |r| r.get(0),
        ).unwrap();
        assert!(root_folders > 0, "no root folders found");

        // Нет висячих parent-ссылок (целостность дерева)
        let orphans: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nodes n
             WHERE n.parent IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM nodes p WHERE p.id = n.parent)",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(orphans, 0, "orphan nodes found — parent_stack bug: {orphans}");

        // ^^ в note правильно конвертирован в \n
        let notes_newline: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE note LIKE '%\n%'",
            [], |r| r.get(0),
        ).unwrap();
        assert!(notes_newline > 0, "no notes with newlines — ^^ conversion failed");
    }
}
