//! References moved as Excel moves them when rows or columns go in or out:
//! the maths, formulas' text, areas — and a whole package (`parts.rs`).

use super::*;

fn rows(at: u32, n: u32, insert: bool) -> Shift {
    Shift { axis: Axis::Rows, at, n, insert }
}
fn cols(at: u32, n: u32, insert: bool) -> Shift {
    Shift { axis: Axis::Cols, at, n, insert }
}
/// Formula `f` on sheet "Budget", rows or columns changed on "Budget".
fn on(f: &str, s: Shift) -> String {
    formula(f, Some("Budget"), "Budget", &s).unwrap().text
}

#[test]
fn lines_and_spans_move_grow_and_shrink() {
    let ins = rows(4, 2, true);
    assert_eq!(ins.line(3), Some(3));
    assert_eq!(ins.line(4), Some(6));
    assert_eq!(ins.span(2, 5), Some((2, 7)), "put in inside: grown");
    assert_eq!(ins.span(2, 3), Some((2, 3)), "ending just above: as it is (Excel's)");
    assert_eq!(ins.span(4, 9), Some((6, 11)), "starting at it: moved");
    assert_eq!(ins.line(LAST_ROW), None, "pushed off the end");
    assert_eq!(rows(2, 1, true).span(1, LAST_ROW), Some((1, LAST_ROW)), "a whole column stays whole");
    let del = rows(4, 2, false);
    assert_eq!(del.line(3), Some(3));
    assert_eq!(del.line(4), None);
    assert_eq!(del.line(5), None);
    assert_eq!(del.line(6), Some(4));
    assert_eq!(del.span(2, 9), Some((2, 7)), "lines inside taken out: shrunk");
    assert_eq!(del.span(4, 9), Some((4, 7)), "its first lines taken out");
    assert_eq!(del.span(2, 5), Some((2, 3)), "its last lines taken out");
    assert_eq!(del.span(4, 5), None, "all of it");
    assert!(ins.splits(2, 5) && !ins.splits(4, 5) && !ins.splits(2, 3));
    assert!(del.splits(3, 4) && del.splits(5, 6) && !del.splits(4, 5) && !del.splits(1, 3));
}

#[test]
fn a_formula_s_references_move_as_excel_moves_them() {
    let ins = rows(4, 1, true);
    // A total over the rows a row went in among: it takes the new row in.
    assert_eq!(on("SUM(D2:D5)", ins), "SUM(D2:D6)");
    assert_eq!(on("SUM(D2:D3)", ins), "SUM(D2:D3)", "ending just above: Excel doesn't grow it");
    assert_eq!(on("B4*C4", ins), "B5*C5");
    assert_eq!(on("B3*$C$9+C$4", ins), "B3*$C$10+C$5", "a `$` is about copying, not inserting");
    assert_eq!(on("SUM(A:A)+SUM(4:6)", ins), "SUM(A:A)+SUM(5:7)", "a whole column stays, a whole row moves");
    assert_eq!(on("Budget!B4+'Budget'!B3", ins), "Budget!B5+'Budget'!B3");
    assert_eq!(on("budget!B4", ins), "budget!B5", "a sheet's name in any case");
    // Not moved: another sheet's, names, tables, text, functions, numbers, errors.
    assert_eq!(on("Notes!B4+Rate+Table1[Qty]+\"B4\"+LOG10(B2)+1.5E+4+#REF!", ins), "Notes!B4+Rate+Table1[Qty]+\"B4\"+LOG10(B2)+1.5E+4+#REF!");
    assert_eq!(on("[1]Budget!B4+'[Other.xlsx]Budget'!B4", ins), "[1]Budget!B4+'[Other.xlsx]Budget'!B4", "another workbook's");
    assert_eq!(on("Table1[[#This Row],[Qty]]*B9", ins), "Table1[[#This Row],[Qty]]*B10");
    // A formula on another sheet: only the references that name the sheet changed.
    let m = formula("Budget!D6*B4+SUM(Budget!D2:D5)", Some("Notes"), "Budget", &ins).unwrap();
    assert_eq!(m.text, "Budget!D7*B4+SUM(Budget!D2:D6)");
    assert!(m.touches);
    let m = formula("B4*2", Some("Notes"), "Budget", &ins).unwrap();
    assert_eq!((m.text.as_str(), m.touches), ("B4*2", false));
    // Columns.
    let c = cols(2, 1, true);
    assert_eq!(on("SUM(A1:C1)*$B$2+D:D", c), "SUM(A1:D1)*$C$2+E:E");
    assert_eq!(on("SUM(3:3)", c), "SUM(3:3)");
    // Nothing moved: the text exactly as written.
    assert_eq!(on("sum( B2 , b3 )", rows(9, 1, true)), "sum( B2 , b3 )");
}

