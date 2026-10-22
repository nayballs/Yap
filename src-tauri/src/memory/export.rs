//! "Export memory": a .zip of Markdown files, one per section (You.md —
//! Profile and Preferences —, People.md, Projects.md, Topics.md), written
//! by hand (stored with Deflate, CRC-32 from flate2) so no zip crate is
//! needed. Pure but for the file write.

use std::io::Write;

use chrono::{Datelike, TimeZone, Timelike};

use super::ops::{Item, Section};

/// "5 Oct 2026" in the person's time zone.
pub fn day(ts: u64) -> String {
    chrono::Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|t| t.format("%-d %b %Y").to_string())
        .unwrap_or_default()
}

/// Where an item came from, in words: "from a chat (“Trip plans”)".
fn origin_words(i: &Item) -> String {
    let title = if i.source.title.is_empty() { String::new() } else { format!(" (“{}”)", i.source.title) };
    match i.source.kind.as_str() {
        "chat" => format!("from a chat{title}"),
        "meeting" => format!("from a meeting{title}"),
        "voice" => "you asked Yap to remember it".to_string(),
        "tell" => "you told Yap".to_string(),
        "import" => "imported".to_string(),
        _ => "added by you".to_string(),
    }
}

/// One section's file (`files` gives each its name), `lead` (Markdown,
/// "" = none) first.
fn section_markdown(heading: &str, lead: &str, sections: &[Section], items: &[Item]) -> String {
    let mut out = format!("# {heading}\n");
    let mut any = !lead.is_empty();
    out.push_str(lead);
    for &section in sections {
        let mut list: Vec<&Item> = items.iter().filter(|i| i.section == section).collect();
        list.sort_by_key(|a| a.title.to_lowercase());
        for i in list {
            any = true;
            out.push_str(&format!("\n## {}\n", i.title));
            if !i.summary.is_empty() {
                out.push_str(&format!("\n{}\n", i.summary));
            }
            if !i.details.is_empty() {
                out.push('\n');
                for d in &i.details {
                    out.push_str(&format!("- {d}\n"));
                }
            }
            out.push_str(&format!("\n_Updated {} · {}_\n", day(i.updated_ts), origin_words(i)));
        }
    }
    if !any {
        out.push_str("\nNothing yet.\n");
    }
    out
}

/// About you's part of You.md: `about` = (label, value) pairs, the empty
/// ones left out ("" when none is set).
fn about_markdown(about: &[(&str, String)]) -> String {
    let lines: Vec<String> = about.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("- {k}: {v}\n")).collect();
    if lines.is_empty() {
        return String::new();
    }
    format!("\n## About you\n\n{}\n_Set in Settings → About you_\n", lines.concat())
}

/// The files of an export: (name, Markdown). `about`: About you's region,
/// name and note, as (label, value), at the top of You.md.
pub fn files(items: &[Item], about: &[(&str, String)]) -> Vec<(String, String)> {
    vec![
        ("You.md".to_string(), section_markdown("You", &about_markdown(about), &[Section::Profile, Section::Preferences], items)),
        ("People.md".to_string(), section_markdown("People", "", &[Section::People], items)),
        ("Projects.md".to_string(), section_markdown("Projects", "", &[Section::Projects], items)),
        ("Topics.md".to_string(), section_markdown("Topics", "", &[Section::Topics], items)),
    ]
}

/// MS-DOS time and date of now (local), as zip headers want them.
fn dos_now() -> (u16, u16) {
    let t = chrono::Local::now();
    let time = ((t.hour() as u16) << 11) | ((t.minute() as u16) << 5) | ((t.second() / 2) as u16);
    let date = (((t.year().max(1980) - 1980) as u16) << 9) | ((t.month() as u16) << 5) | (t.day() as u16);
    (time, date)
}

/// A zip archive of `files` (names in UTF-8, each Deflate-compressed).
pub fn zip(files: &[(String, String)]) -> std::io::Result<Vec<u8>> {
    let (time, date) = dos_now();
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, text) in files {
        let data = text.as_bytes();
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data)?;
        let packed = enc.finish()?;
        let offset = out.len() as u32;
        let name_bytes = name.as_bytes();
        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // flags: UTF-8 names
        out.extend_from_slice(&8u16.to_le_bytes()); // Deflate
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.sum().to_le_bytes());
        out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&packed);
        // Its central directory entry.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.sum().to_le_bytes());
        central.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }
    let central_at = out.len() as u32;
    out.extend_from_slice(&central);
    // End of central directory.
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::ops::{Item, Origin, Section};
    use super::*;

    #[test]
    fn an_export_is_one_markdown_file_per_section() {
        let items = vec![
            Item {
                id: 1,
                section: Section::Profile,
                title: "Profile".into(),
                summary: String::new(),
                details: vec!["Works as a nurse".into()],
                source: Origin::new("chat", Some(3), "Shift patterns"),
                created_ts: 1_790_000_000,
                updated_ts: 1_790_000_000,
                dictation: true,
            },
            Item {
                id: 2,
                section: Section::People,
                title: "Rochelle".into(),
                summary: "Sister".into(),
                details: vec!["Lives in York".into()],
                source: Origin::new("manual", None, ""),
                created_ts: 1_790_000_000,
                updated_ts: 1_790_000_000,
                dictation: true,
            },
        ];
        let f = files(&items, &[]);
        let names: Vec<&str> = f.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["You.md", "People.md", "Projects.md", "Topics.md"]);
        assert!(f[0].1.starts_with("# You\n\n## Profile\n\n- Works as a nurse\n"), "{}", f[0].1);
        assert!(f[0].1.contains("from a chat (“Shift patterns”)"), "{}", f[0].1);
        assert!(f[1].1.contains("## Rochelle\n\nSister\n\n- Lives in York\n"), "{}", f[1].1);
        assert!(f[2].1.contains("Nothing yet."));
        // About you's region, name and note head You.md; the empty ones
        // are left out, and nothing set adds nothing.
        let about = [("Name", "Declan".to_string()), ("Region", "United Kingdom".to_string()), ("What you told Yap", String::new())];
        let you = &files(&items, &about)[0].1;
        assert!(
            you.starts_with("# You\n\n## About you\n\n- Name: Declan\n- Region: United Kingdom\n\n_Set in Settings → About you_\n\n## Profile\n"),
            "{you}"
        );
        assert!(!files(&[], &about)[0].1.contains("Nothing yet."));
        assert!(files(&[], &[])[0].1.contains("Nothing yet."));
        // A valid archive: the signatures, and each file's text inflates back.
        let z = zip(&f).unwrap();
        assert_eq!(&z[..4], &[0x50, 0x4b, 0x03, 0x04]);
        assert_eq!(&z[z.len() - 22..z.len() - 18], &[0x50, 0x4b, 0x05, 0x06]);
        let name_len = u16::from_le_bytes([z[26], z[27]]) as usize;
        let packed_len = u32::from_le_bytes([z[18], z[19], z[20], z[21]]) as usize;
        let start = 30 + name_len;
        let mut text = String::new();
        use std::io::Read;
        flate2::read::DeflateDecoder::new(&z[start..start + packed_len]).read_to_string(&mut text).unwrap();
        assert_eq!(text, f[0].1);
    }
}
