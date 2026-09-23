use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::dto::{HistoryEntry, RunRequest};

static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(1);
const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
const SQLITE_OPEN_FULLMUTEX: c_int = 0x0001_0000;

pub struct History {
    path: PathBuf,
}

impl History {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let history = Self {
            path: path.to_owned(),
        };
        let database = Database::open(path)?;
        database.exec(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                created_at_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                target TEXT NOT NULL,
                status TEXT NOT NULL,
                request TEXT NOT NULL,
                result TEXT NOT NULL DEFAULT '',
                verification TEXT NOT NULL DEFAULT '',
                error TEXT NOT NULL DEFAULT ''
             );
             CREATE INDEX IF NOT EXISTS runs_created_at ON runs(created_at_ms DESC);
             UPDATE runs SET status = 'interrupted', error = 'Gate stopped before completion'
               WHERE status = 'running';",
        )?;
        Ok(history)
    }

    pub fn begin(&self, request: &RunRequest) -> anyhow::Result<String> {
        let now = now_ms();
        let sequence = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
        let id = format!("{now:x}-{sequence:x}");
        let request_json = serde_json::to_string(request)?;
        let database = Database::open(&self.path)?;
        let mut statement = database.prepare(
            "INSERT INTO runs(id, created_at_ms, kind, target, status, request)
             VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
        )?;
        statement.bind_text(1, &id)?;
        statement.bind_i64(2, now)?;
        statement.bind_text(3, &format!("{:?}", request.kind))?;
        statement.bind_text(4, &request.target)?;
        statement.bind_text(5, &request_json)?;
        statement.done()?;
        Ok(id)
    }

    pub fn fail(&self, id: &str, error: &str) -> anyhow::Result<()> {
        let database = Database::open(&self.path)?;
        let mut statement =
            database.prepare("UPDATE runs SET status = 'failed', error = ?2 WHERE id = ?1")?;
        statement.bind_text(1, id)?;
        statement.bind_text(2, error)?;
        statement.done()
    }

    pub fn append_result(&self, id: &str, value: &str) -> anyhow::Result<()> {
        let database = Database::open(&self.path)?;
        let mut statement =
            database.prepare("UPDATE runs SET result = result || ?2 || char(10) WHERE id = ?1")?;
        statement.bind_text(1, id)?;
        statement.bind_text(2, value)?;
        statement.done()
    }

    pub fn complete(&self, id: &str, verification: &str) -> anyhow::Result<()> {
        let database = Database::open(&self.path)?;
        let mut statement = database
            .prepare("UPDATE runs SET status = 'complete', verification = ?2 WHERE id = ?1")?;
        statement.bind_text(1, id)?;
        statement.bind_text(2, verification)?;
        statement.done()
    }

    pub fn list(&self, limit: u32) -> anyhow::Result<Vec<HistoryEntry>> {
        let database = Database::open(&self.path)?;
        let mut statement = database.prepare(
            "SELECT id, created_at_ms, kind, target, status, request, result, verification, error
             FROM runs ORDER BY created_at_ms DESC LIMIT ?1",
        )?;
        statement.bind_i64(1, i64::from(limit.min(500)))?;
        let mut entries = Vec::new();
        while statement.row()? {
            entries.push(HistoryEntry {
                id: statement.text(0),
                created_at_ms: statement.i64(1),
                kind: statement.text(2),
                target: statement.text(3),
                status: statement.text(4),
                request: statement.text(5),
                result: statement.text(6),
                verification: statement.text(7),
                error: statement.text(8),
            });
        }
        Ok(entries)
    }

    pub fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let database = Database::open(&self.path)?;
        let mut statement = database.prepare("DELETE FROM runs WHERE id = ?1")?;
        statement.bind_text(1, id)?;
        statement.done()?;
        Ok(database.changes() > 0)
    }

    pub fn clear(&self) -> anyhow::Result<usize> {
        let database = Database::open(&self.path)?;
        database.exec("DELETE FROM runs")?;
        Ok(database.changes().try_into().unwrap_or(usize::MAX))
    }
}