#[test]
fn a_reference_taken_out_is_ref_and_an_area_shrinks() {
    let del = rows(4, 1, false);
    assert_eq!(on("SUM(D2:D6)", del), "SUM(D2:D5)");
    assert_eq!(on("SUM(D4:D6)", del), "SUM(D4:D5)", "its first row gone");
    assert_eq!(on("SUM(D2:D4)", del), "SUM(D2:D3)", "its last row gone (IronCalc's own would say #REF!)");
    assert_eq!(on("B4*2", del), "#REF!*2");
    assert_eq!(on("SUM(D4:D4)+B5", del), "SUM(#REF!)+B4");
    assert_eq!(on("Budget!B4", del), "Budget!#REF!");
    assert_eq!(on("SUM(B:B)", del), "SUM(B:B)");
    assert_eq!(on("SUM(4:4)", del), "SUM(#REF!)");
    assert_eq!(on("SUM(B2:D9)", cols(3, 1, false)), "SUM(B2:C9)");
}

#[test]
fn unsure_references_are_refused() {
    let ins = rows(4, 1, true);
    assert!(formula("SUM(Jan:Mar!B4)", Some("Budget"), "Budget", &ins).is_err(), "across sheets");
    assert!(formula("SUM('Jan:Mar'!B4)", Some("Budget"), "Budget", &ins).is_err());
    assert!(formula("$B$4", None, "Budget", &ins).is_err(), "a defined name with no sheet in it");
    assert!(formula("\"unclosed", Some("Budget"), "Budget", &ins).is_err());
    // A name or a constant with no reference at all: fine with no sheet.
    assert_eq!(formula("0.2", None, "Budget", &ins).unwrap().text, "0.2");
    assert_eq!(formula("Budget!$B$2:$B$5", None, "Budget", &ins).unwrap().text, "Budget!$B$2:$B$6");
}

#[test]
fn areas_and_lists_of_areas_move() {
    let ins = rows(4, 1, true);
    assert_eq!(area("A1:D5", &ins).unwrap().as_deref(), Some("A1:D6"));
    assert_eq!(area("B7", &ins).unwrap().as_deref(), Some("B8"));
    assert_eq!(sqref("B2:B5 D9", &ins).unwrap(), "B2:B6 D10");
    assert_eq!(sqref("B2:B3 D1", &ins).unwrap(), "B2:B3 D1", "nothing moved: as written");
    assert_eq!(sqref("B4 D2:D9", &rows(4, 1, false)).unwrap(), "D2:D8", "one taken out, one shrunk");
    assert_eq!(sqref("B4", &rows(4, 1, false)).unwrap(), "");
    assert_eq!(cell_name("C4", &rows(4, 1, false)), Some(None));
    assert_eq!(cell_name("C5", &cols(2, 2, true)), Some(Some("E5".to_string())));
}

// ---- a whole package ----------------------------------------------------------------------------

use super::super::tests::{f, n, s, xlsx_of, STYLES};
use super::parts::{package, parts_of, Change};

