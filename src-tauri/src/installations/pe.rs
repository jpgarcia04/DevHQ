//! What an executable file's version resource declares, read as data.
//!
//! A Windows executable (a PE image) can carry a version resource: `ProductVersion`, `FileVersion`
//! and `ProductName`, among others, as text its build wrote. Windows offers APIs that read them,
//! but they load the file as an image, and the file is untrusted data, whoever declared its path:
//! one made to exploit a loader or a parser must find neither. So this module reads the few
//! structures that lead to those three texts, itself, from the file's bytes:
//!
//! - the DOS header, for where the PE header is; the PE and optional headers, for where the
//!   resource directory is; the section table, to turn that address into a place in the file;
//! - the resource directory, exactly three levels down (type `RT_VERSION`, then the first name,
//!   then the first language), to the version data;
//! - the version data: `VS_VERSIONINFO`, its `StringFileInfo`, the first string table in it, and
//!   in it the three texts.
//!
//! Nothing here is `unsafe`, nothing is executed or mapped, and nothing trusts a number the file
//! gives: every offset and size is checked against the file and against a limit before it is used,
//! every sum is checked for overflow, the directory is walked to a fixed depth (a table that points
//! back at itself is read at most three times) and every loop over the file's structures has a
//! bound of its own. What the file gets wrong is an answer («its version is unknown, because…»),
//! never a panic. The hostile battery (`battery.rs`) holds it to this.

use std::io::{ErrorKind, Read, Seek, SeekFrom};

/// The longest text kept of each of the three: the limit of an application's name. A version
/// resource that declares a longer one is not cut: that text is not what the file says.
pub(crate) const MAX_TEXT: usize = 120;
/// Sections a PE image can have, by its format.
const MAX_SECTIONS: usize = 96;
/// Entries read of one resource directory table. A real one has a handful.
const MAX_ENTRIES: usize = 4096;
/// The largest version data read. A real one takes one or two kilobytes.
const MAX_VERSION_DATA: usize = 256 * 1024;
/// Children walked of any one node of the version data.
const MAX_CHILDREN: usize = 1024;
/// The longest key of a node of the version data, in UTF-16 units («LegalTrademarks» has 15).
const MAX_KEY: usize = 64;

/// The resource type of version data.
const RT_VERSION: u32 = 16;
/// In a resource directory entry: the offset leads to another table, not to data; the name is a
/// string, not a number.
const HIGH_BIT: u32 = 0x8000_0000;

/// What a version resource declares, as the file says it. At least one of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Declared {
    pub product_version: Option<String>,
    pub file_version: Option<String>,
    pub product_name: Option<String>,
}

/// What kind of file this is, as far as DevHQ tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Image {
    /// A Windows executable, with what its version resource declares or why that is unknown.
    Windows(Result<Declared, String>),
    /// An ELF executable, which has no version resource.
    Elf,
    /// Neither: why it is no executable DevHQ knows.
    Other(String),
}

