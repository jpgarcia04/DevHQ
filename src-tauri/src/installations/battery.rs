//! The hostile battery of the installation reader: files made to break a reader of executables.
//!
//! The declared path is untrusted, and so is the file there: a reader that trusted one number of
//! it could be led to read past a buffer, to loop for ever or to allocate without end. Each file
//! here gets one thing wrong on purpose (resources that leave the file, tables that point back at
//! themselves, counts and sizes at their maximum, strings without an end or of tens of thousands
//! of characters, a file that is no executable at all), and then thousands more are derived by
//! cutting a good file at every length and by overwriting bytes of it at random. The reader must
//! answer each one, never panic, never read more of the file than its limits allow, and, as the
//! real reader in its confined process, never change the file or anything beside it.
//!
//! Every file is synthetic and holds no code: nothing here could run even if something tried.

use super::pe::tests::{examined, image, utf16, version_data, version_node, Layout, RESOURCES};
use super::pe::{examine, Declared, Image};
use super::tests::{folder, path_text, sha256_of, test_reader};
use super::DEADLINE;
use std::io::{Cursor, Read, Seek, SeekFrom};

/// What the reader must say of a file.
#[derive(Debug, Clone, Copy)]
enum Expected {
    /// A Windows executable whose version it reads.
    Declares,
    /// A Windows executable whose version is unknown, for a reason that holds this text.
    Unknown(&'static str),
    /// No executable, for a reason that holds this text.
    NotExecutable(&'static str),
}

const STRINGS: [(&str, &str); 3] = [
    ("ProductVersion", "1.4.0+3f9c2ab"),
    ("FileVersion", "1.4.0.0"),
    ("ProductName", "Atlas Notes"),
];

fn good() -> (Vec<u8>, Layout) {
    image(&version_data(&STRINGS), true)
}

/// A good image with the four bytes at `at` replaced.
fn with32(at: impl Fn(&Layout) -> usize, value: u32) -> Vec<u8> {
    let (mut file, layout) = good();
    let at = at(&layout);
    file[at..at + 4].copy_from_slice(&value.to_le_bytes());
    file
}

fn with16(at: impl Fn(&Layout) -> usize, value: u16) -> Vec<u8> {
    let (mut file, layout) = good();
    let at = at(&layout);
    file[at..at + 2].copy_from_slice(&value.to_le_bytes());
    file
}

/// An image whose version data is a root node with `children` under it.
fn with_children(children: &[Vec<u8>]) -> Vec<u8> {
    image(
        &version_node("VS_VERSION_INFO", &[0; 52], false, children),
        true,
    )
    .0
}

/// An image whose version data declares `strings` in one table.
fn with_strings(strings: &[Vec<u8>]) -> Vec<u8> {
    let table = version_node("040904B0", &[], true, strings);
    with_children(&[version_node("StringFileInfo", &[], true, &[table])])
}

fn string(key: &str, value: &[u8]) -> Vec<u8> {
    version_node(key, value, true, &[])
}

/// A deterministic stream of numbers: the battery is the same on every run.
struct Numbers(u64);

impl Numbers {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        usize::try_from(self.next() % limit as u64).unwrap()
    }
}

/// Each hostile file, by what it gets wrong.
fn hostile() -> Vec<(&'static str, Vec<u8>, Expected)> {
    use Expected::{Declares, NotExecutable, Unknown};
    // Places in a good image (PE32+): the resource directory's address and size in the optional
    // header, the one section's entry, and the tables and entries of the resources.
    let directory = |layout: &Layout| layout.optional + 112 + 16;
    let malformed = "resources are malformed";
    let broken = "version resource is malformed";
    let mut random = Numbers(0x5DEE_CE66_D1CE_4E5B);
    let mut noise = b"MZ".to_vec();
    noise.extend((0..4096).map(|_| random.next() as u8));
    let mut unending_key = vec![0; 6];
    unending_key.extend(utf16(&"A".repeat(16)));
    unending_key.truncate(6 + 32);
    unending_key[0] = 38;
    vec![
        (
            "an empty file",
            Vec::new(),
            NotExecutable("does not start as an executable"),
        ),
        (
            "a text file",
            b"@echo off\r\nstart atlas-notes.exe\r\n".to_vec(),
            NotExecutable("does not start as an executable"),
        ),
        (
            "two bytes of an executable",
            b"MZ".to_vec(),
            NotExecutable("too short"),
        ),
        ("random bytes behind an MZ", noise, NotExecutable("")),
        (
            "a PE header past the end of the file",
            with32(|_| 0x3C, 0xFFFF_FFF0),
            NotExecutable("past its end"),
        ),
        (
            "headers longer than the file",
            with16(|_| 0x80 + 4 + 16, 0xFFFF),
            NotExecutable("end past its end"),
        ),
        (
            "an optional header of no known kind",
            with16(|layout| layout.optional, 0x0107),
            NotExecutable("no kind DevHQ knows"),
        ),
        (
            "65535 sections",
            with16(|_| 0x80 + 4 + 2, 0xFFFF),
            Unknown("more sections than a Windows executable can have"),
        ),
        (
            "no resource directory",
            with32(directory, 0),
            Unknown("has no resources"),
        ),
        (
            "resources in no section",
            with32(directory, 0x7000_0000),
            Unknown("in no section the file holds"),
        ),
        (
            "resources that start past the end of the file",
            with32(|layout| layout.sections + 20, 0xFFFF_FF00),
            Unknown(malformed),
        ),
        // A directory or a section that claims more than there is reads what is really there.
        (
            "a resource directory of 4 GiB",
            with32(|layout| directory(layout) + 4, 0xFFFF_FFFF),
            Declares,
        ),
        (
            "a section of 4 GiB",
            with32(|layout| layout.sections + 16, 0xFFFF_FFFF),
            Declares,
        ),
        (
            "131070 entries in a table",
            {
                let (mut file, layout) = good();
                file[layout.resources + 12..layout.resources + 16].copy_from_slice(&[0xFF; 4]);
                file
            },
            Unknown(malformed),
        ),
        (
            "tables that point back at the first one",
            {
                let (mut file, layout) = good();
                for entry in [20, 28] {
                    let at = layout.resources + entry;
                    file[at..at + 4].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
                }
                file
            },
            Unknown(malformed),
        ),
        (
            "a table where the version data should be",
            with32(|layout| layout.resources + 0x4C, 0x8000_0038),
            Unknown(malformed),
        ),
        (
            "version data where a table should be",
            with32(|layout| layout.resources + 0x34, 0x50),
            Unknown(malformed),
        ),
        (
            "a table past the end of the resources",
            with32(|layout| layout.resources + 28, 0x8FFF_FFF0),
            Unknown(malformed),
        ),
        (
            "a data entry that is its own data",
            {
                let (mut file, layout) = good();
                let at = layout.resources + 0x50;
                file[at..at + 4].copy_from_slice(&(RESOURCES + 0x50).to_le_bytes());
                file[at + 4..at + 8].copy_from_slice(&16_u32.to_le_bytes());
                file
            },
            Unknown(broken),
        ),
        (
            "version data of 4 GiB",
            with32(|layout| layout.resources + 0x54, 0xFFFF_FFFF),
            Unknown("more than DevHQ reads"),
        ),
        (
            "version data longer than its section",
            with32(|layout| layout.resources + 0x54, 200_000),
            Unknown("ends past its section"),
        ),
        (
            "version data in no section",
            with32(|layout| layout.resources + 0x50, 0x9000_0000),
            Unknown("in no section the file holds"),
        ),
        (
            "version data of length zero",
            with16(|layout| layout.data, 0),
            Unknown(broken),
        ),
        (
            "version data longer than itself",
            with16(|layout| layout.data, 0xFFFF),
            Unknown(broken),
        ),
        (
            "version data under another name",
            image(&version_node("VS_VERSION_INF0", &[0; 52], false, &[]), true).0,
            Unknown(broken),
        ),
        (
            "a child of length zero, which a careless reader would read for ever",
            with_children(&[vec![0; 8]]),
            Unknown(broken),
        ),
        (
            "a child longer than its parent",
            with_children(&[vec![0xFF, 0xFF, 0, 0, 1, 0, 0x41, 0]]),
            Unknown(broken),
        ),
        (
            "a key that never ends",
            with_children(&[unending_key]),
            Unknown(broken),
        ),
        (
            "a key of a hundred characters",
            with_children(&[version_node(&"K".repeat(100), &[], true, &[])]),
            Unknown(broken),
        ),
        (
            "two thousand strings",
            with_strings(&vec![string("A", &utf16("x")); 2000]),
            Unknown(broken),
        ),
        (
            "a version that is no text",
            with_strings(&[string("ProductVersion", &[0x00, 0xD8, 0x00, 0x00])]),
            Unknown(broken),
        ),
        (
            "a version of thirty thousand characters",
            with_strings(&[string("ProductVersion", &utf16(&"9".repeat(30_000)))]),
            Unknown("declares a ProductVersion that DevHQ does not keep"),
        ),
        (
            "a product name with a line break",
            with_strings(&[string("ProductName", &utf16("Atlas\nNotes"))]),
            Unknown("declares a ProductName that DevHQ does not keep"),
        ),
        (
            "strings, none of them a version or a name",
            with_strings(&[string("CompanyName", &utf16("Ana Ruiz"))]),
            Unknown("declares no ProductVersion"),
        ),
        (
            "a value that claims to be longer than the data",
            {
                let (mut file, layout) = good();
                file[layout.data + 2..layout.data + 4].copy_from_slice(&[0xFF; 2]);
                file
            },
            Unknown("declares no ProductVersion"),
        ),
    ]
}

/// A file that counts how much of it is read.
struct Counted<'a> {
    file: Cursor<&'a [u8]>,
    read: u64,
}