/// Adds parts to a package (a chart, a drawing, comments, a table…).
fn with_parts(base: &[u8], extra: &[(&str, &str)]) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(base)).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> = (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (f.name().map(|n| n.into_owned()).unwrap(), b)
        })
        .collect();
    for (name, text) in extra {
        match entries.iter_mut().find(|(n, _)| n == name) {
            Some(e) => e.1 = text.as_bytes().to_vec(),
            None => entries.push((name.to_string(), text.as_bytes().to_vec())),
        }
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, bytes) in &entries {
            z.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A budget (fictional): items in rows 2–5 with their totals, a grand total
/// in row 6 over D2:D5, and everything that points at cells: a merge, a
/// conditional format, a validation list, a hyperlink, a defined name and a
/// print area, a shared formula, frozen panes, a page break, an x14 rule,
/// a chart's series, a picture anchored below, comments with their notes'
/// boxes, a table further down, and a second sheet that sums the first.
fn budget() -> Vec<u8> {
    let data = [
        r#"<dimension ref="A1:F12"/><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/><selection pane="bottomLeft" activeCell="B5" sqref="B5"/></sheetView></sheetViews>"#.to_string(),
        "<sheetData>".to_string(),
        format!(r#"<row r="1">{}{}{}{}</row>"#, s("A1", "Item", 1), s("B1", "Qty", 1), s("C1", "Price", 1), s("D1", "Total", 1)),
        format!(r#"<row r="2">{}{}{}<c r="D2" s="4"><f t="shared" ref="D2:D5" si="0">B2*C2</f></c></row>"#, s("A2", "Compost", 0), n("B2", 6.0, 0), n("C2", 4.5, 4)),
        format!(r#"<row r="3" s="5" customFormat="1">{}{}{}<c r="D3" s="4"><f t="shared" si="0"/></c></row>"#, s("A3", "Seed trays", 0), n("B3", 2.0, 0), n("C3", 3.25, 4)),
        format!(r#"<row r="4">{}{}{}<c r="D4" s="4"><f t="shared" si="0"/></c></row>"#, s("A4", "Twine", 0), n("B4", 1.0, 0), n("C4", 2.0, 4)),
        format!(r#"<row r="5">{}{}{}<c r="D5" s="4"><f t="shared" si="0"/></c></row>"#, s("A5", "Netting", 0), n("B5", 3.0, 0), n("C5", 9.0, 4)),
        format!(r#"<row r="6">{}{}<c r="E6" s="4" t="n"/></row>"#, s("A6", "Total", 1), f("D6", "SUM(D2:D5)", 4)),
        format!(r#"<row r="8">{}{}</row>"#, s("A8", "Share", 0), f("B8", "D4/$D$6", 6)),
        format!(r#"<row r="10">{}{}</row><row r="11">{}{}</row><row r="12">{}{}</row>"#, s("A10", "Bed", 1), s("B10", "Crop", 1), s("A11", "One", 0), s("B11", "Kale", 0), s("A12", "Two", 0), s("B12", "Leeks", 0)),
        "</sheetData>".to_string(),
        r#"<mergeCells count="2"><mergeCell ref="A8:A9"/><mergeCell ref="E2:F4"/></mergeCells>"#.to_string(),
        r#"<conditionalFormatting sqref="D2:D5"><cfRule type="cellIs" dxfId="0" priority="1" operator="greaterThan"><formula>$D$6/2</formula></cfRule></conditionalFormatting>"#.to_string(),
        r#"<dataValidations count="1"><dataValidation type="whole" allowBlank="1" sqref="B2:B5"><formula1>0</formula1><formula2>$B$8*100</formula2></dataValidation></dataValidations>"#.to_string(),
        r#"<hyperlinks><hyperlink ref="A6" location="Budget!D6" display="Total"/></hyperlinks>"#.to_string(),
        r#"<rowBreaks count="1" manualBreakCount="1"><brk id="6" max="16383" man="1"/></rowBreaks>"#.to_string(),
        r#"<drawing r:id="rId1"/><legacyDrawing r:id="rId2"/><tableParts count="1"><tablePart r:id="rId4"/></tableParts>"#.to_string(),
        r#"<extLst><ext uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:conditionalFormattings><x14:conditionalFormatting xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:cfRule type="dataBar" id="{00000000-0000-0000-0000-000000000001}"><x14:dataBar><x14:cfvo type="autoMin"/><x14:cfvo type="autoMax"/></x14:dataBar></x14:cfRule><xm:sqref>C2:C5</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></ext></extLst>"#.to_string(),
    ]
    .concat();
    let notes = format!(r#"<sheetData><row r="1">{}{}</row></sheetData>"#, s("A1", "Spent", 0), f("B1", "Budget!D6+SUM(Budget!B2:B5)", 0));
    let base = xlsx_of(&[("Budget", "", data), ("Notes", "", notes)], STYLES);
    let wb = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId2"/></sheets><definedNames><definedName name="Items">Budget!$A$2:$A$5</definedName><definedName name="_xlnm.Print_Area" localSheetId="0">Budget!$A$1:$D$6</definedName><definedName name="Rate">0.2</definedName></definedNames></workbook>"#
    );
    let sheet_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rId2" Type="{REL}/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/><Relationship Id="rId3" Type="{REL}/comments" Target="../comments1.xml"/><Relationship Id="rId4" Type="{REL}/table" Target="../tables/table1.xml"/></Relationships>"#
    );
    let drawing = r#"<?xml version="1.0" encoding="UTF-8"?><xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><xdr:twoCellAnchor><xdr:from><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>9</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>14</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Chart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm/><a:graphic/></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor><xdr:twoCellAnchor editAs="oneCell"><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>14</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>18</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:pic/><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
    let chart = r#"<?xml version="1.0" encoding="UTF-8"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:f>Budget!$D$1</c:f></c:strRef></c:tx><c:cat><c:strRef><c:f>Budget!$A$2:$A$5</c:f></c:strRef></c:cat><c:val><c:numRef><c:f>Budget!$D$2:$D$5</c:f><c:numCache><c:ptCount val="4"/></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
    let comments = r#"<?xml version="1.0" encoding="UTF-8"?><comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Sam Example</author></authors><commentList><comment ref="C5" authorId="0"><text><r><t>Ask about netting</t></r></text></comment></commentList></comments>"#;
    let vml = "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\"><v:shape id=\"_x0000_s1025\" type=\"#_x0000_t202\"><x:ClientData ObjectType=\"Note\"><x:MoveWithCells/><x:Anchor>\n 3, 15, 3, 10, 5, 15, 7, 4</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>4</x:Row><x:Column>2</x:Column></x:ClientData></v:shape></xml>";
    let table = r#"<?xml version="1.0" encoding="UTF-8"?><table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="Beds" displayName="Beds" ref="A10:B12" totalsRowShown="0"><autoFilter ref="A10:B12"/><tableColumns count="2"><tableColumn id="1" name="Bed"/><tableColumn id="2" name="Crop"/></tableColumns></table>"#;
    with_parts(
        &base,
        &[
            ("xl/workbook.xml", &wb),
            ("xl/worksheets/_rels/sheet1.xml.rels", &sheet_rels),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", &format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/chart" Target="../charts/chart1.xml"/></Relationships>"#)),
            ("xl/charts/chart1.xml", chart),
            ("xl/comments1.xml", comments),
            ("xl/drawings/vmlDrawing1.vml", vml),
            ("xl/tables/table1.xml", table),
        ],
    )
}

/// A shared formula's cells as IronCalc would spell them.
fn formula_at(sheet: u32, r: u32, c: u32) -> Option<String> {
    match (sheet, r, c) {
        (0, r @ 2..=5, 4) => Some(format!("B{r}*C{r}")),
        (0, 6, 4) => Some("SUM(D2:D5)".into()),
        (0, 8, 2) => Some("D4/$D$6".into()),
        (1, 1, 2) => Some("Budget!D6+SUM(Budget!B2:B5)".into()),
        _ => None,
    }
}

fn changed(base: &[u8], s: Shift) -> Result<std::collections::HashMap<String, String>, String> {
    let change = Change { sheet: 0, shift: s, formula_at: &formula_at };
    package(base, &change).map(|b| parts_of(&b))
}

#[test]
fn a_row_put_in_inside_the_totals_moves_every_reference() {
    let base = budget();
    let p = changed(&base, rows(4, 1, true)).unwrap();
    let sheet = &p["xl/worksheets/sheet1.xml"];
    // The cells: renumbered below, the total takes the new row in, the share's reference follows its cell.
    assert!(sheet.contains(r#"<row r="5"><c r="A5" t="inlineStr"><is><t>Twine</t></is></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="D7" s="4"><f>SUM(D2:D6)</f></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="B9" s="6"><f>D5/$D$7</f></c>"#), "{sheet}");
    // The shared formula's cells come apart, each its own (moved) formula.
    assert!(sheet.contains(r#"<c r="D2" s="4"><f>B2*C2</f></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="D5" s="4"><f>B5*C5</f></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="D6" s="4"><f>B6*C6</f></c>"#), "{sheet}");
    assert!(!sheet.contains("t=\"shared\""), "{sheet}");
    // The new row 4 takes the row above's look (row 3's style), Format Same As Above.
    assert!(sheet.contains(r#"<row r="4" s="5" customFormat="1"><c r="C4" s="4"/><c r="D4" s="4"/></row>"#), "{sheet}");
    // Its other parts.
    assert!(sheet.contains(r#"<dimension ref="A1:F13"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<mergeCell ref="A9:A10"/><mergeCell ref="E2:F5"/>"#), "a merge below moved, one across grown: {sheet}");
    assert!(sheet.contains(r#"<conditionalFormatting sqref="D2:D6"><cfRule type="cellIs" dxfId="0" priority="1" operator="greaterThan"><formula>$D$7/2</formula>"#), "{sheet}");
    assert!(sheet.contains(r#"sqref="B2:B6"><formula1>0</formula1><formula2>$B$9*100</formula2>"#), "{sheet}");
    assert!(sheet.contains(r#"<hyperlink ref="A7" location="Budget!D7" display="Total"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<brk id="7" max="16383" man="1"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<selection pane="bottomLeft" activeCell="B6" sqref="B6"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<pane ySplit="1" topLeftCell="A2""#), "frozen header as it was: {sheet}");
    assert!(sheet.contains("<xm:sqref>C2:C6</xm:sqref>"), "{sheet}");
    // Everywhere else.
    assert!(p["xl/worksheets/sheet2.xml"].contains("<f>Budget!D7+SUM(Budget!B2:B6)</f>"), "{}", p["xl/worksheets/sheet2.xml"]);
    let wb = &p["xl/workbook.xml"];
    assert!(wb.contains(r#"<definedName name="Items">Budget!$A$2:$A$6</definedName>"#) && wb.contains(">Budget!$A$1:$D$7</definedName>") && wb.contains(">0.2</definedName>"), "{wb}");
    let chart = &p["xl/charts/chart1.xml"];
    assert!(chart.contains("<c:f>Budget!$D$1</c:f>") && chart.contains("<c:f>Budget!$A$2:$A$6</c:f>") && chart.contains("<c:f>Budget!$D$2:$D$6</c:f>"), "{chart}");
    let drawing = &p["xl/drawings/drawing1.xml"];
    // The chart across the rows grows (row 14 → 15, from 0); the picture below moves, its size kept.
    assert!(drawing.contains("<xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>9</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>15</xdr:row>"), "{drawing}");
    assert!(drawing.contains("<xdr:row>15</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>19</xdr:row>"), "{drawing}");
    assert!(p["xl/comments1.xml"].contains(r#"<comment ref="C6""#));
    let vml = &p["xl/drawings/vmlDrawing1.vml"];
    assert!(vml.contains("<x:Row>5</x:Row>") && vml.contains("<x:Column>2</x:Column>") && vml.contains("3, 15, 4, 10, 5, 15, 8, 4"), "{vml}");
    let table = &p["xl/tables/table1.xml"];
    assert!(table.contains(r#"ref="A11:B13""#) && table.contains(r#"<autoFilter ref="A11:B13"/>"#), "{table}");
    // Parts with nothing to move: as they were, byte for byte.
    let before = parts_of(&base);
    for name in ["xl/styles.xml", "[Content_Types].xml", "xl/drawings/_rels/drawing1.xml.rels", "xl/worksheets/_rels/sheet1.xml.rels"] {
        assert_eq!(before[name], p[name], "{name} as it was");
    }
}

#[test]
fn a_row_taken_out_shrinks_areas_and_refuses_what_it_cant_move() {
    let base = budget();
    let p = changed(&base, rows(4, 1, false)).unwrap();
    let sheet = &p["xl/worksheets/sheet1.xml"];
    assert!(!sheet.contains("Twine"), "{sheet}");
    assert!(sheet.contains(r#"<c r="D5" s="4"><f>SUM(D2:D4)</f></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="B7" s="6"><f>#REF!/$D$5</f></c>"#), "a cell taken out is #REF!: {sheet}");
    assert!(sheet.contains(r#"<mergeCell ref="A7:A8"/><mergeCell ref="E2:F3"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<conditionalFormatting sqref="D2:D4">"#), "{sheet}");
    assert!(p["xl/workbook.xml"].contains("Budget!$A$2:$A$4</definedName>") && p["xl/workbook.xml"].contains("Budget!$A$1:$D$5</definedName>"));
    assert!(sheet.contains(r#"sqref="B2:B4"><formula1>0</formula1><formula2>$B$7*100</formula2>"#), "{sheet}");
    assert!(sheet.contains(r#"<hyperlink ref="A5" location="Budget!D5" display="Total"/>"#), "{sheet}");
    assert!(sheet.contains(r#"<brk id="5" max="16383" man="1"/>"#), "{sheet}");
    assert!(sheet.contains("<xm:sqref>C2:C4</xm:sqref>"), "{sheet}");
    assert!(p["xl/charts/chart1.xml"].contains("<c:f>Budget!$D$2:$D$4</c:f>"));
    assert!(p["xl/worksheets/sheet2.xml"].contains("<f>Budget!D5+SUM(Budget!B2:B4)</f>"));
    assert!(p["xl/tables/table1.xml"].contains(r#"ref="A9:B11""#));
    // The chart over the rows shrinks with them (its last row, 14 from 0, now 13).
    assert!(p["xl/drawings/drawing1.xml"].contains("<xdr:row>13</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:graphicFrame"), "{}", p["xl/drawings/drawing1.xml"]);
    // A merge left one cell, a rule or a list left no cells: gone, and the list's count said.
    let two = changed(&base, rows(8, 2, false)).unwrap();
    assert!(two["xl/worksheets/sheet1.xml"].contains(r#"<mergeCells count="1"><mergeCell ref="E2:F4"/></mergeCells>"#), "{}", two["xl/worksheets/sheet1.xml"]);
    let small = format!(
        r#"<sheetData><row r="1">{}</row><row r="3">{}</row><row r="6">{}</row></sheetData><conditionalFormatting sqref="D2:D5"><cfRule type="cellIs" dxfId="0" priority="1" operator="greaterThan"><formula>10</formula></cfRule></conditionalFormatting><dataValidations count="1"><dataValidation type="whole" sqref="B2:B5"><formula1>0</formula1></dataValidation></dataValidations>"#,
        s("A1", "Item", 1),
        n("B3", 2.0, 0),
        f("D6", "SUM(D2:D5)", 4)
    );
    let all = changed(&xlsx_of(&[("Budget", "", small)], STYLES), rows(2, 4, false)).unwrap();
    let s1 = &all["xl/worksheets/sheet1.xml"];
    assert!(!s1.contains("<conditionalFormatting") && !s1.contains("<dataValidations"), "{s1}");
    assert!(s1.contains(r#"<c r="D2" s="4"><f>SUM(#REF!)</f></c>"#), "{s1}");
    // The comment on C5 sits in row 5: moved up, not taken out.
    assert!(p["xl/comments1.xml"].contains(r#"<comment ref="C4""#));
    // Its own row taken out: refused, the comment never lost.
    assert!(changed(&base, rows(5, 1, false)).unwrap_err().contains("comment"));
    // The picture's rows taken out: refused.
    assert!(changed(&base, rows(15, 1, false)).unwrap_err().contains("picture"));
    // A table's heading taken out: refused.
    assert!(changed(&base, rows(10, 1, false)).unwrap_err().contains("table"));
}

#[test]
fn columns_move_widths_and_the_cells_to_the_right() {
    let base = budget();
    let p = changed(&base, cols(3, 1, true)).unwrap();
    let sheet = &p["xl/worksheets/sheet1.xml"];
    assert!(sheet.contains(r#"<c r="E7" s="4"><f>SUM(E2:E5)</f></c>"#) || sheet.contains(r#"<c r="E6" s="4"><f>SUM(E2:E5)</f></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="E2" s="4"><f>B2*D2</f></c>"#), "{sheet}");
    assert!(sheet.contains(r#"<c r="B8" s="6"><f>E4/$E$6</f></c>"#), "{sheet}");
    // The new column C takes column B's look where B had one (Format Same As Left): the heading's bold.
    assert!(sheet.contains(r#"<is><t>Qty</t></is></c><c r="C1" s="1"/><c r="D1" s="1" t="inlineStr"><is><t>Price</t>"#), "{sheet}");
    assert!(!sheet.contains(r#"<c r="C2""#), "no look, no cell: {sheet}");
    assert!(p["xl/comments1.xml"].contains(r#"<comment ref="D5""#));
    assert!(p["xl/worksheets/sheet2.xml"].contains("<f>Budget!E6+SUM(Budget!B2:B5)</f>"));
    // A table's columns cut apart: refused.
    assert!(changed(&base, cols(2, 1, true)).unwrap_err().contains("table"));
}

#[test]
fn a_part_yap_cant_move_is_refused() {
    let base = budget();
    let pivot = with_parts(&base, &[("xl/pivotCache/pivotCacheDefinition1.xml", r#"<pivotCacheDefinition><cacheSource type="worksheet"><worksheetSource ref="A1:D5" sheet="Budget"/></cacheSource></pivotCacheDefinition>"#)]);
    assert!(changed(&pivot, rows(4, 1, true)).unwrap_err().contains("pivot"));
    let data = format!(r#"<sheetData><row r="1">{}</row></sheetData><scenarios><scenario name="High"/></scenarios>"#, n("A1", 1.0, 0));
    let odd = xlsx_of(&[("Budget", "", data)], STYLES);
    assert!(changed(&odd, rows(1, 1, true)).unwrap_err().contains("scenarios"));
    let array = r#"<sheetData><row r="2"><c r="B2"><f t="array" ref="B2:B4">A2:A4*2</f></c></row></sheetData>"#.to_string();
    let arr = xlsx_of(&[("Budget", "", array)], STYLES);
    assert!(changed(&arr, rows(3, 1, true)).unwrap_err().contains("array"));
    assert!(changed(&arr, rows(2, 1, true)).is_ok(), "the whole array moved: fine");
}