/// Reads at most `size` bytes at `offset` of a file `len` bytes long: None when that range is not
/// inside the file, also when the file shrank since its length was taken.
fn bytes_at<F: Read + Seek>(
    file: &mut F,
    len: u64,
    offset: u64,
    size: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let Some(end) = offset.checked_add(size as u64) else {
        return Ok(None);
    };
    if end > len {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; size];
    match file.read_exact(&mut bytes) {
        Ok(()) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => Ok(None),
        Err(error) => Err(error),
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

/// A section of the image: where it is loaded, and which bytes of the file back it.
struct Section {
    address: u32,
    file_offset: u32,
    file_size: u32,
}

/// Where in the file the image address `address` is, and how many bytes follow it there: only
/// bytes the file really has for that section.
fn in_file(sections: &[Section], address: u32) -> Option<(u64, u64)> {
    sections.iter().find_map(|section| {
        let within = address.checked_sub(section.address)?;
        (within < section.file_size).then(|| {
            (
                u64::from(section.file_offset) + u64::from(within),
                u64::from(section.file_size - within),
            )
        })
    })
}

/// The file `file`, `len` bytes long, as far as DevHQ tells what it is. An error is the system's,
/// reading the file: what the file itself gets wrong is part of the answer.
pub(crate) fn examine<F: Read + Seek>(file: &mut F, len: u64) -> std::io::Result<Image> {
    let other = |why: &str| Ok(Image::Other(why.to_owned()));
    let Some(start) = bytes_at(file, len, 0, 4.min(usize::try_from(len).unwrap_or(4)))? else {
        return other("The file could not be read to its length.");
    };
    if start == b"\x7fELF" {
        return Ok(Image::Elf);
    }
    if start.get(..2) != Some(b"MZ") {
        return other("The file does not start as an executable (no MZ or ELF header).");
    }
    let Some(dos) = bytes_at(file, len, 0, 64)? else {
        return other("The file is too short to be a Windows executable.");
    };
    let pe = u64::from(u32_at(&dos, 0x3C).unwrap_or(0));
    let Some(header) = bytes_at(file, len, pe, 24)? else {
        return other("The file names a PE header past its end: it is no Windows executable.");
    };
    if header.get(..4) != Some(b"PE\0\0") {
        return other(
            "The file has an MZ header and no PE header: it is no Windows executable DevHQ reads.",
        );
    }
    let section_count = usize::from(u16_at(&header, 6).unwrap_or(0));
    let optional_size = usize::from(u16_at(&header, 20).unwrap_or(0));
    let optional_at = pe + 24;
    let Some(optional) = bytes_at(file, len, optional_at, optional_size)? else {
        return other("The file's PE headers end past its end: it is no Windows executable.");
    };
    // PE32 and PE32+ keep their data directories at different places.
    let (count_at, directories_at) = match u16_at(&optional, 0) {
        Some(0x10B) => (92, 96),
        Some(0x20B) => (108, 112),
        _ => return other("The file's PE header is of no kind DevHQ knows (not PE32 or PE32+)."),
    };
    Ok(Image::Windows(version(
        file,
        len,
        &optional,
        (count_at, directories_at),
        (optional_at + optional_size as u64, section_count),
    )?))
}

/// What the version resource of a PE image declares; the inner error is why that is unknown.
fn version<F: Read + Seek>(
    file: &mut F,
    len: u64,
    optional: &[u8],
    (count_at, directories_at): (usize, usize),
    (sections_at, section_count): (u64, usize),
) -> std::io::Result<Result<Declared, String>> {
    let unknown = |why: &str| Ok(Err(why.to_owned()));
    let malformed = || {
        Ok(Err(
            "The file's resources are malformed, so DevHQ did not read its version resource."
                .to_owned(),
        ))
    };
    let no_resources = "The file has no resources, so it declares no version.";
    // The resource directory is the third data directory: its address and its size.
    let directories = u32_at(optional, count_at).unwrap_or(0);
    let (Some(address), Some(size)) = (
        u32_at(optional, directories_at + 16),
        u32_at(optional, directories_at + 20),
    ) else {
        return unknown(no_resources);
    };
    if directories < 3 || address == 0 || size == 0 {
        return unknown(no_resources);
    }
    if section_count > MAX_SECTIONS {
        return unknown("The file declares more sections than a Windows executable can have.");
    }
    let Some(table) = bytes_at(file, len, sections_at, section_count * 40)? else {
        return unknown("The file's section table ends past its end.");
    };
    let sections: Vec<Section> = table
        .chunks_exact(40)
        .filter_map(|entry| {
            Some(Section {
                address: u32_at(entry, 12)?,
                file_size: u32_at(entry, 16)?,
                file_offset: u32_at(entry, 20)?,
            })
        })
        .collect();
    let Some((root, available)) = in_file(&sections, address) else {
        return unknown("The file's resources are in no section the file holds.");
    };
    // Every place in the resources is relative to their start, and must stay inside them.
    let limit = u64::from(size).min(available);
    let read_at = |file: &mut F, relative: u64, size: usize| -> std::io::Result<Option<Vec<u8>>> {
        match relative.checked_add(size as u64) {
            Some(end) if end <= limit => bytes_at(file, len, root + relative, size),
            _ => Ok(None),
        }
    };
    // Three levels, never more: the type, a name under it, a language under that.
    let mut table_at = 0_u64;
    let mut data_at = None;
    for level in 0..3 {
        let Some(head) = read_at(file, table_at, 16)? else {
            return malformed();
        };
        let entries = usize::from(u16_at(&head, 12).unwrap_or(0))
            + usize::from(u16_at(&head, 14).unwrap_or(0));
        if entries > MAX_ENTRIES {
            return malformed();
        }
        let Some(list) = read_at(file, table_at + 16, entries * 8)? else {
            return malformed();
        };
        let mut pairs = list
            .chunks_exact(8)
            .filter_map(|entry| Some((u32_at(entry, 0)?, u32_at(entry, 4)?)));
        let found = if level == 0 {
            pairs.find(|(name, _)| *name == RT_VERSION)
        } else {
            pairs.next()
        };
        let Some((_, offset)) = found else {
            return if level == 0 {
                unknown("The file has no version resource.")
            } else {
                malformed()
            };
        };
        let leads_to_table = offset & HIGH_BIT != 0;
        let offset = u64::from(offset & !HIGH_BIT);
        match (level, leads_to_table) {
            (0 | 1, true) => table_at = offset,
            (2, false) => data_at = Some(offset),
            _ => return malformed(),
        }
    }
    let Some(entry) = read_at(file, data_at.unwrap_or(0), 16)? else {
        return malformed();
    };
    let (Some(data_address), Some(data_size)) = (u32_at(&entry, 0), u32_at(&entry, 4)) else {
        return malformed();
    };
    let data_size = usize::try_from(data_size).unwrap_or(usize::MAX);
    if data_size > MAX_VERSION_DATA {
        return unknown(&format!(
            "The file's version resource says it takes {data_size} bytes, more than DevHQ reads ({MAX_VERSION_DATA})."
        ));
    }
    let Some((data_offset, data_available)) = in_file(&sections, data_address) else {
        return unknown("The file's version resource is in no section the file holds.");
    };
    if data_available < data_size as u64 {
        return unknown("The file's version resource ends past its section.");
    }
    let Some(data) = bytes_at(file, len, data_offset, data_size)? else {
        return unknown("The file's version resource ends past the file.");
    };
    Ok(declared(&data))
}

/// The start of a node of the version data, each of which is its length, the length of its value,
/// whether that value is text, and its key (UTF-16, ended by a zero) padded to four bytes.
struct Node<'a> {
    /// The whole node: `bytes[..length]`.
    bytes: &'a [u8],
    value_length: usize,
    key: String,
    /// Where its value, or its children, start within `bytes`.
    after_key: usize,
}

fn align(position: usize) -> usize {
    position.saturating_add(3) & !3
}

/// The node at the start of `bytes`: None when what is there is no node that fits in them.
fn node(bytes: &[u8]) -> Option<Node<'_>> {
    let length = usize::from(u16_at(bytes, 0)?);
    let value_length = usize::from(u16_at(bytes, 2)?);
    if length < 6 {
        return None;
    }
    let bytes = bytes.get(..length)?;
    let mut units = Vec::new();
    let mut at = 6;
    loop {
        let unit = u16_at(bytes, at)?;
        at += 2;
        if unit == 0 {
            break;
        }
        if units.len() == MAX_KEY {
            return None;
        }
        units.push(unit);
    }
    Some(Node {
        bytes,
        value_length,
        key: String::from_utf16(&units).ok()?,
        // A node starts at a multiple of four in the data, so padding within it is the same.
        after_key: align(at).min(length),
    })
}

