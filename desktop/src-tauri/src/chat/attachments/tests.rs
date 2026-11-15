use super::*;

/// A page of a PDF made here: typed text, or a scan (a picture of
/// fictional words, `fixtures/scanned-note.png`, and no text).
pub(crate) enum Pg<'a> {
    Text(&'a str),
    Scan,
}

/// The scanned page's picture: an 8-bit grey PNG, its width, height and
/// compressed rows (a PDF's FlateDecode with PNG's predictors takes them
/// as they are).
fn scan_picture() -> (u32, u32, Vec<u8>) {
    let png = include_bytes!("fixtures/scanned-note.png");
    let (mut w, mut h, mut idat) = (0, 0, Vec::new());
    let mut at = 8;
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        let data = &png[at + 8..at + 8 + len];
        match kind {
            b"IHDR" => {
                w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                assert_eq!((data[8], data[9], data[12]), (8, 0, 0), "8-bit grey, not interlaced");
            }
            b"IDAT" => idat.extend_from_slice(data),
            _ => {}
        }
        at += 12 + len;
    }
    (w, h, idat)
}

/// A PDF of `pages`, its cross-reference table right (made here: no file of
/// anyone's). A scanned page shows the picture across its top half.
pub(crate) fn pdf_of(pages: &[Pg]) -> Vec<u8> {
    let (w, h, rows) = scan_picture();
    let esc = |t: &str| t.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    // 1 the catalog, 2 the pages, 3 the font, 4 the picture; then each page's
    // contents and the page.
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 6 + i * 2)).collect();
    let picture = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /DecodeParms << /Predictor 15 /Colors 1 /BitsPerComponent 8 /Columns {w} >> /Length {} >>\nstream\n",
        rows.len()
    );
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        [picture.into_bytes(), rows, b"\nendstream".to_vec()].concat(),
    ];
    for page in pages {
        let content = match page {
            Pg::Text(t) => format!("BT /F1 12 Tf 72 720 Td ({}) Tj ET", esc(t)),
            Pg::Scan => "q 500 0 0 150 56 600 cm /Im1 Do Q".to_string(),
        };
        objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).into_bytes());
        let n = objects.len();
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {n} 0 R /Resources << /Font << /F1 3 0 R >> /XObject << /Im1 4 0 R >> >> >>").into_bytes());
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// A one-page PDF with `text` on it.
pub(crate) fn tiny_pdf(text: &str) -> Vec<u8> {
    pdf_of(&[Pg::Text(text)])
}

/// A PDF of `n` scanned pages.
pub(crate) fn scanned_pdf(n: usize) -> Vec<u8> {
    pdf_of(&(0..n).map(|_| Pg::Scan).collect::<Vec<_>>())
}