struct Database(*mut Sqlite3);

impl Database {
    fn open(path: &Path) -> anyhow::Result<Self> {
        let path = CString::new(path.to_string_lossy().as_bytes())?;
        let mut handle = ptr::null_mut();
        let result = unsafe {
            sqlite3_open_v2(
                path.as_ptr(),
                &raw mut handle,
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX,
                ptr::null(),
            )
        };
        if result != SQLITE_OK {
            let message = sqlite_error(handle);
            if !handle.is_null() {
                unsafe { sqlite3_close(handle) };
            }
            anyhow::bail!("opening history database failed: {message}");
        }
        unsafe { sqlite3_busy_timeout(handle, 2_000) };
        Ok(Self(handle))
    }

    fn exec(&self, sql: &str) -> anyhow::Result<()> {
        let sql = CString::new(sql)?;
        let mut error = ptr::null_mut();
        let result =
            unsafe { sqlite3_exec(self.0, sql.as_ptr(), None, ptr::null_mut(), &raw mut error) };
        if result == SQLITE_OK {
            return Ok(());
        }
        let message = if error.is_null() {
            sqlite_error(self.0)
        } else {
            let message = unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            unsafe { sqlite3_free(error.cast()) };
            message
        };
        anyhow::bail!("history database command failed: {message}")
    }

    fn prepare<'a>(&'a self, sql: &str) -> anyhow::Result<Statement<'a>> {
        let sql = CString::new(sql)?;
        let mut statement = ptr::null_mut();
        let result = unsafe {
            sqlite3_prepare_v2(
                self.0,
                sql.as_ptr(),
                -1,
                &raw mut statement,
                ptr::null_mut(),
            )
        };
        if result != SQLITE_OK {
            anyhow::bail!("preparing history query failed: {}", sqlite_error(self.0));
        }
        Ok(Statement {
            database: self,
            statement,
        })
    }

    fn changes(&self) -> c_int {
        unsafe { sqlite3_changes(self.0) }
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        unsafe { sqlite3_close(self.0) };
    }
}

struct Statement<'a> {
    database: &'a Database,
    statement: *mut Sqlite3Statement,
}

impl Statement<'_> {
    fn bind_text(&mut self, index: c_int, value: &str) -> anyhow::Result<()> {
        let bytes = value.as_bytes();
        let result = unsafe {
            sqlite3_bind_text(
                self.statement,
                index,
                bytes.as_ptr().cast(),
                bytes.len().try_into()?,
                sqlite_transient(),
            )
        };
        self.check(result, "binding history text")
    }

    fn bind_i64(&mut self, index: c_int, value: i64) -> anyhow::Result<()> {
        let result = unsafe { sqlite3_bind_int64(self.statement, index, value) };
        self.check(result, "binding history integer")
    }

    fn done(&mut self) -> anyhow::Result<()> {
        let result = unsafe { sqlite3_step(self.statement) };
        if result == SQLITE_DONE {
            Ok(())
        } else {
            anyhow::bail!("history mutation failed: {}", sqlite_error(self.database.0))
        }
    }

    fn row(&mut self) -> anyhow::Result<bool> {
        match unsafe { sqlite3_step(self.statement) } {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            _ => anyhow::bail!("history query failed: {}", sqlite_error(self.database.0)),
        }
    }

    fn text(&self, column: c_int) -> String {
        let value = unsafe { sqlite3_column_text(self.statement, column) };
        if value.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(value.cast()) }
                .to_string_lossy()
                .into_owned()
        }
    }

    fn i64(&self, column: c_int) -> i64 {
        unsafe { sqlite3_column_int64(self.statement, column) }
    }

    fn check(&self, result: c_int, action: &str) -> anyhow::Result<()> {
        if result == SQLITE_OK {
            Ok(())
        } else {
            anyhow::bail!("{action} failed: {}", sqlite_error(self.database.0))
        }
    }
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        unsafe { sqlite3_finalize(self.statement) };
    }
}