impl Read for Counted<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.file.read(buffer)?;
        self.read += read as u64;
        Ok(read)
    }
}

impl Seek for Counted<'_> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(to)
    }
}

/// The most the reader of version resources may read of any file, whatever the file says: its
/// headers, its section table, three tables of the resources and the version data, each at its
/// limit.
const MOST_READ: u64 = 512 * 1024;

/// Examines `file`, which must cost no more than `MOST_READ` bytes read, whatever it is.
fn examined_within_limits(file: &[u8], what: &str) -> Image {
    let mut counted = Counted {
        file: Cursor::new(file),
        read: 0,
    };
    let image = examine(&mut counted, file.len() as u64).unwrap();
    assert!(
        counted.read <= MOST_READ,
        "{what}: {} bytes read",
        counted.read
    );
    image
}

fn declared() -> Image {
    Image::Windows(Ok(Declared {
        product_version: Some("1.4.0+3f9c2ab".into()),
        file_version: Some("1.4.0.0".into()),
        product_name: Some("Atlas Notes".into()),
    }))
}

#[test]
fn answers_every_hostile_file_without_trusting_a_number_of_it() {
    assert_eq!(examined(&good().0), declared());
    for (what, file, expected) in hostile() {
        let image = examined_within_limits(&file, what);
        match (expected, &image) {
            (Expected::Declares, found) => assert_eq!(found, &declared(), "{what}"),
            (Expected::Unknown(why), Image::Windows(Err(reason)))
            | (Expected::NotExecutable(why), Image::Other(reason)) => {
                assert!(reason.contains(why), "{what}: {reason}");
            }
            _ => panic!("{what}: {image:?}"),
        }
    }
}

