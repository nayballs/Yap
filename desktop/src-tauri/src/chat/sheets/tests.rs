use super::*;
use crate::chat::attachments::tests::zip_of;

// ---- workbooks made here (no file of anyone's) --------------------------------------------------

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// An `.xlsx` of `sheets` (name, state, the worksheet's XML inside
/// `<worksheet>`) with `styles` (the inside of `<styleSheet>`), as openpyxl
/// writes one: inline strings, formulas with no results.
pub(crate) fn xlsx_of(sheets: &[(&str, &str, String)], styles: &str) -> Vec<u8> {
    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    let mut book = format!(r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets>"#);
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, (name, state, xml)) in sheets.iter().enumerate() {
        let n = i + 1;
        let state = if state.is_empty() { String::new() } else { format!(r#" state="{state}""#) };
        book.push_str(&format!(r#"<sheet name="{name}" sheetId="{n}"{state} r:id="rId{n}"/>"#));
        rels.push_str(&format!(r#"<Relationship Id="rId{n}" Type="{REL}/worksheet" Target="worksheets/sheet{n}.xml"/>"#));
        types.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        parts.push((format!("xl/worksheets/sheet{n}.xml"), format!(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="{MAIN}" xmlns:r="{REL}">{xml}</worksheet>"#)));
    }
    book.push_str("</sheets></workbook>");
    rels.push_str(&format!(r#"<Relationship Id="rId99" Type="{REL}/styles" Target="styles.xml"/></Relationships>"#));
    types.push_str("</Types>");
    let styles = format!(r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="{MAIN}">{styles}</styleSheet>"#);
    let root = format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#);
    let mut files: Vec<(String, String)> = vec![("[Content_Types].xml".into(), types), ("_rels/.rels".into(), root), ("xl/workbook.xml".into(), book), ("xl/_rels/workbook.xml.rels".into(), rels), ("xl/styles.xml".into(), styles)];
    files.extend(parts);
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    zip_of(&borrowed)
}

/// The styles of [`seed_order`]: 0 plain, 1 bold, 2 the title (bold 14,
/// a blue fill, centred), 3 a header (bold, a thin line under it), 4 pounds,
/// 5 a date, 6 a percentage, 7 italic red, 8 an accent colour of the theme.
pub(crate) const STYLES: &str = r#"<numFmts count="2"><numFmt numFmtId="164" formatCode="&quot;£&quot;#,##0.00"/><numFmt numFmtId="165" formatCode="dd/mm/yyyy"/></numFmts><fonts count="5"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/></font><font><b/><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/></font><font><b/><sz val="14"/><color theme="1"/><name val="Calibri"/><family val="2"/></font><font><i/><sz val="11"/><color rgb="FFFF0000"/><name val="Calibri"/><family val="2"/></font><font><sz val="11"/><color theme="4"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFDDEBF7"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border><left/><right/><top/><bottom style="thin"><color auto="1"/></bottom><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="9"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="2" fillId="2" borderId="0" xfId="0" applyFont="1" applyFill="1" applyAlignment="1"><alignment horizontal="center"/></xf><xf numFmtId="0" fontId="1" fillId="0" borderId="1" xfId="0" applyFont="1" applyBorder="1"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="165" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="9" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="0" fontId="3" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="4" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#;

pub(crate) fn s(r: &str, text: &str, style: u32) -> String {
    let st = if style > 0 { format!(r#" s="{style}""#) } else { String::new() };
    format!(r#"<c r="{r}"{st} t="inlineStr"><is><t>{text}</t></is></c>"#)
}
pub(crate) fn n(r: &str, v: f64, style: u32) -> String {
    let st = if style > 0 { format!(r#" s="{style}""#) } else { String::new() };
    format!(r#"<c r="{r}"{st}><v>{v}</v></c>"#)
}
pub(crate) fn f(r: &str, formula: &str, style: u32) -> String {
    let st = if style > 0 { format!(r#" s="{style}""#) } else { String::new() };
    format!(r#"<c r="{r}"{st}><f>{formula}</f></c>"#)
}

/// **A seed order** (fictional): a title merged over A1:D1, a header row,
/// three items with their totals as formulas a script wrote (no results), a
/// total, prices in pounds, a date, a share in per cent, the header rows
/// frozen, two widths; a second sheet of notes and a hidden third one.
pub(crate) fn seed_order() -> Vec<u8> {
    let budget = [
        r#"<sheetViews><sheetView workbookViewId="0"><pane ySplit="2" topLeftCell="A3" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>"#.to_string(),
        r#"<cols><col min="1" max="1" width="20" customWidth="1"/><col min="2" max="2" width="8" customWidth="1"/></cols>"#.to_string(),
        "<sheetData>".to_string(),
        format!(r#"<row r="1" ht="21" customHeight="1">{}<c r="B1" s="2"/><c r="C1" s="2"/><c r="D1" s="2"/></row>"#, s("A1", "Seed order", 2)),
        format!(r#"<row r="2">{}{}{}{}{}{}</row>"#, s("A2", "Item", 3), s("B2", "Qty", 3), s("C2", "Price", 3), s("D2", "Total", 3), s("E2", "Ordered", 3), s("F2", "Share", 3)),
        format!(r#"<row r="3">{}{}{}{}{}{}</row>"#, s("A3", "Carrots", 0), n("B3", 3.0, 0), n("C3", 2.5, 4), f("D3", "B3*C3", 4), n("E3", 46341.0, 5), f("F3", "D3/D$6", 6)),
        format!(r#"<row r="4">{}{}{}{}{}</row>"#, s("A4", "Runner beans", 0), n("B4", 2.0, 0), n("C4", 1.75, 4), f("D4", "B4*C4", 4), f("F4", "D4/D$6", 6)),
        format!(r#"<row r="5">{}{}{}{}{}</row>"#, s("A5", "Kale", 8), n("B5", 4.0, 0), n("C5", 3.0, 4), f("D5", "B5*C5", 4), f("F5", "D5/D$6", 6)),
        format!(r#"<row r="6">{}{}</row>"#, s("A6", "Total", 1), f("D6", "SUM(D3:D5)", 4)),
        "</sheetData>".to_string(),
        r#"<mergeCells count="1"><mergeCell ref="A1:D1"/></mergeCells>"#.to_string(),
    ]
    .concat();
    let notes = format!("<sheetData><row r=\"1\">{}</row><row r=\"2\">{}</row></sheetData>", s("A1", "Order by Friday", 7), s("A2", "Plot 7 &amp; plot 9", 0));
    let old = format!("<sheetData><row r=\"1\">{}</row></sheetData>", s("A1", "Last year", 0));
    xlsx_of(&[("Budget", "", budget), ("Notes", "", notes), ("Old", "hidden", old)], STYLES)
}

/// A big sheet: `rows` rows of five columns (an id, a bed, a crop, a
/// number, a formula), the header frozen.
pub(crate) fn big_xlsx(rows: u32) -> Vec<u8> {
    let crops = ["Carrots", "Kale", "Leeks", "Onions", "Peas"];
    let mut xml = String::with_capacity(rows as usize * 200);
    xml.push_str(r#"<sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetData>"#);
    xml.push_str(&format!(r#"<row r="1">{}{}{}{}{}</row>"#, s("A1", "Id", 1), s("B1", "Bed", 1), s("C1", "Crop", 1), s("D1", "Rows", 1), s("E1", "Plants", 1)));
    for r in 2..=rows {
        xml.push_str(&format!(r#"<row r="{r}">{}{}{}{}{}</row>"#, n(&format!("A{r}"), (r - 1) as f64, 0), s(&format!("B{r}"), &format!("Bed {}", r % 40), 0), s(&format!("C{r}"), crops[r as usize % 5], 0), n(&format!("D{r}"), (r % 7 + 1) as f64, 0), f(&format!("E{r}"), &format!("D{r}*12"), 0)));
    }
    xml.push_str("</sheetData>");
    xlsx_of(&[("Beds", "", xml)], STYLES)
}

/// An `.ods` (OpenDocument): one sheet of a header and two rows, a total as a formula with its result.
pub(crate) fn tiny_ods() -> Vec<u8> {
    let content = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2" office:version="1.2"><office:body><office:spreadsheet><table:table table:name="Harvest"><table:table-row><table:table-cell office:value-type="string"><text:p>Crop</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Kilos</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell office:value-type="string"><text:p>Potatoes</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="12.5"><text:p>12.5</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell office:value-type="string"><text:p>Total</text:p></table:table-cell><table:table-cell table:formula="of:=SUM([.B2:.B2])" office:value-type="float" office:value="12.5"><text:p>12.5</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#;
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    zip_of(&[("mimetype", "application/vnd.oasis.opendocument.spreadsheet"), ("META-INF/manifest.xml", manifest), ("content.xml", content)])
}

pub(crate) fn cell(sheet: &Sheet, r: u32, c: u32) -> &Cell {
    sheet.rows.iter().find(|row| row.r == r).and_then(|row| row.cells.iter().find(|x| x.col == c)).unwrap_or_else(|| panic!("no cell at row {r}, column {c}"))
}

// ---- the tests ----------------------------------------------------------------------------------

#[test]
fn columns_and_areas_are_named_as_excel_names_them() {
    assert_eq!(column_name(1), "A");
    assert_eq!(column_name(26), "Z");
    assert_eq!(column_name(27), "AA");
    assert_eq!(column_name(16_384), "XFD");
    assert_eq!(Area { top: 1, left: 1, bottom: 120, right: 6 }.a1(), "A1:F120");
    assert_eq!(Area { top: 2, left: 2, bottom: 2, right: 2 }.a1(), "B2");
    assert_eq!(Area::default().a1(), "");
    let mut a = Area::default();
    a.take(5, 3);
    a.take(2, 7);
    assert_eq!((a.a1(), a.rows(), a.cols()), ("C2:G5".to_string(), 4, 5));
}

#[test]
fn spreadsheets_are_known_by_their_endings() {
    assert_eq!(Format::of("Seeds.XLSX"), Some(Format::Xlsx));
    assert_eq!(Format::of("macros.xlsm"), Some(Format::Xlsx));
    assert_eq!(Format::of("old.xls"), Some(Format::Xls));
    assert_eq!(Format::of("binary.xlsb"), Some(Format::Xls));
    assert_eq!(Format::of("harvest.ods"), Some(Format::Ods));
    assert_eq!(Format::of("beds.csv"), Some(Format::Csv));
    assert_eq!(Format::of("beds.tsv"), Some(Format::Tsv));
    assert_eq!(Format::of("notes.txt"), None);
    assert_eq!(Format::of("xlsx"), None);
}

#[test]
fn a_scripts_workbook_is_calculated_and_formatted() {
    let book = read_now(Format::Xlsx, &seed_order()).unwrap();
    assert!(book.calculated);
    assert_eq!(book.sheets.len(), 3);
    let b = &book.sheets[0];
    assert_eq!(b.name, "Budget");
    // The totals the file had no results for, calculated, in its own formats.
    assert_eq!(cell(b, 3, 4).text, "£7.50");
    assert_eq!(cell(b, 3, 4).formula.as_deref(), Some("=B3*C3"));
    assert_eq!(cell(b, 4, 4).text, "£3.50");
    assert_eq!(cell(b, 6, 4).text, "£23.00");
    assert_eq!(cell(b, 6, 4).formula.as_deref(), Some("=SUM(D3:D5)"));
    assert_eq!(cell(b, 3, 6).text, "33%");
    assert_eq!(cell(b, 3, 6).kind, Kind::Number);
    // A date, in the file's own format; a price.
    assert_eq!(cell(b, 3, 5).text, "15/11/2026");
    assert_eq!(cell(b, 3, 3).text, "£2.50");
    assert_eq!(cell(b, 3, 1).kind, Kind::Text);
    assert_eq!(b.used.a1(), "A1:F6");
    assert_eq!(b.formulas, 7);
    assert_eq!((b.frozen_rows, b.frozen_cols), (2, 0));
    assert_eq!(b.merges, vec![Merge { row: 1, col: 1, rows: 1, cols: 4 }]);
    // Widths as Excel draws them (20 characters, 8), a row 21 pt high.
    assert_eq!(b.col_px(1), 140.0);
    assert_eq!(b.col_px(2), 56.0);
    assert_eq!(b.col_px(3), COLUMN_PX);
    assert_eq!(b.row_looks.iter().find(|r| r.r == 1).and_then(|r| r.px), Some(28.0));
    assert!(b.grid);
    // The second sheet; the third hidden.
    assert_eq!(cell(&book.sheets[1], 2, 1).text, "Plot 7 & plot 9");
    assert!(!book.sheets[1].hidden);
    assert!(book.sheets[2].hidden);
}

/// Lab, 15 Nov 2026: a workbook whose `styles.xml` leaves out parts the
/// format lets it (here `<cellStyles>`, then all but `<cellXfs>`) made
/// IronCalc panic, and the file was left out of Chat. It's calculated and
/// keeps its looks now.
#[test]
fn a_workbook_missing_optional_style_parts_is_still_calculated() {
    let without = STYLES.split("<cellStyles").next().unwrap();
    let rows = format!("<sheetData><row r=\"1\">{}{}</row><row r=\"2\">{}{}</row><row r=\"3\">{}</row></sheetData>", s("A1", "Item", 1), s("B1", "Price", 1), s("A2", "Seeds", 0), n("B2", 2.5, 4), f("B3", "B2*4", 4));
    let book = read_now(Format::Xlsx, &xlsx_of(&[("Costs", "", rows)], without)).unwrap();
    assert!(book.calculated, "IronCalc read it (calamine can't calculate)");
    let c = &book.sheets[0];
    assert_eq!(cell(c, 3, 2).text, "£10.00");
    assert!(book.styles[cell(c, 1, 1).style as usize].bold, "the header's bold kept");

    // Only cellXfs: fonts, fills, borders, cellStyleXfs and cellStyles all filled in.
    let only_xfs = r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#;
    let rows = format!("<sheetData><row r=\"1\">{}{}{}</row></sheetData>", n("A1", 2.0, 0), n("B1", 3.0, 0), f("C1", "A1*B1", 0));
    let book = read_now(Format::Xlsx, &xlsx_of(&[("S", "", rows)], only_xfs)).unwrap();
    assert!(book.calculated);
    assert_eq!(cell(&book.sheets[0], 1, 3).text, "6");
}

/// Lab, 16 Nov 2026: openpyxl's `£#,##0.00` (the pound sign bare, as Excel
/// takes it) showed every price as `#VALUE!`. Read as Excel shows it now.
#[test]
fn a_bare_pound_sign_in_a_format_reads_as_excel_shows_it() {
    assert_eq!(xlsx::quoted_literals("£#,##0.00").as_deref(), Some("\"£\"#,##0.00"));
    assert_eq!(xlsx::quoted_literals("#,##0.00 ₹;[Red]-£#,##0").as_deref(), Some("#,##0.00 \"₹\";[Red]-\"£\"#,##0"));
    assert_eq!(xlsx::quoted_literals("\"£\"#,##0.00"), None, "quoted already");
    assert_eq!(xlsx::quoted_literals("[$£-809]#,##0.00"), None, "a locale's code in brackets");
    assert_eq!(xlsx::quoted_literals("\\£0"), None, "escaped");
    assert_eq!(xlsx::quoted_literals("€#,##0;$0"), None, "the two IronCalc takes bare");
    let styles = STYLES.replace(r#"<numFmt numFmtId="164" formatCode="&quot;£&quot;#,##0.00"/>"#, r#"<numFmt numFmtId="164" formatCode="£#,##0.00"/>"#);
    let rows = format!("<sheetData><row r=\"1\">{}{}{}</row></sheetData>", n("A1", 4.0, 0), n("B1", 6.5, 4), f("C1", "A1*B1", 4));
    let book = read_now(Format::Xlsx, &xlsx_of(&[("Budget", "", rows)], &styles)).unwrap();
    assert_eq!(cell(&book.sheets[0], 1, 2).text, "£6.50");
    assert_eq!(cell(&book.sheets[0], 1, 3).text, "£26.00");
}

#[test]
fn missing_style_parts_are_added_in_the_roots_own_prefix() {
    let plain = r#"<?xml version="1.0"?><styleSheet xmlns="m"><fonts count="1"><font/></fonts><cellXfs count="1"><xf/></cellXfs></styleSheet>"#;
    let full = xlsx::full_styles(plain).unwrap();
    for part in ["<fills", "<borders", "<cellStyleXfs", "<cellStyles "] {
        assert!(full.contains(part), "{part} added");
    }
    assert_eq!(full.matches("<fonts").count(), 1, "one it had isn't added again");
    assert!(full.ends_with("</styleSheet>"));
    // `<cellStyleXfs>` isn't `<cellStyles>`: a prefix of a tag's name never counts as the tag.
    assert!(xlsx::full_styles(r#"<styleSheet><cellStyleXfs/></styleSheet>"#).unwrap().contains("<cellStyles count"));

    let prefixed = r#"<x:styleSheet xmlns:x="m"><x:fonts count="1"><x:font/></x:fonts></x:styleSheet>"#;
    let full = xlsx::full_styles(prefixed).unwrap();
    assert!(full.contains(r#"<x:fills count="2"><x:fill><x:patternFill patternType="none"/></x:fill>"#), "{full}");
    assert!(full.contains("</x:cellStyles></x:styleSheet>"), "{full}");

    let complete = format!("<styleSheet>{STYLES}</styleSheet>");
    assert_eq!(xlsx::full_styles(&complete), None, "nothing to add: nothing to read again");
    assert_eq!(xlsx::full_styles("<styleSheet>"), None, "no end tag to add them before");
}

#[test]
fn a_workbooks_looks_are_kept() {
    let book = read_now(Format::Xlsx, &seed_order()).unwrap();
    let b = &book.sheets[0];
    let style = |r: u32, c: u32| &book.styles[cell(b, r, c).style as usize];
    // The title: bold, 14 pt, its fill, centred; its merged neighbours styled too.
    let title = style(1, 1);
    assert!(title.bold);
    assert_eq!(title.size, Some(140));
    assert_eq!(title.fill.as_deref(), Some("#DDEBF7"));
    assert_eq!(title.h, Some("center"));
    assert_eq!(cell(b, 1, 2).kind, Kind::Empty);
    // A header: bold with a thin line under it, its colour Excel's automatic.
    let head = style(2, 1);
    assert!(head.bold && head.bottom.as_ref().is_some_and(|e| e.style == "thin"));
    assert_eq!(head.color, None);
    // "Text 1" of the theme is automatic too (light text in dark mode); a red is red.
    assert_eq!(style(3, 1), &Style::default());
    assert_eq!(book.styles[cell(&book.sheets[1], 1, 1).style as usize].color.as_deref(), Some("#FF0000"));
    assert!(book.styles[cell(&book.sheets[1], 1, 1).style as usize].italic);
    // An accent of the theme, resolved (Office's: blue).
    assert_eq!(style(5, 1).color.as_deref(), Some("#4472C4"));
    assert_eq!(book.font, "Calibri");
    assert_eq!(book.size, 11.0);
}

#[test]
fn a_big_sheet_is_read_and_asked_for_a_stretch_at_a_time() {
    let book = read_now(Format::Xlsx, &big_xlsx(30_000)).unwrap();
    let beds = &book.sheets[0];
    assert_eq!(beds.used.a1(), "A1:E30000");
    assert_eq!(beds.values(), 150_000);
    assert_eq!(cell(beds, 29_999, 5).text, ((29_999 % 7 + 1) * 12).to_string());
    let rows = view::rows_of(beds, 10_000, 10_049);
    assert_eq!(rows.len(), 50);
    assert_eq!((rows[0].r, rows[49].r), (10_000, 10_049));
    // At most a thousand rows an ask.
    assert_eq!(view::rows_of(beds, 1, 30_000).len(), view::ROWS_AT_ONCE as usize);
}

#[test]
fn a_merge_reaching_into_the_rows_asked_for_brings_its_cell() {
    let mut sheet = Sheet { name: "S".into(), ..Default::default() };
    sheet.rows = vec![Row { r: 2, cells: vec![Cell { col: 1, text: "Beds".into(), kind: Kind::Text, formula: None, input: None, style: 0 }] }, Row { r: 40, cells: vec![Cell { col: 2, text: "x".into(), kind: Kind::Text, formula: None, input: None, style: 0 }] }];
    sheet.merges = vec![Merge { row: 2, col: 1, rows: 30, cols: 2 }];
    let rows = view::rows_of(&sheet, 20, 50);
    assert_eq!(rows.iter().map(|r| r.r).collect::<Vec<_>>(), vec![2, 40]);
    assert_eq!(view::rows_of(&sheet, 40, 50).iter().map(|r| r.r).collect::<Vec<_>>(), vec![40]);
}

#[test]
fn a_workbook_too_big_keeps_its_first_rows_and_counts_the_rest() {
    let rows: Vec<Row> = (1..=10).map(|r| Row { r, cells: (1..=4).map(|c| Cell { col: c, text: "x".into(), kind: Kind::Text, formula: None, input: None, style: 0 }).collect() }).collect();
    let (kept, cut, cells) = keep_rows(rows.clone(), 20);
    assert_eq!((kept.len(), cut, cells), (5, 5, 20));
    let (kept, cut, _) = keep_rows(rows.clone(), 0);
    assert_eq!((kept.len(), cut), (0, 10));
    // A first row bigger than the room is kept whole.
    let (kept, cut, _) = keep_rows(rows, 2);
    assert_eq!((kept.len(), cut), (1, 9));
}

#[test]
fn csv_and_tsv_are_one_sheet_of_values_as_written() {
    let csv = "Crop,Kilos,Note\r\nPotatoes,12.5,\"Dug up, washed\"\r\nLeeks,3,\r\n";
    let book = read_now(Format::Csv, csv.as_bytes()).unwrap();
    let s = &book.sheets[0];
    assert_eq!(s.used.a1(), "A1:C3");
    assert_eq!(cell(s, 2, 3).text, "Dug up, washed");
    assert_eq!(cell(s, 2, 2).kind, Kind::Number);
    assert_eq!(cell(s, 2, 2).text, "12.5");
    assert_eq!(cell(s, 1, 2).kind, Kind::Text);
    assert!(!book.calculated);
    // A CSV saved where the decimal mark is a comma: split on ";".
    let semi = read_now(Format::Csv, "Crop;Kilos\nKale;2,5\n".as_bytes()).unwrap();
    assert_eq!(cell(&semi.sheets[0], 2, 2).text, "2,5");
    let tsv = read_now(Format::Tsv, "Crop\tKilos\nKale\t4\n".as_bytes()).unwrap();
    assert_eq!(cell(&tsv.sheets[0], 2, 2).text, "4");
    // Columns fitted to what's in them.
    assert!(s.col_px(3) > COLUMN_PX, "{}", s.col_px(3));
    assert_eq!(delimited::kind_of("1e3"), Kind::Number);
    assert_eq!(delimited::kind_of("NaN"), Kind::Text);
    assert_eq!(delimited::kind_of("TRUE"), Kind::Bool);
}

#[test]
fn an_ods_is_read_with_its_formulas() {
    let book = read_now(Format::Ods, &tiny_ods()).unwrap();
    let s = &book.sheets[0];
    assert_eq!(s.name, "Harvest");
    assert_eq!(cell(s, 2, 2).text, "12.5");
    assert_eq!(cell(s, 3, 2).text, "12.5");
    assert!(cell(s, 3, 2).formula.as_deref().is_some_and(|f| f.contains("SUM")), "{:?}", cell(s, 3, 2).formula);
    assert!(!book.calculated);
}

#[test]
fn numbers_and_dates_read_by_calamine_look_as_excel_shows_them() {
    use calamine::Data;
    assert_eq!(xls::shown(&Data::Float(0.1 + 0.2)).0, "0.3");
    assert_eq!(xls::shown(&Data::Float(1234.5)).0, "1234.5");
    assert_eq!(xls::shown(&Data::Int(7)), ("7".to_string(), Kind::Number));
    assert_eq!(xls::shown(&Data::Bool(true)).0, "TRUE");
    assert_eq!(xls::shown(&Data::String(String::new())).1, Kind::Empty);
    assert_eq!(xls::format_with(46341.0, "yyyy-mm-dd"), "2026-11-15");
}

#[test]
fn files_that_arent_workbooks_or_are_locked_are_said() {
    assert_eq!(read_now(Format::Xlsx, b"PK\x03\x04 not a zip at all").unwrap_err(), Why::Failed);
    assert_eq!(read_now(Format::Ods, b"nothing").unwrap_err(), Why::Failed);
    // A password-protected .xlsx: an OLE file holding an encrypted package.
    let mut locked = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    locked.extend(vec![0u8; 512]);
    locked.extend("EncryptedPackage".encode_utf16().flat_map(|u| u.to_le_bytes()));
    assert!(is_encrypted_office(&locked));
    assert_eq!(read_now(Format::Xlsx, &locked).unwrap_err(), Why::Locked);
    assert_eq!(read("seeds.txt", b"a,b".to_vec()).unwrap_err(), Why::Failed);
}

#[test]
fn reading_one_in_its_own_thread_gives_the_same() {
    let book = read("seed-order.xlsx", seed_order()).unwrap();
    assert_eq!(cell(&book.sheets[0], 6, 4).text, "£23.00");
}

// ---- what the model gets (attachments/sheet.rs) -------------------------------------------------

use crate::chat::attachments::sheet as text;

#[test]
fn the_model_gets_each_sheet_as_a_table_with_its_formulas() {
    let book = read_now(Format::Xlsx, &seed_order()).unwrap();
    let t = text::of_book(&book, text::TOKENS);
    assert!(t.starts_with("An Excel workbook with 3 sheets: \"Budget\", \"Notes\", \"Old\" (hidden)."), "{t}");
    assert!(t.contains("## Sheet 1 of 3: \"Budget\"\nUsed range A1:F6 (6 rows × 6 columns); frozen: rows 1–2; merged: A1:D1."), "{t}");
    assert!(t.contains("| | A | B | C | D | E | F |"), "{t}");
    assert!(t.contains("| 3 | Carrots | 3 | £2.50 | £7.50 | 15/11/2026 | 33% |"), "{t}");
    assert!(t.contains("| 6 | Total |  |  | £23.00 |  |  |"), "{t}");
    assert!(t.contains("D3:D5: =B3*C3 (the same in each row, filled down)"), "{t}");
    assert!(t.contains("F3:F5: =D3/D$6 (the same in each row, filled down)"), "{t}");
    assert!(t.contains("D6: =SUM(D3:D5)"), "{t}");
    assert!(t.contains("Formulas (results above calculated on this PC):"), "{t}");
    assert!(t.contains("## Sheet 3 of 3: \"Old\" (hidden)"), "{t}");
    assert!(t.contains("| 2 | Plot 7 & plot 9 |"), "{t}");
}

#[test]
fn a_big_sheet_is_cut_to_the_room_with_a_note() {
    let book = read_now(Format::Xlsx, &big_xlsx(20_000)).unwrap();
    let t = text::of_book(&book, 4_000);
    let tokens = crate::chat::budget::estimate(&t);
    assert!(tokens <= 4_400, "{tokens} tokens");
    assert!(t.contains("| 1 | Id | Bed | Crop | Rows | Plants |"), "{}", &t[..400]);
    assert!(t.contains("rows left out to fit the model; the sheet's last rows follow"), "{t}");
    assert!(t.contains("| 20000 | 19999 |"), "the last row is kept");
    assert!(t.contains("E2:E20000: =D2*12 (the same in each row, filled down)"), "{t}");
}

#[test]
fn the_room_is_shared_small_sheets_first() {
    assert_eq!(text::shares(&[100, 5_000, 300], 1_000), vec![100, 600, 300]);
    assert_eq!(text::shares(&[5_000, 4_000], 1_000), vec![500, 500]);
    assert_eq!(text::shares(&[100, 200], 1_000), vec![100, 200]);
    assert_eq!(text::shares(&[], 1_000), Vec::<usize>::new());
}

#[test]
fn formulas_filled_down_or_across_have_one_shape() {
    assert_eq!(text::shape("=B2*C2", 2, 4), text::shape("=B3*C3", 3, 4));
    assert_ne!(text::shape("=B2*C2", 2, 4), text::shape("=B2*C3", 3, 4));
    // `$` parts stay; text and function names aren't references.
    assert_eq!(text::shape("=D3/D$6", 3, 6), text::shape("=D4/D$6", 4, 6));
    assert_eq!(text::shape("=LOG10(A1)&\"B2\"", 1, 2), "=LOG10(R[0]C[-1])&\"B2\"");
    assert_eq!(text::shape("='Sheet 2'!A1", 1, 1), "='Sheet 2'!R[0]C[0]");
    assert_eq!(text::tidy("a|b\nc"), "a\\|b / c");
    assert_eq!(text::thousands(1_234_567), "1,234,567");
}

#[test]
fn an_attached_spreadsheet_is_read_as_a_sheet_within_the_room() {
    let r = crate::chat::attachments::read("seed-order.xlsx", seed_order()).unwrap();
    assert_eq!(r.kind, "sheet");
    assert!(r.text.contains("| 6 | Total |"), "{}", r.text);
    let csv = crate::chat::attachments::read("beds.csv", b"Bed,Crop\n1,Kale\n".to_vec()).unwrap();
    assert_eq!(csv.kind, "sheet");
    assert!(csv.text.starts_with("One table; its values as written in the file.\n\n2 rows × 2 columns (A1:B2)."), "{}", csv.text);
    // The box's room: a big one cut to it (never under the least).
    let big = text::with_budget(Some(2_000), || crate::chat::attachments::read("beds.xlsx", big_xlsx(5_000)).unwrap());
    assert!(crate::chat::budget::estimate(&big.text) <= 2_300, "{}", crate::chat::budget::estimate(&big.text));
    assert_eq!(text::with_budget(Some(10), text::budget), text::LEAST);
    assert_eq!(text::budget(), text::TOKENS);
    assert_eq!(crate::chat::attachments::read("broken.xlsx", b"PK\x03\x04".to_vec()).unwrap_err(), "Yap couldn't read the cells of broken.xlsx.");
    assert_eq!(crate::chat::attachments::read("empty.csv", b"\n\n".to_vec()).unwrap_err(), "empty.csv has nothing in its cells.");
}