fn sqlite_transient() -> Option<unsafe extern "C" fn(*mut c_void)> {
    // SQLite reserves the all-ones destructor value to mean "copy now".
    unsafe { std::mem::transmute::<isize, _>(-1) }
}

fn sqlite_error(database: *mut Sqlite3) -> String {
    if database.is_null() {
        return "unknown SQLite error".into();
    }
    unsafe { CStr::from_ptr(sqlite3_errmsg(database)) }
        .to_string_lossy()
        .into_owned()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[repr(C)]
struct Sqlite3 {
    _private: [u8; 0],
}

#[repr(C)]
struct Sqlite3Statement {
    _private: [u8; 0],
}

// Windows links the bundled SQLite that libsqlite3-sys compiles in.
#[cfg(windows)]
use libsqlite3_sys as _;

#[cfg_attr(not(windows), link(name = "sqlite3"))]
unsafe extern "C" {
    fn sqlite3_open_v2(
        filename: *const c_char,
        database: *mut *mut Sqlite3,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close(database: *mut Sqlite3) -> c_int;
    fn sqlite3_busy_timeout(database: *mut Sqlite3, milliseconds: c_int) -> c_int;
    fn sqlite3_errmsg(database: *mut Sqlite3) -> *const c_char;
    fn sqlite3_exec(
        database: *mut Sqlite3,
        sql: *const c_char,
        callback: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        context: *mut c_void,
        error: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_free(value: *mut c_void);
    fn sqlite3_prepare_v2(
        database: *mut Sqlite3,
        sql: *const c_char,
        sql_bytes: c_int,
        statement: *mut *mut Sqlite3Statement,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(statement: *mut Sqlite3Statement) -> c_int;
    fn sqlite3_bind_text(
        statement: *mut Sqlite3Statement,
        index: c_int,
        value: *const c_char,
        bytes: c_int,
        destructor: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int;
    fn sqlite3_bind_int64(statement: *mut Sqlite3Statement, index: c_int, value: i64) -> c_int;
    fn sqlite3_step(statement: *mut Sqlite3Statement) -> c_int;
    fn sqlite3_column_text(statement: *mut Sqlite3Statement, column: c_int) -> *const u8;
    fn sqlite3_column_int64(statement: *mut Sqlite3Statement, column: c_int) -> i64;
    fn sqlite3_changes(database: *mut Sqlite3) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{AssuranceInput, RunKind};

    fn request() -> RunRequest {
        RunRequest {
            kind: RunKind::Fetch,
            target: "provider-node".into(),
            node_addresses: Vec::new(),
            input: r#"{"model":"test","input":"hello"}"#.into(),
            trust_anchor: "genesis".into(),
            service: "openai".into(),
            method: "responses".into(),
            execution_environment: "environment".into(),
            assurance: AssuranceInput::ProducerSigned,
            apple_app_id: String::new(),
            apple_cd_hashes: Vec::new(),
        }
    }

    #[test]
    fn records_completion_and_supports_private_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let history = History::open(&directory.path().join("history.sqlite3")).unwrap();
        let id = history.begin(&request()).unwrap();
        history.append_result(&id, "chunk").unwrap();
        history.complete(&id, "verified").unwrap();

        let entries = history.list(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "complete");
        assert_eq!(entries[0].result, "chunk\n");
        assert_eq!(entries[0].verification, "verified");
        assert!(entries[0].request.contains("trustAnchor"));
        assert!(history.delete(&id).unwrap());
        assert!(history.list(10).unwrap().is_empty());
    }

    #[test]
    fn marks_incomplete_runs_interrupted_on_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite3");
        let history = History::open(&path).unwrap();
        let id = history.begin(&request()).unwrap();
        drop(history);

        let reopened = History::open(&path).unwrap();
        let entry = reopened.list(1).unwrap().pop().unwrap();
        assert_eq!(entry.id, id);
        assert_eq!(entry.status, "interrupted");
        assert_eq!(entry.error, "Gate stopped before completion");
        assert_eq!(reopened.clear().unwrap(), 1);
    }
}