/// A zip of `files` (name, text).
pub(crate) fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, text) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
            std::io::Write::write_all(&mut z, text.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn ids_hide_no_path() {
    let id = new_id();
    assert!(is_id(&id), "{id}");
    assert!(!is_id("file_../../evil.txt"));
    assert!(!is_id("img_0123456789abcdef"));
    assert!(!is_id("file_0123456789ABCDEF"));
    assert!(text_path("..\\x").is_none());
}

#[test]
fn text_is_told_from_bytes_that_arent() {
    assert_eq!(decode(b"Seeds\r\nBulbs\r").as_deref(), Some("Seeds\nBulbs\n"));
    assert_eq!(decode(b"\xEF\xBB\xBFcaf\xC3\xA9").as_deref(), Some("café"));
    // UTF-16 with its mark, as Notepad and PowerShell save it.
    let mut le = vec![0xFF, 0xFE];
    le.extend("Beds".encode_utf16().flat_map(|u| u.to_le_bytes()));
    assert_eq!(decode(&le).as_deref(), Some("Beds"));
    let mut be = vec![0xFE, 0xFF];
    be.extend("Beds".encode_utf16().flat_map(|u| u.to_be_bytes()));
    assert_eq!(decode(&be).as_deref(), Some("Beds"));
    // A program, a picture: never text.
    assert_eq!(decode(b"MZ\x90\x00\x03\x00\x00\x00"), None);
    assert_eq!(decode(&[0x01, 0x02, 0x03, 0x04, 0x05, b'a']), None);
}

#[test]
fn each_kind_is_read_or_said_plainly() {
    let r = read("plan.txt", b"Water twice a week.".to_vec()).unwrap();
    assert_eq!((r.kind, r.text.as_str(), r.pages), ("text", "Water twice a week.", None));
    assert_eq!(read("main.rs", b"fn main() {}".to_vec()).unwrap().kind, "code");
    assert_eq!(read("notes.md", b"# Garden".to_vec()).unwrap().kind, "markdown");
    // An ending Yap doesn't know, but text inside: text.
    assert_eq!(read("Makefile", b"all:\n\techo hi".to_vec()).unwrap().kind, "text");
    assert_eq!(read("budget.xlsx", vec![0x50, 0x4B, 0x03, 0x04, 0, 0, 0, 0]).unwrap_err(), "Chat can't read budget.xlsx (a spreadsheet: save it as CSV to attach it). It reads text, code, PDF, Word and PowerPoint files, and pictures.");
    assert!(read("photo.png", b"\x89PNG".to_vec()).unwrap_err().contains("is a picture"));
    assert_eq!(read("empty.txt", Vec::new()).unwrap_err(), "empty.txt is empty.");
    assert_eq!(read("blank.txt", b" \n\n ".to_vec()).unwrap_err(), "blank.txt is empty.");
    let pdf = read("seed-order.pdf", tiny_pdf("Twelve packets of runner beans")).unwrap();
    assert_eq!((pdf.kind, pdf.pages), ("pdf", Some(1)));
    assert!(pdf.text.contains("Twelve packets of runner beans"), "{}", pdf.text);
    assert_eq!(read("broken.pdf", b"%PDF-1.7 nothing else".to_vec()).unwrap_err(), "Yap couldn't read the text of broken.pdf.");
    let docx = zip_of(&[("word/document.xml", "<w:document><w:body><w:p><w:r><w:t>Plot 7</w:t></w:r></w:p></w:body></w:document>")]);
    let word = read("allotment.docx", docx).unwrap();
    assert_eq!((word.kind, word.text.as_str()), ("word", "Plot 7"));
    let empty_docx = zip_of(&[("word/document.xml", "<w:document><w:body><w:p/></w:body></w:document>")]);
    assert_eq!(read("blank.docx", empty_docx).unwrap_err(), "blank.docx has no text in it.");
}

#[test]
fn presentations_and_older_word_documents_are_read_and_the_rest_said_for_their_kind() {
    let deck = read("allotment-year.pptx", super::powerpoint::tests::pptx()).unwrap();
    assert_eq!(deck.kind, "powerpoint");
    assert!(deck.text.starts_with("Slide 1: Allotment year\n\nSlide 2: Spring planting\n"), "{}", deck.text);
    assert!(deck.text.ends_with("Speaker notes: Mention the frost dates."), "{}", deck.text);
    assert_eq!(read("broken.pptx", b"not a zip".to_vec()).unwrap_err(), "Yap couldn't read the text of broken.pptx.");
    let blank = zip_of(&[("ppt/presentation.xml", "<p:presentation/>")]);
    assert_eq!(read("blank.pptx", blank).unwrap_err(), "blank.pptx has no text in it.");
    // An older Word document; a locked one; one cut short; one that's really a .docx, or RTF.
    let doc = read("rules.doc", super::doc::tests::garden_doc()).unwrap();
    assert_eq!((doc.kind, doc.text.lines().next()), ("word", Some("Allotment rules")));
    assert_eq!(read("secret.doc", super::doc::tests::doc_of(&[(true, "Plot 9\r")], true)).unwrap_err(), "secret.doc is password-protected, so Yap can't read it.");
    let mut cut = super::doc::tests::garden_doc();
    cut.truncate(700);
    assert_eq!(read("cut.doc", cut).unwrap_err(), "Yap couldn't read the text of cut.doc (it may be from Word 95 or older): save it as .docx in Word to attach it.");
    let docx = zip_of(&[("word/document.xml", "<w:document><w:body><w:p><w:r><w:t>Plot 8</w:t></w:r></w:p></w:body></w:document>")]);
    assert_eq!(read("saved-as.doc", docx).unwrap().text, "Plot 8");
    let rtf = read("letter.doc", b"{\\rtf1\\ansi Dear neighbour}".to_vec()).unwrap();
    assert_eq!((rtf.kind, rtf.text.as_str()), ("text", "{\\rtf1\\ansi Dear neighbour}"));
    // An older presentation: never read, said.
    assert_eq!(
        read("old.ppt", vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]).unwrap_err(),
        "Chat can't read old.ppt (an older PowerPoint presentation: save it as .pptx in PowerPoint to attach it). It reads text, code, PDF, Word and PowerPoint files, and pictures."
    );
}