/// A good file cut at every length, and one with bytes overwritten at random, thousands of times:
/// whatever comes out, the reader answers, within its limits. A panic or a read without end fails
/// the test.
#[test]
fn survives_a_good_file_cut_anywhere_or_overwritten_at_random() {
    let (file, layout) = good();
    for length in 0..file.len() {
        let image = examined_within_limits(&file[..length], "cut");
        // A file cut before the end of its version data never declares what the whole one does.
        if length < layout.data {
            assert_ne!(image, declared(), "cut at {length}");
        }
    }
    let mut random = Numbers(0x2545_F491_4F6C_DD1D);
    // Where the structures are: overwriting there breaks them far more often than anywhere else.
    let structures: Vec<usize> = (0x3C..0x40)
        .chain(0x80..layout.sections + 40)
        .chain(layout.resources..file.len())
        .collect();
    for round in 0..30_000 {
        let mut broken = file.clone();
        for _ in 0..=random.below(6) {
            let at = structures[random.below(structures.len())];
            broken[at] = match random.below(4) {
                0 => 0x00,
                1 => 0xFF,
                2 => 0x80,
                _ => random.next() as u8,
            };
        }
        match examined_within_limits(&broken, "overwritten") {
            Image::Windows(Ok(found)) => {
                for text in [
                    found.product_version,
                    found.file_version,
                    found.product_name,
                ]
                .into_iter()
                .flatten()
                {
                    assert!(
                        !text.is_empty()
                            && text.chars().count() <= super::pe::MAX_TEXT
                            && !text.chars().any(char::is_control),
                        "round {round}: {text:?}"
                    );
                }
            }
            Image::Windows(Err(reason)) | Image::Other(reason) => {
                assert!(!reason.is_empty(), "round {round}");
            }
            Image::Elf => {}
        }
    }
}

