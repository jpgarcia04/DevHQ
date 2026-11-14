//! What runs inside `devhq --read-installation`: one file read as data, in a process of its own
//! that first forbids itself to start programs (see `local_copies::confine`).
//!
//! It reads one JSON object from stdin, `{"path": "…"}`, and answers on stdout with one JSON line:
//! `installation`, with what the file was, or `error`, a failure of DevHQ itself (it could not
//! confine itself, the request names no absolute path).
//!
//! The file is opened to read, once, and nothing else is: no neighbour of it, no folder, no
//! registry key, no other program's file. It is never run, mapped or loaded. Its bytes go through
//! SHA-256 whole, so the hash is of what was there, also when an installer was writing it; and the
//! few structures that lead to its version resource are read by `pe`. Where the system really
//! keeps the file is asked as DevHQ asks it of its own inventory (`inventory::stored_at`), so the
//! two agree on what a link, a junction or a packaged app's redirection is.

use super::pe::{self, Image};
use super::{plain_text, processes, Facts, MAX_DETAIL, MAX_STORED_AT};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The request is one short JSON object.
const MAX_REQUEST: u64 = 64 * 1024;

/// Whether the reader must forbid itself to start programs before reading. Only tests read
/// without it, to show that the reader itself starts none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Confinement {
    Required,
    #[cfg(test)]
    Skipped,
}

/// Serves one request from `input` on `output`; returns the process exit code.
pub(crate) fn serve(input: impl Read, mut output: impl Write, confinement: Confinement) -> i32 {
    let mut answer = |line: Value| -> bool {
        writeln!(output, "{line}")
            .and_then(|()| output.flush())
            .is_ok()
    };
    if confinement == Confinement::Required {
        if let Err(message) = crate::local_copies::confine::forbid_programs() {
            answer(json!({ "phase": "error", "message": message }));
            return 1;
        }
    }
    match request(input).and_then(|path| read(Path::new(&path))) {
        Ok(facts) => {
            let mut line = json!(facts);
            line["phase"] = json!("installation");
            i32::from(!answer(line))
        }
        Err(message) => {
            answer(json!({ "phase": "error", "message": message }));
            1
        }
    }
}

/// The path the request names.
fn request(input: impl Read) -> Result<String, String> {
    let mut text = String::new();
    input
        .take(MAX_REQUEST)
        .read_to_string(&mut text)
        .map_err(|error| format!("The reader could not read its request: {error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("The reader's request is not JSON: {error}"))?;
    value
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "The reader's request names no path.".to_owned())
}

fn not_read(outcome: &str, detail: &str, stored_at: Option<String>) -> Facts {
    Facts {
        outcome: outcome.to_owned(),
        detail: Some(plain_text(detail, MAX_DETAIL)),
        stored_at,
        ..Facts::default()
    }
}

/// The path as the system names its parts: Windows takes `/` for `\`, and the place it resolves a
/// file to always comes with `\`.
fn as_the_system_writes_it(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.display().to_string().replace('/', "\\"))
    } else {
        path.to_owned()
    }
}

/// The file at `path`, as far as it is an installation DevHQ can read. An error is a failure of
/// DevHQ, not a fact of the file.
pub(crate) fn read(path: &Path) -> Result<Facts, String> {
    if !path.is_absolute() {
        return Err(format!(
            "{} is no absolute path on this system, so DevHQ did not look for it.",
            path.display()
        ));
    }
    let path = as_the_system_writes_it(path);
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(not_read("missing", "Nothing is at this path.", None));
        }
        Err(error) => return Ok(not_read("unreadable", &error.to_string(), None)),
    };
    // Asked as DevHQ asks where its own inventory is, so that both say the same of a link, a
    // junction or the redirection of AppData for a process a packaged app started.
    let stored_at = crate::inventory::stored_at(&path)
        .map(|kept| plain_text(&kept, MAX_STORED_AT))
        .filter(|kept| !kept.is_empty());
    if metadata.is_dir() {
        return Ok(not_read(
            "not_an_executable",
            "This is a folder. Declare the path of the executable file itself.",
            stored_at,
        ));
    }
    if !metadata.is_file() {
        return Ok(not_read(
            "not_an_executable",
            "This is no regular file (a device, a pipe or a socket).",
            stored_at,
        ));
    }
    let unreadable = |error: &std::io::Error, stored_at: &Option<String>| {
        not_read("unreadable", &error.to_string(), stored_at.clone())
    };
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(error) => return Ok(unreadable(&error, &stored_at)),
    };
    // What was opened, which may no longer be what the path named a moment ago.
    let opened = match file.metadata() {
        Ok(opened) if opened.is_file() => opened,
        Ok(_) => {
            return Ok(not_read(
                "not_an_executable",
                "This is no regular file (a device, a pipe or a socket).",
                stored_at,
            ))
        }
        Err(error) => return Ok(unreadable(&error, &stored_at)),
    };
    let image = match pe::examine(&mut file, opened.len()) {
        Ok(image) => image,
        Err(error) => return Ok(unreadable(&error, &stored_at)),
    };
    let declared = match image {
        Image::Other(why) => return Ok(not_read("not_an_executable", &why, stored_at)),
        Image::Windows(declared) => declared,
        Image::Elf => Err(
            "An ELF executable has no version resource, so the file declares no version."
                .to_owned(),
        ),
    };
    // Its content, whole: what identifies the build, whatever version it declares.
    if let Err(error) = file.seek(SeekFrom::Start(0)) {
        return Ok(unreadable(&error, &stored_at));
    }
    let mut hash = Sha256::new();
    let mut size: u64 = 0;
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                hash.update(&buffer[..read]);
                size += read as u64;
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Ok(unreadable(&error, &stored_at)),
        }
    }
    let sha256: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let modified_at = opened
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_millis()).ok());
    // A process is named by the path it was started from, which may be the declared one or the
    // one the system resolves it to.
    let resolved = std::fs::canonicalize(&path)
        .ok()
        .map(|resolved| PathBuf::from(crate::inventory::plain(&resolved)));
    let mut names = vec![path.as_path()];
    names.extend(resolved.as_deref());
    let (processes, processes_unexamined, processes_unknown) = match processes::running_from(&names)
    {
        Ok(count) => (Some(count.running), Some(count.unexamined), None),
        Err(why) => (None, None, Some(plain_text(&why, MAX_DETAIL))),
    };
    let (declared, version_unknown) = match declared {
        Ok(declared) => (declared, None),
        Err(why) => (pe::Declared::default(), Some(plain_text(&why, MAX_DETAIL))),
    };
    Ok(Facts {
        outcome: "read".to_owned(),
        detail: None,
        stored_at,
        sha256: Some(sha256),
        size_bytes: Some(size),
        modified_at,
        product_version: declared.product_version,
        file_version: declared.file_version,
        product_name: declared.product_name,
        version_unknown,
        processes,
        processes_unexamined,
        processes_unknown,
    })
}