/// The nodes that follow one another in `bytes`, each padded to four bytes: None when something
/// there is no node, or there are more than DevHQ walks.
fn children(bytes: &[u8]) -> Option<Vec<Node<'_>>> {
    let mut found = Vec::new();
    let mut at = 0;
    // What is left after the last child is padding.
    while bytes.len().saturating_sub(at) >= 6 {
        if found.len() == MAX_CHILDREN {
            return None;
        }
        let child = node(&bytes[at..])?;
        at = align(at + child.bytes.len());
        found.push(child);
    }
    Some(found)
}

/// The text a string node holds: UTF-16 up to its first zero, or to the node's end.
fn text(node: &Node) -> Option<String> {
    let units: Vec<u16> = node.bytes[node.after_key..]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16(&units).ok()
}

/// What version data declares; the error is why DevHQ does not know.
fn declared(data: &[u8]) -> Result<Declared, String> {
    let malformed =
        || "The file's version resource is malformed, so what it declares is unknown.".to_owned();
    let root = node(data).ok_or_else(malformed)?;
    if root.key != "VS_VERSION_INFO" {
        return Err(malformed());
    }
    // Its value is the fixed, numeric version information; its children follow it, padded.
    let after_value = align(root.after_key.saturating_add(root.value_length));
    let rest = root.bytes.get(after_value..).unwrap_or_default();
    let mut found = Declared::default();
    for info in children(rest).ok_or_else(malformed)? {
        if info.key != "StringFileInfo" {
            continue;
        }
        // The first table: a file can repeat its strings in several languages.
        let tables = children(&info.bytes[info.after_key..]).ok_or_else(malformed)?;
        let Some(table) = tables.first() else {
            continue;
        };
        for string in children(&table.bytes[table.after_key..]).ok_or_else(malformed)? {
            let slot = match string.key.as_str() {
                "ProductVersion" => &mut found.product_version,
                "FileVersion" => &mut found.file_version,
                "ProductName" => &mut found.product_name,
                _ => continue,
            };
            if slot.is_some() {
                continue;
            }
            let value = text(&string).ok_or_else(malformed)?;
            let value = value.trim();
            if value.chars().count() > MAX_TEXT || value.chars().any(char::is_control) {
                return Err(format!(
                    "The file's version resource declares a {} that DevHQ does not keep: longer than {MAX_TEXT} characters, or with control characters.",
                    string.key
                ));
            }
            if !value.is_empty() {
                *slot = Some(value.to_owned());
            }
        }
        break;
    }
    if found == Declared::default() {
        return Err(
            "The file's version resource declares no ProductVersion, FileVersion or ProductName."
                .to_owned(),
        );
    }
    Ok(found)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A node of version data: its key, its value's bytes (text or not) and its children, padded
    /// as the format says.
    pub(crate) fn version_node(
        key: &str,
        value: &[u8],
        is_text: bool,
        children: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut bytes = vec![0; 6];
        for unit in key.encode_utf16().chain([0]) {
            bytes.extend(unit.to_le_bytes());
        }
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        bytes.extend(value);
        for child in children {
            while !bytes.len().is_multiple_of(4) {
                bytes.push(0);
            }
            bytes.extend(child);
        }
        let length = u16::try_from(bytes.len()).unwrap();
        let value_length = u16::try_from(if is_text {
            value.len() / 2
        } else {
            value.len()
        })
        .unwrap();
        bytes[0..2].copy_from_slice(&length.to_le_bytes());
        bytes[2..4].copy_from_slice(&value_length.to_le_bytes());
        bytes[4..6].copy_from_slice(&u16::from(is_text).to_le_bytes());
        bytes
    }

    pub(crate) fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    /// Version data that declares `strings`, as a resource compiler writes it.
    pub(crate) fn version_data(strings: &[(&str, &str)]) -> Vec<u8> {
        let strings: Vec<Vec<u8>> = strings
            .iter()
            .map(|(key, value)| version_node(key, &utf16(value), true, &[]))
            .collect();
        let table = version_node("040904B0", &[], true, &strings);
        let info = version_node("StringFileInfo", &[], true, &[table]);
        let translation = version_node("Translation", &[0x09, 0x04, 0xB0, 0x04], false, &[]);
        let var = version_node("VarFileInfo", &[], true, &[translation]);
        // VS_FIXEDFILEINFO: its signature, and the rest left at zero.
        let mut fixed = vec![0; 52];
        fixed[..4].copy_from_slice(&0xFEEF_04BD_u32.to_le_bytes());
        version_node("VS_VERSION_INFO", &fixed, false, &[info, var])
    }

    /// Where the pieces of a synthetic image are, for tests that break one of them.
    pub(crate) struct Layout {
        /// The file offset of the optional header.
        pub optional: usize,
        /// The file offset of the section table.
        pub sections: usize,
        /// The file offset of the resource section, which is at image address `RESOURCES`.
        pub resources: usize,
        /// The file offset of the version data.
        pub data: usize,
    }

    /// The image address of the resource section in a synthetic image.
    pub(crate) const RESOURCES: u32 = 0x2000;

    /// A minimal PE32+ image (or PE32 when `wide` is false) whose only section is its resources,
    /// with `data` as its version data. Nothing in it can run: it has no code.
    pub(crate) fn image(data: &[u8], wide: bool) -> (Vec<u8>, Layout) {
        let mut file = vec![0; 0x80];
        file[..2].copy_from_slice(b"MZ");
        file[0x3C..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        file.extend(b"PE\0\0");
        let optional_size: u16 = if wide { 240 } else { 224 };
        let mut coff = vec![0; 20];
        coff[0..2].copy_from_slice(&(if wide { 0x8664_u16 } else { 0x014C }).to_le_bytes());
        coff[2..4].copy_from_slice(&1_u16.to_le_bytes());
        coff[16..18].copy_from_slice(&optional_size.to_le_bytes());
        file.extend(coff);
        let optional_at = file.len();
        let mut optional = vec![0; usize::from(optional_size)];
        optional[0..2].copy_from_slice(&(if wide { 0x20B_u16 } else { 0x10B }).to_le_bytes());
        let (count_at, directories_at) = if wide { (108, 112) } else { (92, 96) };
        optional[count_at..count_at + 4].copy_from_slice(&16_u32.to_le_bytes());
        // The resources: a table per level, the data entry, the data.
        let mut resources = Vec::new();
        let table = |named: u16, ids: u16| {
            let mut head = vec![0; 16];
            head[12..14].copy_from_slice(&named.to_le_bytes());
            head[14..16].copy_from_slice(&ids.to_le_bytes());
            head
        };
        let entry = |name: u32, offset: u32| [name.to_le_bytes(), offset.to_le_bytes()].concat();
        // An icon group first, so that the version is not simply the first entry.
        resources.extend(table(0, 2));
        resources.extend(entry(14, HIGH_BIT | 0x20));
        resources.extend(entry(RT_VERSION, HIGH_BIT | 0x20));
        resources.extend(table(0, 1)); // 0x20
        resources.extend(entry(1, HIGH_BIT | 0x38));
        resources.extend(table(0, 1)); // 0x38
        resources.extend(entry(0x0409, 0x50));
        let data_at = 0x60_u32;
        resources.extend((RESOURCES + data_at).to_le_bytes()); // 0x50
        resources.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
        resources.extend([0; 8]);
        resources.extend(data);
        let resources_at = 0x400_usize;
        let size = u32::try_from(resources.len()).unwrap();
        optional[directories_at + 16..directories_at + 20]
            .copy_from_slice(&RESOURCES.to_le_bytes());
        optional[directories_at + 20..directories_at + 24].copy_from_slice(&size.to_le_bytes());
        file.extend(optional);
        let sections_at = file.len();
        let mut section = vec![0; 40];
        section[..5].copy_from_slice(b".rsrc");
        section[8..12].copy_from_slice(&size.to_le_bytes());
        section[12..16].copy_from_slice(&RESOURCES.to_le_bytes());
        section[16..20].copy_from_slice(&size.to_le_bytes());
        section[20..24].copy_from_slice(&u32::try_from(resources_at).unwrap().to_le_bytes());
        file.extend(section);
        file.resize(resources_at, 0);
        file.extend(resources);
        (
            file,
            Layout {
                optional: optional_at,
                sections: sections_at,
                resources: resources_at,
                data: resources_at + data_at as usize,
            },
        )
    }

    pub(crate) fn examined(file: &[u8]) -> Image {
        examine(&mut Cursor::new(file), file.len() as u64).unwrap()
    }

    fn all(product_version: &str, file_version: &str, product_name: &str) -> Image {
        Image::Windows(Ok(Declared {
            product_version: Some(product_version.into()),
            file_version: Some(file_version.into()),
            product_name: Some(product_name.into()),
        }))
    }

    #[test]
    fn reads_what_a_version_resource_declares_as_the_file_says_it() {
        let strings = [
            ("CompanyName", "Ana Ruiz"),
            ("FileDescription", "Atlas Notes"),
            ("FileVersion", "1.4.0.0"),
            ("ProductName", "Atlas Notes"),
            ("ProductVersion", "1.4.0+3f9c2ab"),
        ];
        for wide in [true, false] {
            let (file, _) = image(&version_data(&strings), wide);
            assert_eq!(
                examined(&file),
                all("1.4.0+3f9c2ab", "1.4.0.0", "Atlas Notes"),
                "PE32+: {wide}"
            );
        }
        // Only what the file declares, each as it is: nothing is derived from another, and
        // surrounding whitespace is no part of a version.
        let (file, _) = image(
            &version_data(&[("ProductVersion", " 0.1.0 "), ("FileVersion", "")]),
            true,
        );
        assert_eq!(
            examined(&file),
            Image::Windows(Ok(Declared {
                product_version: Some("0.1.0".into()),
                ..Declared::default()
            }))
        );
        // Text outside ASCII, and a file that repeats its strings in a second language: the first.
        let spanish = version_node(
            "0C0A04B0",
            &[],
            true,
            &[version_node(
                "ProductName",
                &utf16("Cuentas Claras"),
                true,
                &[],
            )],
        );
        let english = version_node(
            "040904B0",
            &[],
            true,
            &[version_node(
                "ProductName",
                &utf16("Clear Accounts"),
                true,
                &[],
            )],
        );
        let info = version_node("StringFileInfo", &[], true, &[spanish, english]);
        let (file, _) = image(
            &version_node("VS_VERSION_INFO", &[0; 52], false, &[info]),
            true,
        );
        assert_eq!(
            examined(&file),
            Image::Windows(Ok(Declared {
                product_name: Some("Cuentas Claras".into()),
                ..Declared::default()
            }))
        );
    }

    #[test]
    fn tells_what_is_no_windows_executable() {
        assert_eq!(examined(b"\x7fELF\x02\x01\x01"), Image::Elf);
        for (file, why) in [
            (b"".to_vec(), "does not start as an executable"),
            (
                b"#!/bin/sh\necho".to_vec(),
                "does not start as an executable",
            ),
            (b"MZ".to_vec(), "too short"),
            // A DOS program: an MZ header that leads to no PE header.
            ([b"MZ".to_vec(), vec![0; 200]].concat(), "no PE header"),
        ] {
            let Image::Other(reason) = examined(&file) else {
                panic!("{file:?}");
            };
            assert!(reason.contains(why), "{reason}");
        }
    }
}