/// Every hostile file, read by the real reader in its process, confined and not: each gets its
/// answer, with the hash of what the file holds, and no file changes, appears or disappears.
#[test]
fn the_reader_reads_every_hostile_file_and_changes_nothing() {
    let folder = folder("hostile-installations");
    let files: Vec<_> = hostile()
        .into_iter()
        .chain([("a good file", good().0, Expected::Declares)])
        .enumerate()
        .map(|(index, (what, bytes, expected))| {
            let path = folder.join(format!("{index:02}.exe"));
            std::fs::write(&path, &bytes).unwrap();
            (what, path, bytes, expected)
        })
        .collect();
    let on_disk = || -> Vec<_> {
        let mut entries: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let modified = entry.metadata().unwrap().modified().unwrap();
                (
                    entry.file_name(),
                    modified,
                    std::fs::read(entry.path()).unwrap(),
                )
            })
            .collect();
        entries.sort();
        entries
    };
    let before = on_disk();
    for mode in ["confined", "unconfined"] {
        let reader = test_reader(mode);
        for (what, path, bytes, expected) in &files {
            let read = reader
                .read(&path_text(path), DEADLINE)
                .unwrap_or_else(|error| panic!("{mode}, {what}: {}", error.message));
            let case = format!("{mode}, {what}: {read:?}");
            match expected {
                Expected::NotExecutable(why) => {
                    assert_eq!(read.outcome, "not_an_executable", "{case}");
                    assert!(read.detail.as_deref().unwrap().contains(why), "{case}");
                    assert_eq!(read.sha256, None, "{case}");
                }
                Expected::Declares | Expected::Unknown(_) => {
                    assert_eq!(read.outcome, "read", "{case}");
                    assert_eq!(
                        read.sha256.as_deref(),
                        Some(sha256_of(bytes).as_str()),
                        "{case}"
                    );
                    assert_eq!(read.size_bytes, Some(bytes.len() as u64), "{case}");
                    assert!(read.modified_at.is_some(), "{case}");
                    // Nothing runs from a file that cannot run.
                    assert_eq!(read.processes, Some(0), "{case}");
                    match expected {
                        Expected::Unknown(why) => {
                            assert!(
                                read.version_unknown.as_deref().unwrap().contains(why),
                                "{case}"
                            );
                            assert_eq!(read.product_version, None, "{case}");
                        }
                        _ => {
                            assert_eq!(read.version_unknown, None, "{case}");
                            assert_eq!(
                                read.product_version.as_deref(),
                                Some("1.4.0+3f9c2ab"),
                                "{case}"
                            );
                            assert_eq!(read.file_version.as_deref(), Some("1.4.0.0"), "{case}");
                            assert_eq!(read.product_name.as_deref(), Some("Atlas Notes"), "{case}");
                        }
                    }
                }
            }
            assert_eq!(read.stored_at, None, "{case}");
        }
        assert!(on_disk() == before, "{mode}: reading changed a file");
    }
    crate::inventory::tests::remove_now(folder);
}