#[test]
fn the_source_says_what_a_files_text_is() {
    assert_eq!(source("notes.md", "markdown", None, None, None), "notes.md");
    assert_eq!(source("o.pdf", "pdf", Some(1), None, None), "o.pdf (a PDF, 1 page: its text, read on the person's PC)");
    assert_eq!(
        source("scan.pdf", "pdf", Some(48), Some(20), Some(28)),
        "scan.pdf (a PDF, 48 pages: its text, read on the person's PC; 20 scanned pages read with OCR, which can get words wrong; 28 pages not read, named at the end)"
    );
    assert_eq!(source("rules.doc", "word", None, None, None), "rules.doc (a Word document: its text)");
    assert_eq!(source("year.pptx", "powerpoint", None, None, None), "year.pptx (a PowerPoint presentation: each slide's text, then its speaker notes)");
}

/// A scanned PDF attached: its chip's fields say how many pages OCR read
/// (where Windows has an OCR language: a PC without one says so).
#[cfg(windows)]
#[test]
fn a_scanned_pdf_kept_says_its_pages_read_with_ocr() {
    match keep("notice.pdf", scanned_pdf(1)) {
        Ok(a) => {
            assert_eq!((a.kind.as_str(), a.pages, a.ocr_pages, a.unread_pages), ("pdf", Some(1), Some(1), None));
            assert!(text(&a.id).unwrap().starts_with("[Page 1, read with OCR]\n"));
            assert!(serde_json::to_string(&a).unwrap().contains("\"ocrPages\":1"));
            delete(&[a.id]);
        }
        Err(e) if e.contains("isn't set up on this PC") => eprintln!("no OCR language on this PC: skipped"),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn a_file_too_long_for_any_model_is_said() {
    let long = "a".repeat(MAX_CHARS + 1);
    let e = read("huge.log", long.into_bytes()).unwrap_err();
    assert!(e.starts_with("huge.log is too long to send: about 1.1M tokens"), "{e}");
    assert_eq!(round_tokens(800), "800");
    assert_eq!(round_tokens(1234), "1.2k");
    assert_eq!(round_tokens(56_789), "56k");
}

#[test]
fn a_name_is_kept_safely() {
    assert_eq!(safe_name("..\\..\\secret.txt"), "secret.txt");
    assert_eq!(safe_name("a<b>:c?.md"), "a_b__c_.md");
    assert_eq!(safe_name("..."), "file");
    assert_eq!(safe_name("notes. "), "notes");
    let long = format!("{}.pdf", "x".repeat(300));
    let kept = safe_name(&long);
    assert_eq!(kept.chars().count(), 120);
    assert!(kept.ends_with(".pdf"));
}

#[test]
fn a_file_is_kept_read_back_and_deleted() {
    let a = keep("seed list.txt", b"Beans\r\nPeas".to_vec()).unwrap();
    assert_eq!((a.kind.as_str(), a.bytes, a.name.as_str()), ("text", 11, "seed list.txt"));
    assert!(a.tokens > 0);
    assert_eq!(text(&a.id).as_deref(), Some("Beans\nPeas"));
    assert_eq!(std::fs::read(copy_path(&a).unwrap()).unwrap(), b"Beans\r\nPeas");
    // Sent: the box's ids, in order; one that went isn't.
    assert_eq!(taken(std::slice::from_ref(&a.id)).unwrap(), vec![a.clone()]);
    assert_eq!(delete(std::slice::from_ref(&a.id)), 1);
    assert!(!exists(&a.id));
    assert!(taken(std::slice::from_ref(&a.id)).unwrap_err().contains("isn't there any more"));
    assert!(taken(&["file_0000000000000000".into()]).is_err());
}

fn att(id: &str, name: &str, kind: &str) -> Attachment {
    Attachment { id: id.into(), name: name.into(), kind: kind.into(), bytes: 10, tokens: 3, pages: (kind == "pdf").then_some(2), ..Default::default() }
}

#[test]
fn the_block_names_each_file_and_says_a_removed_one() {
    let files = vec![(att("file_1", "plan.txt", "text"), Some("Beans\n".to_string())), (att("file_2", "order.pdf", "pdf"), Some("[Page 1]\nSeeds".to_string())), (att("file_3", "old.md", "markdown"), None)];
    assert_eq!(
        block_of(&files),
        "<documents>\n<document index=\"1\">\n<source>plan.txt</source>\n<document_content>\nBeans\n</document_content>\n</document>\n<document index=\"2\">\n<source>order.pdf (a PDF, 2 pages: its text, read on the person's PC)</source>\n<document_content>\n[Page 1]\nSeeds\n</document_content>\n</document>\n</documents>\n(A file was attached here, old.md; it was removed.)"
    );
    // Only removed ones: their lines alone.
    assert_eq!(block_of(&files[2..]), "(A file was attached here, old.md; it was removed.)");
    assert_eq!(block_of(&[]), "");
}

#[test]
fn the_block_goes_before_the_question_whatever_its_shape() {
    let mut plain = vec![json!({ "role": "system", "content": "s" }), json!({ "role": "user", "content": "What's in it?" })];
    attach_block(&mut plain, "<documents>D</documents>");
    assert_eq!(plain[1]["content"], "<documents>D</documents>\n\nWhat's in it?");
    // With screenshots: a text part first.
    let mut parts = vec![json!({ "role": "user", "content": [{ "type": "image_url", "image_url": { "url": "data:" } }, { "type": "text", "text": "And this?" }] })];
    attach_block(&mut parts, "<documents>D</documents>");
    assert_eq!(parts[0]["content"][0], json!({ "type": "text", "text": "<documents>D</documents>" }));
    assert_eq!(parts[0]["content"].as_array().unwrap().len(), 3);
    // Outside a turn with files: nothing.
    let mut none = vec![json!({ "role": "user", "content": "Hi" })];
    attach(&mut none);
    assert_eq!(none[0]["content"], "Hi");
    assert_eq!(tokens_of(&[]), 0);
    assert_eq!(tokens_of(&[att("file_1", "a.txt", "text")]), 3 + 24 + 8);
}

#[tokio::test]
async fn the_turn_carries_its_files() {
    let a = keep("bed-plan.md", b"North bed: tomatoes".to_vec()).unwrap();
    let (said, tokens, files) = with(vec![a.clone()], async { (with_text("Which bed?"), current_tokens(), current()) }).await;
    assert!(said.starts_with("<documents>\n<document index=\"1\">\n<source>bed-plan.md</source>"), "{said}");
    assert!(said.contains("North bed: tomatoes") && said.ends_with("\n\nWhich bed?"), "{said}");
    assert_eq!(tokens, tokens_of(std::slice::from_ref(&a)));
    assert_eq!(files, vec![a.clone()]);
    assert_eq!(with_text("Outside"), "Outside");
    delete(&[a.id]);
}

#[test]
fn the_sweep_keeps_what_a_chat_carries_for_its_days_and_a_stray_one_a_day() {
    assert!(!sweep_verdict(false, 60, 30));
    assert!(sweep_verdict(false, ORPHAN_SECS + 1, 30));
    assert!(!sweep_verdict(true, 10 * 86_400, 30));
    assert!(sweep_verdict(true, 31 * 86_400, 30));
    assert!(!sweep_verdict(true, 3_000 * 86_400, 0));
    assert_eq!(keep_days(90), 90);
    assert_eq!(keep_days(12), KEEP_DEFAULT);

    let kept = keep("kept.txt", b"kept".to_vec()).unwrap();
    let stray = keep("stray.txt", b"stray".to_vec()).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let referenced: HashSet<String> = [kept.id.clone()].into();
    assert_eq!(sweep_in(&dir(), &referenced, 30, now), 0);
    // Two days on: the stray one goes, text and copy; the chat's stays.
    assert_eq!(sweep_in(&dir(), &referenced, 30, now + 2 * 86_400), 1);
    assert!(!exists(&stray.id) && !dir().join(&stray.id).exists());
    assert!(exists(&kept.id));
    // Past its days: it goes too.
    assert_eq!(sweep_in(&dir(), &referenced, 30, now + 31 * 86_400), 1);
    assert!(!exists(&kept.id));
}

#[test]
fn files_an_older_yaps_save_dropped_come_back() {
    let q = |ts, files: Vec<Attachment>| crate::chat::chats::ChatMessage { role: "user".into(), text: "q".into(), ts, attachments: files, ..Default::default() };
    let a = att("file_0123456789abcdef", "plan.txt", "text");
    let conv = |messages: Vec<crate::chat::chats::ChatMessage>| {
        let mut c: crate::chat::chats::Conversation = serde_json::from_value(json!({ "id": 4, "title": "Beds", "createdTs": 0, "updatedTs": 0 })).unwrap();
        c.messages = messages;
        c
    };
    let convs = vec![conv(vec![q(100, vec![a.clone()]), q(200, vec![])])];
    let mirror = mirror_of(&convs);
    assert_eq!(mirror.messages.len(), 1);
    // The older Yap's save: the field gone, a message added before it.
    let mut dropped = vec![conv(vec![q(50, vec![]), q(100, vec![]), q(200, vec![])])];
    assert_eq!(restore_from(&mut dropped, &mirror), 1);
    assert_eq!(dropped[0].messages[1].attachments, vec![a]);
    assert!(dropped[0].messages[0].attachments.is_empty());
    // Once back, nothing twice.
    assert_eq!(restore_from(&mut dropped, &mirror), 0);

    // A Yap from before a scan's OCR pages: the file kept, its counts dropped; they come back.
    let scan = Attachment { ocr_pages: Some(2), unread_pages: Some(1), ..att("file_00000000000000aa", "notice.pdf", "pdf") };
    let mirror = mirror_of(&[conv(vec![q(300, vec![scan.clone()])])]);
    let mut older = vec![conv(vec![q(300, vec![Attachment { ocr_pages: None, unread_pages: None, ..scan.clone() }])])];
    assert_eq!(restore_from(&mut older, &mirror), 1);
    assert_eq!(older[0].messages[0].attachments, vec![scan]);
    assert_eq!(restore_from(&mut older, &mirror), 0);
}

#[test]
fn what_cant_be_read_says_what_it_is() {
    assert!(cant_read("a.zip", "zip").contains("(an archive)"));
    assert!(cant_read("talk.mp3", "mp3").contains("Upload transcribes it"));
    assert_eq!(cant_read("thing.bin", "bin"), "Chat can't read thing.bin. It reads text, code, PDF, Word and PowerPoint files, and pictures.");
    assert!(cant_read("deck.odp", "odp").contains("(an OpenDocument presentation: save it as .pptx to attach it)"));
    assert!(cant_read("deck.key", "key").contains("(a Keynote presentation: export it as PowerPoint to attach it)"));
}
