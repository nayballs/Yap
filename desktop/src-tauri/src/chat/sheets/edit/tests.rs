//! Editing a sheet and saving it: the patcher, the rules, the editors.
//! Workbooks made here (fictional), as `sheets/tests.rs` makes them.

use std::collections::HashMap;
use std::io::Read;

use super::super::patch::{self, area_of, cell_xml, escape, patch_sheet, place_of, scan, CellPut, Put, SheetPatch, Value};
use super::super::save;
use super::super::tests::{cell, f, n, s, seed_order, xlsx_of, STYLES};
use super::super::{read_now, Format};
use super::*;

/// Each entry of a zip: its name and its bytes as stored (compressed as it is).
fn raw_entries(zip: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    let mut out = HashMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        out.insert(f.name().map(|n| n.into_owned()).unwrap(), b);
    }
    out
}

fn part(zip: &[u8], name: &str) -> String {
    String::from_utf8(raw_entries(zip).remove(name).unwrap_or_else(|| panic!("no {name}"))).unwrap()
}

/// `base` with more parts (or a part replaced).
fn with_parts(base: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(String, Vec<u8>)> = {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(base)).unwrap();
        (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).unwrap();
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                (f.name().map(|n| n.into_owned()).unwrap(), b)
            })
            .collect()
    };
    for (name, bytes) in extra {
        match entries.iter_mut().find(|(n, _)| n == name) {
            Some(e) => e.1 = bytes.to_vec(),
            None => entries.push((name.to_string(), bytes.to_vec())),
        }
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, bytes) in &entries {
            z.start_file(name.as_str(), zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
            std::io::Write::write_all(&mut z, bytes).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

fn xlsx_editor(bytes: &[u8]) -> Box<Xlsx> {
    match Editor::open("seed-order.xlsx", Format::Xlsx, bytes).unwrap() {
        Editor::Xlsx(x) => x,
        _ => unreachable!(),
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("yap-sheet-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---- the patcher --------------------------------------------------------------------------------

#[test]
fn cells_are_named_and_written_as_a_file_keeps_them() {
    assert_eq!(place_of("B7"), Some((7, 2)));
    assert_eq!(place_of("$AB$12"), Some((12, 28)));
    assert_eq!(place_of("7B"), None);
    assert_eq!(area_of("D5:A1").map(|a| a.a1()), Some("A1:D5".to_string()));
    assert_eq!(escape("a<b & \"c\"\u{1}_x0041_"), "a&lt;b &amp; &quot;c&quot;_x0001__x005F_x0041_");
    let put = |p: Put, style| CellPut { put: p, style };
    assert_eq!(cell_xml("", "C5", &put(Put::Value(Value::Number(4.0)), 4)).unwrap(), r#"<c r="C5" s="4"><v>4</v></c>"#);
    assert_eq!(cell_xml("", "A1", &put(Put::Value(Value::Text(" Kale & co".into())), 0)).unwrap(), r#"<c r="A1" t="inlineStr"><is><t xml:space="preserve"> Kale &amp; co</t></is></c>"#);
    assert_eq!(cell_xml("x:", "B2", &put(Put::Value(Value::Bool(true)), 0)).unwrap(), r#"<x:c r="B2" t="b"><x:v>1</x:v></x:c>"#);
    assert_eq!(cell_xml("", "D6", &put(Put::Formula("SUM(D3:D5)".into(), Some(Value::Number(27.0))), 4)).unwrap(), r#"<c r="D6" s="4"><f>SUM(D3:D5)</f><v>27</v></c>"#);
    assert_eq!(cell_xml("", "E1", &put(Put::Formula("A1&amp;\"x\"".into(), Some(Value::Text("ax".into()))), 0)).unwrap(), r#"<c r="E1" t="str"><f>A1&amp;amp;&quot;x&quot;</f><v>ax</v></c>"#);
    assert_eq!(cell_xml("", "F1", &put(Put::Formula("1/0".into(), Some(Value::Error("#DIV/0!".into()))), 0)).unwrap(), r#"<c r="F1" t="e"><f>1/0</f><v>#DIV/0!</v></c>"#);
    // Emptied: a styled cell stays as its style; a plain one goes.
    assert_eq!(cell_xml("", "G1", &put(Put::Empty, 3)).unwrap(), r#"<c r="G1" s="3"/>"#);
    assert_eq!(cell_xml("", "G1", &put(Put::Empty, 0)), None);
    assert_eq!(patch::number(0.1 + 0.2), "0.30000000000000004");
    assert_eq!(patch::number(1e21), "1000000000000000000000");
}

const SHEET: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:C6"/><sheetData><row r="1" spans="1:3"><c r="A1" t="s"><v>0</v></c><c r="C1" s="2"><v>3</v></c></row><row r="4"><c r="A4"><v>1</v></c><c r="C4"><v>2</v></c></row><row r="5"/><row r="6"><c r="A6"><f>SUM(A1:A5)</f><v>1</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells><dataValidations count="1"><dataValidation type="list" sqref="C4"><formula1>"1,2,3"</formula1></dataValidation></dataValidations></worksheet>"#;

fn puts(list: &[((u32, u32), Put, u32)]) -> SheetPatch {
    SheetPatch { puts: list.iter().map(|(at, p, s)| (*at, CellPut { put: p.clone(), style: *s })).collect(), results: HashMap::new() }
}

#[test]
fn a_patch_changes_only_the_cells_it_names() {
    // One cell replaced: every other byte as it was.
    let out = patch_sheet(SHEET, &puts(&[((4, 3), Put::Value(Value::Number(9.0)), 0)])).unwrap();
    assert_eq!(out, SHEET.replace(r#"<c r="C4"><v>2</v></c>"#, r#"<c r="C4"><v>9</v></c>"#));
    // Put in: between two cells, past a row's last one, a row of its own between two, into an empty row,
    // after the last row; the dimension grown, a row's `spans` hint dropped.
    let out = patch_sheet(
        SHEET,
        &puts(&[((4, 2), Put::Value(Value::Text("Kale".into())), 0), ((1, 5), Put::Value(Value::Number(5.0)), 0), ((3, 1), Put::Value(Value::Bool(false)), 0), ((5, 2), Put::Value(Value::Number(7.0)), 0), ((9, 1), Put::Value(Value::Number(1.0)), 0)]),
    )
    .unwrap();
    assert!(out.contains(r#"<dimension ref="A1:E9"/>"#), "{out}");
    assert!(out.contains(r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" s="2"><v>3</v></c><c r="E1"><v>5</v></c></row>"#), "{out}");
    assert!(out.contains(r#"</row><row r="3"><c r="A3" t="b"><v>0</v></c></row><row r="4">"#), "{out}");
    assert!(out.contains(r#"<c r="A4"><v>1</v></c><c r="B4" t="inlineStr"><is><t>Kale</t></is></c><c r="C4"><v>2</v></c>"#), "{out}");
    assert!(out.contains(r#"<row r="5"><c r="B5"><v>7</v></c></row>"#), "{out}");
    assert!(out.contains(r#"<row r="9"><c r="A9"><v>1</v></c></row></sheetData>"#), "{out}");
    assert!(out.contains("<dataValidations count=\"1\">"), "what Yap doesn't know is kept");
    // A cell emptied (no style): it goes.
    let out = patch_sheet(SHEET, &puts(&[((1, 1), Put::Empty, 0)])).unwrap();
    assert!(out.contains(r#"<row r="1"><c r="C1" s="2"><v>3</v></c></row>"#), "{out}");
    // Into `<sheetData/>`.
    let bare = r#"<worksheet xmlns="m"><sheetData/></worksheet>"#;
    assert_eq!(patch_sheet(bare, &puts(&[((2, 2), Put::Value(Value::Number(1.0)), 0)])).unwrap(), r#"<worksheet xmlns="m"><sheetData><row r="2"><c r="B2"><v>1</v></c></row></sheetData></worksheet>"#);
}

#[test]
fn a_prefixed_sheet_is_patched_in_its_own_prefix() {
    let xml = r#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;
    let out = patch_sheet(xml, &puts(&[((1, 2), Put::Value(Value::Number(2.0)), 0), ((2, 1), Put::Value(Value::Text("b".into())), 0)])).unwrap();
    assert_eq!(out, r#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>1</x:v></x:c><x:c r="B1"><x:v>2</x:v></x:c></x:row><x:row r="2"><x:c r="A2" t="inlineStr"><x:is><x:t>b</x:t></x:is></x:c></x:row></x:sheetData></x:worksheet>"#);
}

#[test]
fn formula_results_are_written_only_where_they_changed() {
    let mut p = SheetPatch::default();
    p.results.insert((6, 1), Value::Number(1.0));
    assert_eq!(patch_sheet(SHEET, &p).unwrap(), SHEET, "the same result: nothing touched");
    p.results.insert((6, 1), Value::Number(12.5));
    let out = patch_sheet(SHEET, &p).unwrap();
    assert_eq!(out, SHEET.replace("<f>SUM(A1:A5)</f><v>1</v>", "<f>SUM(A1:A5)</f><v>12.5</v>"));
    // A shared formula's own `<f>` kept as written; a text result's type.
    let xml = r#"<worksheet><sheetData><row r="1"><c r="B1"><f t="shared" ref="B1:B2" si="0">A1&amp;"!"</f><v>x!</v></c></row><row r="2"><c r="B2" t="str"><f t="shared" si="0"/><v>y!</v></c></row></sheetData></worksheet>"#;
    let mut p = SheetPatch::default();
    p.results.insert((2, 2), Value::Text("z & y!".into()));
    let out = patch_sheet(xml, &p).unwrap();
    assert!(out.contains(r#"<c r="B2" t="str"><f t="shared" si="0"/><v>z &amp; y!</v></c>"#), "{out}");
    assert!(out.contains(r#"<f t="shared" ref="B1:B2" si="0">A1&amp;"!"</f><v>x!</v>"#), "{out}");
    let scanned = scan(xml).unwrap();
    assert_eq!(scanned.master_of((1, 2)).map(|s| s.cells.clone()), Some(vec![(1, 2), (2, 2)]));
}

#[test]
fn a_sheet_with_rows_or_cells_unnamed_isnt_patched() {
    assert!(scan(r#"<worksheet><sheetData><row><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#).is_err());
    assert!(scan(r#"<worksheet><sheetData><row r="1"><c><v>1</v></c></row></sheetData></worksheet>"#).is_err());
    let arrays = scan(r#"<worksheet><sheetData><row r="1"><c r="B1"><f t="array" ref="B1:B3">A1:A3*2</f><v>2</v></c></row></sheetData></worksheet>"#).unwrap();
    assert!(arrays.in_array((2, 2)) && !arrays.in_array((1, 1)));
}

#[test]
fn the_workbook_asks_excel_to_calculate_and_its_calc_chain_goes() {
    let wb = r#"<workbook xmlns="m"><bookViews/><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets><calcPr calcId="191029"/></workbook>"#;
    assert_eq!(save::workbook_xml(wb), r#"<workbook xmlns="m"><bookViews/><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#);
    let none = r#"<workbook xmlns="m"><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets><definedNames/><extLst/></workbook>"#;
    assert_eq!(save::workbook_xml(none), r#"<workbook xmlns="m"><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets><definedNames/><calcPr fullCalcOnLoad="1"/><extLst/></workbook>"#);
    let rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId9" Type="http://x/calcChain" Target="calcChain.xml"/></Relationships>"#;
    let types = r#"<Types><Override PartName="/xl/calcChain.xml" ContentType="c"/><Override PartName="/xl/workbook.xml" ContentType="w"/></Types>"#;
    let (rels, types) = save::without_calc_chain(rels, types);
    assert_eq!(rels, r#"<Relationships><Relationship Id="rId1" Type="http://x/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#);
    assert_eq!(types, r#"<Types><Override PartName="/xl/workbook.xml" ContentType="w"/></Types>"#);
}

#[test]
fn a_typed_format_adds_its_style_to_the_file() {
    assert_eq!(save::with_count(r#"<cellXfs count="9">"#, 10), r#"<cellXfs count="10">"#);
    assert_eq!(save::with_count("<cellXfs>", 2), r#"<cellXfs count="2">"#);
    assert_eq!(save::xf_with(r#"<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>"#, 9, false).unwrap(), r#"<xf fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1" numFmtId="9" applyNumberFormat="1"/>"#);
    assert_eq!(save::xf_with(r#"<xf numFmtId="0" fontId="2"><alignment horizontal="center"/></xf>"#, 0, true).unwrap(), r#"<xf fontId="2" numFmtId="0" quotePrefix="1"><alignment horizontal="center"/></xf>"#);
    let styles = format!(r#"<styleSheet xmlns="m">{STYLES}</styleSheet>"#);
    let more = save::styles_with(&styles, &[r#"<xf numFmtId="166"/>"#.to_string()], &[(166, "0.0%".to_string())]).unwrap();
    assert!(more.contains(r#"<numFmts count="3">"#) && more.contains(r#"<numFmt numFmtId="166" formatCode="0.0%"/></numFmts>"#), "{more}");
    assert!(more.contains(r#"<cellXfs count="10">"#) && more.contains(r#"<xf numFmtId="166"/></cellXfs>"#), "{more}");
    // None yet: made first.
    let bare = r#"<styleSheet xmlns="m"><fonts/><cellXfs count="1"><xf/></cellXfs></styleSheet>"#;
    assert_eq!(save::styles_with(bare, &[], &[(164, "0".into())]).unwrap(), r#"<styleSheet xmlns="m"><numFmts count="1"><numFmt numFmtId="164" formatCode="0"/></numFmts><fonts/><cellXfs count="1"><xf/></cellXfs></styleSheet>"#);
}

// ---- an .xlsx, edited and saved -----------------------------------------------------------------

#[test]
fn an_edited_workbook_is_calculated_and_saves_only_what_changed() {
    let original = seed_order();
    let mut x = xlsx_editor(&original);
    assert!(x.changed().next().is_none());
    // Kale's price: 3 → 4 (C5), as typed.
    x.edit(0, 5, 3, "4").unwrap();
    let book = super::super::xlsx::book_of(x.model.get_model(), &x.kept);
    assert_eq!(cell(&book.sheets[0], 5, 3).text, "£4.00", "its pounds kept");
    assert_eq!(cell(&book.sheets[0], 5, 4).text, "£16.00");
    assert_eq!(cell(&book.sheets[0], 6, 4).text, "£27.00", "the total calculated again");
    let written = save::xlsx_bytes(&x).unwrap();
    let saved = written.bytes.clone();
    // Read back as the view reads a file: the change and the total.
    let again = read_now(Format::Xlsx, &saved).unwrap();
    assert_eq!(cell(&again.sheets[0], 5, 3).text, "£4.00");
    assert_eq!(cell(&again.sheets[0], 6, 4).text, "£27.00");
    let sheet1 = part(&saved, "xl/worksheets/sheet1.xml");
    assert!(sheet1.contains(r#"<c r="C5" s="4"><v>4</v></c>"#), "{sheet1}");
    assert!(sheet1.contains(r#"<c r="D6" s="4"><f>SUM(D3:D5)</f><v>27</v></c>"#), "each result in the file too: {sheet1}");
    // Every other part as it was: the other sheets, the styles, the relationships.
    let (before, after) = (raw_entries(&original), raw_entries(&saved));
    for name in ["xl/worksheets/sheet2.xml", "xl/worksheets/sheet3.xml", "xl/styles.xml", "xl/_rels/workbook.xml.rels", "[Content_Types].xml", "_rels/.rels"] {
        assert_eq!(before[name], after[name], "{name} kept as it was");
    }
    assert_eq!(before.len(), after.len());
    assert!(part(&saved, "xl/workbook.xml").contains(r#"<calcPr fullCalcOnLoad="1"/>"#));
    // Saved: nothing changed any more; Undo back to the file's 3 is a change again.
    x.saved(written);
    assert!(x.changed().next().is_none());
    assert_eq!(x.undo(), Some((0, 5, 3)));
    assert_eq!(x.changed().count(), 1);
    assert_eq!(x.redo(), Some((0, 5, 3)));
    assert!(x.changed().next().is_none(), "Redo: the file's again");
}

#[test]
fn undo_back_to_the_file_is_no_change_and_a_formula_typed_is_calculated() {
    let mut e = Editor::open("seed-order.xlsx", Format::Xlsx, &seed_order()).unwrap();
    e.edit(0, 3, 2, "5").unwrap();
    e.edit(0, 8, 2, "=SUM(B3:B5)").unwrap();
    assert!(e.dirty());
    let book = e.book();
    assert_eq!(cell(&book.sheets[0], 8, 2).text, "11");
    assert_eq!(cell(&book.sheets[0], 8, 2).formula.as_deref(), Some("=SUM(B3:B5)"));
    assert_eq!(cell(&book.sheets[0], 3, 4).text, "£12.50");
    // A number's input for the formula bar: as typed, not as shown.
    assert_eq!(cell(&book.sheets[0], 3, 3).input.as_deref(), Some("2.5"));
    assert_eq!(e.undo(), Some((0, 8, 2)));
    assert_eq!(e.undo(), Some((0, 3, 2)));
    assert!(!e.dirty(), "back to the file");
    assert!(e.can_redo() && !e.can_undo());
    assert_eq!(e.undo(), None);
}

#[test]
fn a_typed_per_cent_is_saved_with_its_format() {
    // A workbook with one plain style: a per cent typed needs a new one, made from the cell's own.
    let only_xfs = r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#;
    let rows = format!("<sheetData><row r=\"1\">{}{}</row></sheetData>", s("A1", "Germination", 0), n("B1", 0.5, 0));
    let mut x = xlsx_editor(&xlsx_of(&[("S", "", rows)], only_xfs));
    x.edit(0, 1, 2, "12%").unwrap();
    let shown = cell(&super::super::xlsx::book_of(x.model.get_model(), &x.kept).sheets[0], 1, 2).text.clone();
    assert_eq!(shown, "12%");
    let written = save::xlsx_bytes(&x).unwrap();
    let saved = written.bytes.clone();
    // IronCalc's own format (its id 50, a built-in's in Excel) under an id of the file's own; the
    // file's styles as it wrote them (the parts IronCalc needed filled in stay out of the file).
    let styles = part(&saved, "xl/styles.xml");
    assert!(styles.contains(r##"<numFmts count="1"><numFmt numFmtId="164" formatCode="#,##0%"/></numFmts>"##), "{styles}");
    assert!(styles.contains(r#"<cellXfs count="2">"#), "{styles}");
    assert!(styles.contains(r#"numFmtId="164" applyNumberFormat="1"/></cellXfs>"#), "{styles}");
    assert!(!styles.contains("<fonts"), "{styles}");
    assert!(part(&saved, "xl/worksheets/sheet1.xml").contains(r#"<c r="B1" s="1"><v>0.12</v></c>"#));
    let again = read_now(Format::Xlsx, &saved).unwrap();
    assert_eq!(cell(&again.sheets[0], 1, 2).text, "12%");
    // Saved, then another per cent: the style the first save added, none added again.
    x.saved(written);
    x.edit(0, 2, 2, "40%").unwrap();
    let next = save::xlsx_bytes(&x).unwrap().bytes;
    let styles = part(&next, "xl/styles.xml");
    assert!(styles.contains(r#"<cellXfs count="2">"#) && styles.contains(r#"<numFmts count="1">"#), "{styles}");
    assert!(part(&next, "xl/worksheets/sheet1.xml").contains(r#"<c r="B2" s="1"><v>0.4</v></c>"#));
    assert_eq!(cell(&read_now(Format::Xlsx, &next).unwrap().sheets[0], 2, 2).text, "40%");
}

#[test]
fn a_bare_pound_format_is_kept_as_the_file_has_it() {
    let styles = STYLES.replace(r#"<numFmt numFmtId="164" formatCode="&quot;£&quot;#,##0.00"/>"#, r#"<numFmt numFmtId="164" formatCode="£#,##0.00"/>"#);
    let rows = format!("<sheetData><row r=\"1\">{}{}{}</row></sheetData>", n("A1", 4.0, 0), n("B1", 6.5, 4), f("C1", "A1*B1", 4));
    let book = xlsx_of(&[("Budget", "", rows)], &styles);
    let mut x = xlsx_editor(&book);
    x.edit(0, 1, 2, "7").unwrap();
    let saved = save::xlsx_bytes(&x).unwrap().bytes;
    assert_eq!(raw_entries(&book)["xl/styles.xml"], raw_entries(&saved)["xl/styles.xml"], "its format as it wrote it");
    let again = read_now(Format::Xlsx, &saved).unwrap();
    assert_eq!(cell(&again.sheets[0], 1, 2).text, "£7.00");
    assert_eq!(cell(&again.sheets[0], 1, 3).text, "£28.00");
}

#[test]
fn a_shared_formulas_first_cell_changed_writes_the_others_out() {
    let rows = format!(
        r#"<sheetData><row r="1">{}<c r="B1"><f t="shared" ref="B1:B3" si="0">A1*2</f></c></row><row r="2">{}<c r="B2"><f t="shared" si="0"/></c></row><row r="3">{}<c r="B3"><f t="shared" si="0"/></c></row></sheetData>"#,
        n("A1", 1.0, 0),
        n("A2", 2.0, 0),
        n("A3", 3.0, 0)
    );
    let mut x = xlsx_editor(&xlsx_of(&[("S", "", rows)], STYLES));
    x.edit(0, 1, 2, "100").unwrap();
    let saved = save::xlsx_bytes(&x).unwrap().bytes;
    let sheet = part(&saved, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(r#"<c r="B2"><f>A2*2</f><v>4</v></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="B3"><f>A3*2</f><v>6</v></c>"#), "{sheet}");
    let again = read_now(Format::Xlsx, &saved).unwrap();
    assert_eq!(cell(&again.sheets[0], 3, 2).text, "6");
    assert_eq!(cell(&again.sheets[0], 1, 2).text, "100");
}

#[test]
fn whatever_yap_doesnt_know_is_kept_and_the_calc_chain_goes() {
    // A chart, a picture, comments, macros, data validation, conditional formatting, a tab colour, page setup.
    let budget = format!(
        r#"<sheetPr><tabColor rgb="FF00B050"/></sheetPr><sheetData><row r="1">{}{}</row><row r="2">{}{}</row></sheetData><conditionalFormatting sqref="B2"><cfRule type="colorScale" priority="1"><colorScale><cfvo type="min"/><cfvo type="max"/><color rgb="FFF8696B"/><color rgb="FF63BE7B"/></colorScale></cfRule></conditionalFormatting><dataValidations count="1"><dataValidation type="whole" sqref="B2"><formula1>0</formula1></dataValidation></dataValidations><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" orientation="landscape"/><drawing r:id="rId1"/><legacyDrawing r:id="rId2"/>"#,
        s("A1", "Crop", 1),
        s("B1", "Kilos", 1),
        s("A2", "Kale", 0),
        n("B2", 4.0, 0)
    );
    let notes = format!("<sheetData><row r=\"1\">{}</row></sheetData>", f("A1", "Budget!B2*2", 0));
    let base = xlsx_of(&[("Budget", "", budget), ("Notes", "", notes)], STYLES);
    let chart = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart/></c:chartSpace>"#;
    let comments = br#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Sam</author></authors><commentList><comment ref="B2" authorId="0"><text><t>Weighed on Friday</t></text></comment></commentList></comments>"#;
    let macros: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 1, 2, 3, 4, 5];
    let calc_chain = br#"<calcChain xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><c r="A1" i="2"/></calcChain>"#;
    let rels = part(&base, "xl/_rels/workbook.xml.rels").replace("</Relationships>", r#"<Relationship Id="rId50" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/></Relationships>"#);
    let types = part(&base, "[Content_Types].xml").replace("</Types>", r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#);
    let book = with_parts(&base, &[("xl/charts/chart1.xml", chart), ("xl/drawings/drawing1.xml", b"<xdr:wsDr/>"), ("xl/media/image1.png", &[0x89, b'P', b'N', b'G']), ("xl/comments1.xml", comments), ("xl/vbaProject.bin", macros), ("xl/calcChain.xml", calc_chain), ("xl/_rels/workbook.xml.rels", rels.as_bytes()), ("[Content_Types].xml", types.as_bytes())]);
    let mut x = xlsx_editor(&book);
    x.edit(0, 2, 2, "12").unwrap();
    let saved = save::xlsx_bytes(&x).unwrap().bytes;
    let (before, after) = (raw_entries(&book), raw_entries(&saved));
    for name in ["xl/charts/chart1.xml", "xl/drawings/drawing1.xml", "xl/media/image1.png", "xl/comments1.xml", "xl/vbaProject.bin", "xl/styles.xml"] {
        assert_eq!(before[name], after[name], "{name} kept as it was");
    }
    let sheet = part(&saved, "xl/worksheets/sheet1.xml");
    for kept in [r#"<tabColor rgb="FF00B050"/>"#, "<conditionalFormatting", "<dataValidations", r#"<pageSetup paperSize="9" orientation="landscape"/>"#, r#"<drawing r:id="rId1"/>"#, r#"<legacyDrawing r:id="rId2"/>"#] {
        assert!(sheet.contains(kept), "{kept} kept: {sheet}");
    }
    assert!(sheet.contains(r#"<c r="B2"><v>12</v></c>"#), "{sheet}");
    // The other sheet's formula on this one: its result brought up to date.
    assert!(part(&saved, "xl/worksheets/sheet2.xml").contains("<f>Budget!B2*2</f><v>24</v>"));
    assert!(!after.contains_key("xl/calcChain.xml"));
    assert!(!part(&saved, "xl/_rels/workbook.xml.rels").contains("calcChain"));
    assert!(!part(&saved, "[Content_Types].xml").contains("calcChain"));
}

#[test]
fn cells_yap_cant_write_as_excel_would_are_refused_plainly() {
    let rows = format!(r#"<sheetData><row r="1">{}<c r="B1"><f t="array" ref="B1:B2">A1:A2*2</f><v>2</v></c></row><row r="2">{}<c r="B2"><v>4</v></c></row></sheetData>"#, n("A1", 1.0, 0), n("A2", 2.0, 0));
    let mut x = xlsx_editor(&xlsx_of(&[("S", "", rows)], STYLES));
    let err = x.edit(0, 2, 2, "9").unwrap_err();
    assert!(err.contains("array formula"), "{err}");
    assert!(x.changed().next().is_none());
    // A sheet whose cells aren't named: read only from the start.
    let unnamed = r#"<sheetData><row><c><v>1</v></c></row></sheetData>"#.to_string();
    let book = xlsx_of(&[("S", "", unnamed)], STYLES);
    if read_now(Format::Xlsx, &book).is_ok() {
        let err = Editor::open("odd.xlsx", Format::Xlsx, &book).err().unwrap_or_default();
        assert!(err.contains("read only"), "{err}");
    }
    // Read only: an attached copy, an .xls or an .ods, a file Windows marks read-only.
    assert_eq!(refused("a.xlsx", Format::Xlsx, false, true, false).as_deref(), Some("This is the copy attached to the chat, so it's read only."));
    assert!(refused("old.xls", Format::Xls, true, true, false).unwrap().contains("open it in Excel"));
    assert!(refused("harvest.ods", Format::Ods, true, true, false).unwrap().contains("spreadsheet app"));
    assert!(refused("a.xlsx", Format::Xlsx, true, false, false).unwrap().contains("couldn't calculate"));
    assert!(refused("a.csv", Format::Csv, true, true, true).unwrap().contains("read-only"));
    assert_eq!(refused("a.csv", Format::Csv, true, true, false), None);
}

// ---- a .csv -----------------------------------------------------------------------------------

#[test]
fn a_csv_edit_rewrites_its_one_record_in_the_files_own_form() {
    let file = b"\xEF\xBB\xBFBed,Crop,Sown\r\n1,\"Broad beans\",\"3 March, under fleece\"\r\n2,Leeks,12 April\r\n";
    let mut e = Editor::open("beds.csv", Format::Csv, file).unwrap();
    assert!(!e.dirty());
    e.edit(0, 3, 2, "Onions, red").unwrap();
    let Editor::Delimited(d) = &e else { unreachable!() };
    assert_eq!(d.bytes(), b"\xEF\xBB\xBFBed,Crop,Sown\r\n1,\"Broad beans\",\"3 March, under fleece\"\r\n2,\"Onions, red\",12 April\r\n".to_vec(), "its mark, its line endings, the other records' quotes");
    // A row past the end, rows between as empty records.
    e.edit(0, 6, 1, "5").unwrap();
    let Editor::Delimited(d) = &e else { unreachable!() };
    assert!(d.text.ends_with("2,\"Onions, red\",12 April\n,,\n,,\n5\n"), "{:?}", d.text);
    let book = e.book();
    assert_eq!(cell(&book.sheets[0], 6, 1).text, "5");
    assert_eq!(cell(&book.sheets[0], 3, 2).text, "Onions, red");
    // A CSV keeps what's typed as typed: a formula too.
    e.edit(0, 2, 4, "=A2+1").unwrap();
    assert_eq!(cell(&e.book().sheets[0], 2, 4).text, "=A2+1");
    // Undo all the way: the file's own text again.
    while e.undo().is_some() {}
    assert!(!e.dirty());
    assert_eq!(e.redo(), Some((0, 3, 2)));
}

#[test]
fn a_csvs_records_are_numbered_as_the_view_numbers_its_rows() {
    let text = "a,b\n\nc,\"d\ne\"\nf,g\n";
    let recs = records(text, b',');
    let book = super::super::delimited::read_text(text, Format::Csv);
    for (i, rec) in recs.iter().enumerate() {
        let r = i as u32 + 1;
        if let Some(row) = book.sheets[0].rows.iter().find(|x| x.r == r) {
            assert_eq!(row.cells[0].text, rec.fields[0], "row {r}");
        }
    }
    // (Whatever a blank line is to the reader, the editor's row 3 is the view's row 3.)
    let third = recs[2].fields[0].clone();
    let changed = with_field(text, b',', 3, 2, "h");
    assert_eq!(super::super::delimited::read_text(&changed, Format::Csv).sheets[0].rows.iter().find(|x| x.r == 3).map(|r| r.cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>()), Some(vec![third, "h".to_string()]));
    assert_eq!(changed[..recs[2].start], text[..recs[2].start], "the records before it as they were");
    // A value emptied at the end of a record: no new separators.
    assert_eq!(with_field("a,b\n", b',', 1, 5, ""), "a,b\n");
    assert_eq!(with_field("a,b\n", b',', 1, 2, ""), "a,\n");
    assert_eq!(record_text(&["x\"y".into(), String::new()], b';'), "\"x\"\"y\";");
}

#[test]
fn a_csv_in_another_encoding_stays_read_only() {
    let err = Editor::open("prices.csv", Format::Csv, b"Item,Price\nSeeds,\xA35\n").err().unwrap_or_default();
    assert!(err.contains("another encoding"), "{err}");
}

// ---- the rule: never over someone else's change -------------------------------------------------

#[test]
fn a_save_writes_only_while_the_file_is_what_was_read() {
    let dir = temp_dir("rule");
    let path = dir.join("beds.csv");
    std::fs::write(&path, "Bed,Crop\n1,Kale\n").unwrap();
    let stamp = super::super::view::Stamp::of(&path).unwrap();
    let hash = save::hash_of(&std::fs::read(&path).unwrap());
    assert!(save::unchanged(&path, stamp, hash));
    // Someone else writes it: the same size and, put back, the same time — but not the same bytes.
    let when = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, "Bed,Crop\n2,Leek\n").unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
    assert_eq!(super::super::view::Stamp::of(&path), Some(stamp));
    assert!(!save::unchanged(&path, stamp, hash), "the bytes differ");
    std::fs::write(&path, "Bed,Crop\n1,Kale\n3,Peas\n").unwrap();
    assert!(!save::unchanged(&path, stamp, hash));
    let _ = std::fs::remove_file(&path);
    assert!(!save::unchanged(&path, stamp, hash), "gone");
    // A copy's name: one not taken.
    std::fs::write(dir.join("seed-order (copy).xlsx"), b"x").unwrap();
    assert_eq!(save::copy_name(&dir, "seed-order.xlsx"), "seed-order (copy 2).xlsx");
    assert_eq!(save::copy_name(&dir, "beds.csv"), "beds (copy).csv");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_view_saves_edits_and_holds_back_over_a_change_on_disk() {
    use super::super::view;
    let dir = temp_dir("view");
    let path = dir.join("seed-order.xlsx");
    std::fs::write(&path, seed_order()).unwrap();
    let folder = Some(dir.to_string_lossy().into_owned());
    let opened = view::open(path.clone(), "seed-order.xlsx".into(), folder.clone()).unwrap();
    assert_eq!(opened.edit.note, None, "a Code chat's .xlsx can be edited");
    let edited = view::edit_cell(opened.id, 0, 5, 3, "4").unwrap();
    assert!(edited.book.edit.dirty && edited.book.edit.can_undo);
    let rows = view::sheet_rows(opened.id, 0, 6, 6).unwrap();
    assert_eq!(rows[0].cells.iter().find(|c| c.col == 4).map(|c| c.text.clone()).as_deref(), Some("£27.00"));
    // Saved: the file has the change.
    let out = view::save(opened.id).unwrap();
    assert!(out.saved && !out.conflict && !out.edit.dirty);
    assert_eq!(cell(&read_now(Format::Xlsx, &std::fs::read(&path).unwrap()).unwrap().sheets[0], 6, 4).text, "£27.00");
    // Changed on disk under a change not saved: nothing written; opening it again keeps the change and says so.
    view::edit_cell(opened.id, 0, 3, 2, "10").unwrap();
    let theirs = super::super::tests::xlsx_of(&[("Budget", "", "<sheetData/>".to_string())], STYLES);
    std::fs::write(&path, &theirs).unwrap();
    let out = view::save(opened.id).unwrap();
    assert!(!out.saved && out.conflict);
    assert_eq!(std::fs::read(&path).unwrap(), theirs, "theirs kept");
    let again = view::open(path.clone(), "seed-order.xlsx".into(), folder.clone()).unwrap();
    assert_eq!(again.id, opened.id);
    assert!(again.edit.dirty && again.edit.conflict);
    // Save as a copy: beside it, with the changes; the original read again next time.
    let copy = view::save_copy(opened.id).unwrap();
    assert_eq!(copy, "seed-order (copy).xlsx");
    let copied = read_now(Format::Xlsx, &std::fs::read(dir.join(&copy)).unwrap()).unwrap();
    assert_eq!(cell(&copied.sheets[0], 3, 4).text, "£25.00");
    let fresh = view::open(path.clone(), "seed-order.xlsx".into(), folder).unwrap();
    assert_ne!(fresh.id, opened.id);
    assert!(!fresh.edit.dirty);
    // An attached copy: read only.
    std::fs::write(dir.join("attached.xlsx"), seed_order()).unwrap();
    let attached = view::open(dir.join("attached.xlsx"), "attached.xlsx".into(), None).unwrap();
    assert!(attached.edit.note.is_some());
    assert!(view::edit_cell(attached.id, 0, 1, 1, "x").unwrap_err().contains("read only"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- rows and columns, Alt+Enter, the locale (16 Nov 2026) ----------------------------------------

#[test]
fn a_row_put_in_inside_the_totals_is_counted_saved_and_undone() {
    let original = seed_order();
    let mut e = Editor::open("seed-order.xlsx", Format::Xlsx, &original).unwrap();
    // A row before row 5 (Kale): inside the total's D3:D5.
    assert_eq!(e.lines(0, Axis::Rows, 5, 1, true).unwrap(), (0, 5, 1));
    assert!(e.dirty(), "a row put in is a change to save");
    let book = e.book();
    assert_eq!(cell(&book.sheets[0], 7, 4).formula.as_deref(), Some("=SUM(D3:D6)"));
    assert_eq!(cell(&book.sheets[0], 6, 1).text, "Kale", "moved down");
    assert_eq!(cell(&book.sheets[0], 3, 6).formula.as_deref(), Some("=D3/D$7"));
    // Typed into the new row: it has the row above's looks (pounds), and the total takes it in.
    e.edit(0, 5, 1, "Shallots").unwrap();
    e.edit(0, 5, 2, "2").unwrap();
    e.edit(0, 5, 3, "1.5").unwrap();
    e.edit(0, 5, 4, "=B5*C5").unwrap();
    let book = e.book();
    assert_eq!(cell(&book.sheets[0], 5, 3).text, "£1.50");
    assert_eq!(cell(&book.sheets[0], 7, 4).text, "£26.00", "23 + 3");
    // Saved: the file has the row, the total over it, the merge and the other sheets as they were.
    let Editor::Xlsx(x) = &e else { unreachable!() };
    let written = save::xlsx_bytes(x).unwrap();
    let again = read_now(Format::Xlsx, &written.bytes).unwrap();
    assert_eq!(cell(&again.sheets[0], 7, 4).text, "£26.00");
    assert_eq!(cell(&again.sheets[0], 5, 1).text, "Shallots");
    assert_eq!(again.sheets[0].merges.len(), 1);
    let sheet1 = part(&written.bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet1.contains("<f>SUM(D3:D6)</f>"), "{sheet1}");
    let (before, after) = (raw_entries(&original), raw_entries(&written.bytes));
    for name in ["xl/worksheets/sheet2.xml", "xl/worksheets/sheet3.xml"] {
        assert_eq!(before[name], after[name], "{name} kept as it was");
    }
    // Undo: the cells, then the row; back to the file, no change at all. Redo puts the row back.
    for _ in 0..4 {
        e.undo().unwrap();
    }
    assert!(e.dirty(), "the row's still in");
    assert_eq!(e.undo(), Some((0, 5, 1)));
    assert!(!e.dirty(), "back to the file");
    assert_eq!(cell(&e.book().sheets[0], 6, 4).formula.as_deref(), Some("=SUM(D3:D5)"));
    assert_eq!(e.redo(), Some((0, 5, 1)));
    assert_eq!(cell(&e.book().sheets[0], 7, 4).formula.as_deref(), Some("=SUM(D3:D6)"));
    assert_eq!(e.redo(), Some((0, 5, 1)), "then the name typed");
    assert_eq!(cell(&e.book().sheets[0], 5, 1).text, "Shallots");
}

#[test]
fn a_row_taken_out_shrinks_the_total_and_a_column_moves_its_formulas() {
    let mut e = Editor::open("seed-order.xlsx", Format::Xlsx, &seed_order()).unwrap();
    // The total's last row taken out: SUM(D3:D4), as Excel has it (IronCalc's own Delete would say #REF!).
    e.lines(0, Axis::Rows, 5, 1, false).unwrap();
    let book = e.book();
    assert_eq!(cell(&book.sheets[0], 5, 4).formula.as_deref(), Some("=SUM(D3:D4)"));
    assert_eq!(cell(&book.sheets[0], 5, 4).text, "£11.00");
    e.undo().unwrap();
    // A column put in before C: the prices and totals move right, every formula with them.
    e.lines(0, Axis::Cols, 3, 1, true).unwrap();
    let book = e.book();
    assert_eq!(cell(&book.sheets[0], 6, 5).formula.as_deref(), Some("=SUM(E3:E5)"));
    assert_eq!(cell(&book.sheets[0], 3, 5).formula.as_deref(), Some("=B3*D3"));
    assert_eq!(cell(&book.sheets[0], 6, 5).text, "£23.00");
    assert_eq!(book.sheets[0].merges[0].cols, 5, "the title's merge grown over the new column");
}

#[test]
fn rows_yap_cant_move_safely_are_refused_and_nothing_changes() {
    let xml = format!(r#"<sheetData><row r="1">{}</row><row r="2">{}<c r="B2"><f t="array" ref="B2:B4">A1:A3*2</f></c></row></sheetData>"#, n("A1", 1.0, 0), n("A2", 2.0, 0));
    let bytes = super::super::tests::xlsx_of(&[("Beds", "", xml)], STYLES);
    let mut e = Editor::open("beds.xlsx", Format::Xlsx, &bytes).unwrap();
    let why = e.lines(0, Axis::Rows, 3, 1, true).unwrap_err();
    assert!(why.contains("insert a row") && why.contains("array formula"), "{why}");
    assert!(!e.dirty() && !e.can_undo());
    assert!(e.lines(0, Axis::Rows, 0, 1, true).is_err());
}

#[test]
fn a_csvs_rows_and_columns_go_in_and_out_record_by_record() {
    let text = "Bed,Crop,Sown\n1,\"Broad beans\",\"3 March, under fleece\"\n2,Leeks,12 April\n";
    let r = |at, insert| Shift { axis: Axis::Rows, at, n: 1, insert };
    let c = |at, insert| Shift { axis: Axis::Cols, at, n: 1, insert };
    assert_eq!(with_lines(text, b',', &r(2, true)), "Bed,Crop,Sown\n,,\n1,\"Broad beans\",\"3 March, under fleece\"\n2,Leeks,12 April\n");
    assert_eq!(with_lines(text, b',', &r(2, false)), "Bed,Crop,Sown\n2,Leeks,12 April\n");
    assert_eq!(with_lines(text, b',', &r(3, false)), "Bed,Crop,Sown\n1,\"Broad beans\",\"3 March, under fleece\"\n");
    assert_eq!(with_lines(text, b',', &r(9, true)), text, "past the last record: nothing");
    assert_eq!(with_lines(text, b',', &c(2, true)), "Bed,,Crop,Sown\n1,,Broad beans,\"3 March, under fleece\"\n2,,Leeks,12 April\n");
    assert_eq!(with_lines(text, b',', &c(1, false)), "Crop,Sown\nBroad beans,\"3 March, under fleece\"\nLeeks,12 April\n");
    assert_eq!(with_lines("a\nb,c\n", b',', &c(1, false)), "\"\"\nc\n", "a record left with nothing is one empty field");
    let mut e = Editor::open("beds.csv", Format::Csv, text.as_bytes()).unwrap();
    e.lines(0, Axis::Rows, 2, 1, false).unwrap();
    assert_eq!(cell(&e.book().sheets[0], 2, 2).text, "Leeks");
    assert!(e.dirty());
    e.undo().unwrap();
    assert!(!e.dirty());
}

#[test]
fn alt_enter_s_line_break_wraps_the_cell_and_is_saved_with_it() {
    let original = seed_order();
    let mut e = Editor::open("seed-order.xlsx", Format::Xlsx, &original).unwrap();
    e.edit(0, 9, 1, "Order by Friday\nPlot 7 first").unwrap();
    let book = e.book();
    let c = cell(&book.sheets[0], 9, 1);
    assert_eq!(c.text, "Order by Friday\nPlot 7 first");
    assert!(book.styles[c.style as usize].wrap, "its text wrapped, as Excel does");
    let Editor::Xlsx(x) = &e else { unreachable!() };
    let written = save::xlsx_bytes(x).unwrap();
    let styles = part(&written.bytes, "xl/styles.xml");
    assert!(styles.contains(r#"<xf fontId="0" fillId="0" borderId="0" xfId="0" numFmtId="0" applyAlignment="1"><alignment wrapText="1"/></xf></cellXfs>"#), "{styles}");
    let sheet1 = part(&written.bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet1.contains("<c r=\"A9\" s=\"9\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Order by Friday\nPlot 7 first</t></is></c>"), "{sheet1}");
    let again = read_now(Format::Xlsx, &written.bytes).unwrap();
    assert!(again.styles[cell(&again.sheets[0], 9, 1).style as usize].wrap);
    // Its row as tall as its two lines: as edited, and read again from the file (a row Excel fits, no height of its own).
    let px = |b: &super::super::Book| b.sheets[0].row_looks.iter().find(|l| l.r == 9).and_then(|l| l.px);
    assert_eq!(px(&book), Some(40.0));
    assert_eq!(px(&again), Some(40.0));
    assert!(!part(&written.bytes, "xl/worksheets/sheet1.xml").contains(r#"<row r="9" ht"#), "no height written: Excel fits it");
    // Undo: the text and its wrap together.
    assert_eq!(e.undo(), Some((0, 9, 1)));
    assert!(!e.dirty());
    // Its row grown to its lines in the view: the lines typed, wrapped at the column's width.
    assert_eq!(wrapped_lines("Pay the rent first\nthen the energy bill", 64.0, 11.0), 5);
    assert_eq!(wrapped_lines("Short", 200.0, 11.0), 1);
    assert_eq!(wrapped_lines("abcdefghijklmnopqrstuvwxyz", 64.0, 11.0), 3, "a word longer than a line");
    // An `<xf>` with an alignment already: wrapText set in it.
    assert_eq!(
        save::xf_with_wrap(r#"<xf numFmtId="0" fontId="2" applyAlignment="1"><alignment horizontal="center"/></xf>"#, 0, false, true).unwrap(),
        r#"<xf fontId="2" numFmtId="0" applyAlignment="1"><alignment horizontal="center" wrapText="1"/></xf>"#
    );
}

#[test]
fn input_is_read_in_the_regions_locale() {
    // In the UK: a date day first, a price in pounds.
    let mut e = Editor::open_in("seed-order.xlsx", Format::Xlsx, &seed_order(), "en-GB").unwrap();
    e.edit(0, 9, 1, "16/11/2026").unwrap();
    e.edit(0, 9, 2, "£12.50").unwrap();
    let Editor::Xlsx(x) = &e else { unreachable!() };
    let m = x.model.get_model();
    assert_eq!(held_of(m, 0, 9, 1), Held::Number(46342f64.to_bits()), "a date: 16 Nov 2026");
    assert_eq!(held_of(m, 0, 9, 2), Held::Number(12.5f64.to_bits()));
    let book = e.book();
    assert_eq!(cell(&book.sheets[0], 9, 2).text, "£12.50");
    assert!(cell(&book.sheets[0], 9, 1).text.contains("16"), "{}", cell(&book.sheets[0], 9, 1).text);
    // Saved and read back in the same locale: what was typed.
    let written = save::xlsx_bytes(x).unwrap();
    assert!(part(&written.bytes, "xl/worksheets/sheet1.xml").contains("<v>46342</v>"));
    // IronCalc's own `en`: the same text stays text, never read the wrong way round.
    let mut us = Editor::open_in("seed-order.xlsx", Format::Xlsx, &seed_order(), "en").unwrap();
    us.edit(0, 9, 1, "16/11/2026").unwrap();
    let Editor::Xlsx(u) = &us else { unreachable!() };
    assert_eq!(held_of(u.model.get_model(), 0, 9, 1), Held::Text("16/11/2026".into()));
}

#[test]
fn a_regions_locale_is_ironcalcs_where_it_has_one() {
    use super::super::xlsx::{locale_for_region, locale_of_file};
    assert_eq!(locale_for_region(Some("en-GB"), Some("GB")), Some("en-GB"));
    assert_eq!(locale_for_region(Some("en-AU"), Some("au")), Some("en-GB"), "day first, as the UK");
    assert_eq!(locale_for_region(Some("pl-GB"), Some("GB")), Some("en-GB"), "the country's way of writing");
    assert_eq!(locale_for_region(Some("en-US"), Some("US")), Some("en"));
    assert_eq!(locale_for_region(Some("de-DE"), Some("DE")), Some("de"));
    assert_eq!(locale_for_region(Some("de-CH"), Some("CH")), None, "Swiss numbers aren't Germany's");
    assert_eq!(locale_for_region(Some("fr-CA"), Some("CA")), None);
    assert_eq!(locale_for_region(Some("fr-BE"), Some("BE")), Some("fr"));
    assert_eq!(locale_for_region(None, None), None);
    // Every one it gives, IronCalc has.
    let have = ironcalc::base::get_supported_locales();
    for id in ["en", "en-GB", "de", "fr", "es", "it"] {
        assert!(have.iter().any(|h| h == id), "IronCalc has {id}: {have:?}");
    }
    // A file's own: the language its formats name.
    let styles = STYLES.replace(r#"formatCode="dd/mm/yyyy""#, r#"formatCode="[$-809]dd/mm/yyyy""#);
    let gb = super::super::tests::xlsx_of(&[("Budget", "", "<sheetData/>".to_string())], &styles);
    assert_eq!(locale_of_file(&gb), Some("en-GB"));
    assert_eq!(locale_of_file(&seed_order()), None);
    assert_eq!(super::super::xlsx::locale_for(&gb), "en-GB", "no Region in a test: the file's");
    assert_eq!(super::super::xlsx::locale_for(&seed_order()), "en");
}

#[test]
fn an_empty_cell_with_a_type_reads_as_empty() {
    use super::super::xlsx::without_typed_empties;
    assert_eq!(
        without_typed_empties(r#"<sheetData><row r="2"><c r="B2" s="4" t="n"/><c r="C2" t="n"><v>3</v></c></row></sheetData>"#).as_deref(),
        Some(r#"<sheetData><row r="2"><c r="B2" s="4"/><c r="C2" t="n"><v>3</v></c></row></sheetData>"#)
    );
    assert_eq!(without_typed_empties(r#"<c r="B2" s="4"/>"#), None);
    // openpyxl's styled empty cell: nothing shown (IronCalc alone read 0), its style kept.
    let xml = format!(r#"<sheetData><row r="2">{}<c r="B2" s="4" t="n"/></row></sheetData>"#, n("A2", 5.0, 4));
    let bytes = super::super::tests::xlsx_of(&[("Budget", "", xml)], STYLES);
    let book = read_now(Format::Xlsx, &bytes).unwrap();
    let b2 = book.sheets[0].rows.iter().find(|r| r.r == 2).and_then(|r| r.cells.iter().find(|c| c.col == 2));
    assert!(b2.is_none_or(|c| c.text.is_empty() && c.kind == super::super::Kind::Empty), "{b2:?}");
    assert_eq!(cell(&book.sheets[0], 2, 1).text, "£5.00");
    // Edited and saved: the untouched cell as the file wrote it.
    let mut e = Editor::open("budget.xlsx", Format::Xlsx, &bytes).unwrap();
    e.edit(0, 2, 1, "6").unwrap();
    let Editor::Xlsx(x) = &e else { unreachable!() };
    assert!(part(&save::xlsx_bytes(x).unwrap().bytes, "xl/worksheets/sheet1.xml").contains(r#"<c r="B2" s="4" t="n"/>"#));
}