/// What is at a path that holds no file to read, and a real executable: this test's own, which
/// this very process is running.
#[test]
fn says_what_is_at_a_path_and_counts_the_processes_running_from_a_real_executable() {
    let folder = folder("installations");
    let reader = test_reader("confined");
    let empty = folder.join("Atlas Notes");
    std::fs::create_dir(&empty).unwrap();
    for (path, outcome, detail) in [
        (
            folder.join("nothing.exe"),
            "missing",
            "Nothing is at this path.",
        ),
        // A path through a file is a path to nothing.
        (
            folder.join("nothing.exe").join("inner.exe"),
            "missing",
            "Nothing is at this path.",
        ),
        (empty, "not_an_executable", "This is a folder."),
    ] {
        let read = reader.read(&path_text(&path), DEADLINE).unwrap();
        assert_eq!(read.outcome, outcome, "{}", path.display());
        assert!(read.detail.as_deref().unwrap().contains(detail), "{read:?}");
        assert_eq!(
            (&read.sha256, read.processes, &read.version_unknown),
            (&None, None, &None)
        );
    }
    // A relative path is DevHQ's failure, kept nowhere: it names no file.
    let error = reader.read("atlas-notes.exe", DEADLINE).unwrap_err();
    assert!(
        error.message.contains("is no absolute path on this system"),
        "{}",
        error.message
    );
    let own = std::env::current_exe().unwrap();
    let read = reader.read(&path_text(&own), DEADLINE).unwrap();
    assert_eq!(read.outcome, "read", "{read:?}");
    assert_eq!(
        read.sha256.as_deref(),
        Some(sha256_of(&std::fs::read(&own).unwrap()).as_str())
    );
    // This process runs from it; the reader that read it, which does too, does not count itself.
    assert!(read.processes.unwrap() >= 1, "{read:?}");
    assert!(read.processes_unexamined.is_some());
    // It declares a version, or says why not: never neither.
    assert_ne!(
        read.version_unknown.is_some(),
        read.product_version.is_some()
            || read.file_version.is_some()
            || read.product_name.is_some(),
        "{read:?}"
    );
    crate::inventory::tests::remove_now(folder);
}

/// A file reached through a link is kept somewhere else than DevHQ asked, as a file under AppData
/// is for a process a packaged app started: the reading says where, with the same answer DevHQ
/// gives of its own inventory. Asked where it really is, it says nothing.
#[test]
fn says_where_the_system_really_keeps_a_file_reached_another_way() {
    let (real, link) = crate::inventory::tests::linked_folder();
    let (bytes, _) = good();
    std::fs::write(real.join("atlas-notes.exe"), &bytes).unwrap();
    let asked = link.join("atlas-notes.exe");
    let kept = real.join("atlas-notes.exe");
    let reader = test_reader("confined");
    let through = reader.read(&path_text(&asked), DEADLINE).unwrap();
    assert_eq!(through.outcome, "read");
    assert_eq!(through.stored_at, Some(kept.display().to_string()));
    assert_eq!(through.stored_at, crate::inventory::stored_at(&asked));
    assert_eq!(through.sha256.as_deref(), Some(sha256_of(&bytes).as_str()));
    let direct = reader.read(&path_text(&kept), DEADLINE).unwrap();
    assert_eq!(direct.stored_at, None);
    assert_eq!(direct.sha256, through.sha256);
    // The same file written with the other separator is the same place.
    if cfg!(windows) {
        let slashed = path_text(&kept).replace('\\', "/");
        assert_eq!(reader.read(&slashed, DEADLINE).unwrap().stored_at, None);
    }
    // What is not an executable still says where it is kept; what is not there has no place.
    std::fs::write(real.join("notes.txt"), "x").unwrap();
    let text = reader
        .read(&path_text(&link.join("notes.txt")), DEADLINE)
        .unwrap();
    assert_eq!(text.outcome, "not_an_executable");
    assert_eq!(
        text.stored_at,
        Some(real.join("notes.txt").display().to_string())
    );
    let nothing = reader
        .read(&path_text(&link.join("gone.exe")), DEADLINE)
        .unwrap();
    assert_eq!(
        (nothing.outcome.as_str(), nothing.stored_at),
        ("missing", None)
    );
    crate::inventory::tests::remove_linked((real, link));
}
