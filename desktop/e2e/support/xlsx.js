// Small workbooks (.xlsx) for the spreadsheet specs, made here as openpyxl writes them: inline strings, and
// formulas with no results (Yap calculates them: chat/sheets/xlsx.rs). The same fictional seed order as the
// Rust tests' (chat/sheets/tests.rs). Excel opens them too.
import zlib from 'node:zlib';
import { zip } from './docx.js';

/** **A zip's parts** (an .xlsx read back from disk): name → its text. */
export function partsOf(buf) {
  const out = {};
  let end = buf.length - 22;
  while (end >= 0 && buf.readUInt32LE(end) !== 0x06054b50) end--;
  if (end < 0) throw new Error('not a zip');
  const count = buf.readUInt16LE(end + 10);
  let p = buf.readUInt32LE(end + 16);
  for (let i = 0; i < count; i++) {
    const method = buf.readUInt16LE(p + 10);
    const size = buf.readUInt32LE(p + 20);
    const [nameLen, extraLen, noteLen] = [buf.readUInt16LE(p + 28), buf.readUInt16LE(p + 30), buf.readUInt16LE(p + 32)];
    const local = buf.readUInt32LE(p + 42);
    const name = buf.toString('utf8', p + 46, p + 46 + nameLen);
    const from = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
    const data = buf.subarray(from, from + size);
    out[name] = (method === 8 ? zlib.inflateRawSync(data) : data).toString('utf8');
    p += 46 + nameLen + extraLen + noteLen;
  }
  return out;
}

const MAIN = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const st = (style) => (style ? ` s="${style}"` : '');
const s = (r, text, style = 0) => `<c r="${r}"${st(style)} t="inlineStr"><is><t>${esc(text)}</t></is></c>`;
const n = (r, v, style = 0) => `<c r="${r}"${st(style)}><v>${v}</v></c>`;
const f = (r, formula, style = 0) => `<c r="${r}"${st(style)}><f>${esc(formula)}</f></c>`;

/** Styles: 0 plain, 1 bold, 2 the title (bold 14, a blue fill, centred), 3 a header (bold, a line under it), 4 pounds, 5 a date, 6 per cent, 7 italic red, 8 the theme's accent. */
const STYLES =
  '<numFmts count="2"><numFmt numFmtId="164" formatCode="&quot;£&quot;#,##0.00"/><numFmt numFmtId="165" formatCode="dd/mm/yyyy"/></numFmts>' +
  '<fonts count="5"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/></font><font><b/><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/></font><font><b/><sz val="14"/><color theme="1"/><name val="Calibri"/><family val="2"/></font><font><i/><sz val="11"/><color rgb="FFFF0000"/><name val="Calibri"/><family val="2"/></font><font><sz val="11"/><color theme="4"/><name val="Calibri"/><family val="2"/></font></fonts>' +
  '<fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFDDEBF7"/><bgColor indexed="64"/></patternFill></fill></fills>' +
  '<borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border><left/><right/><top/><bottom style="thin"><color auto="1"/></bottom><diagonal/></border></borders>' +
  '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>' +
  '<cellXfs count="9"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="2" fillId="2" borderId="0" xfId="0" applyFont="1" applyFill="1" applyAlignment="1"><alignment horizontal="center"/></xf><xf numFmtId="0" fontId="1" fillId="0" borderId="1" xfId="0" applyFont="1" applyBorder="1"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="165" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="9" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="0" fontId="3" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="4" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs>' +
  '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>';

/** An .xlsx of `sheets` ([{ name, state?, xml }], the inside of each `<worksheet>`). A Buffer. */
export function xlsxOf(sheets) {
  const files = {};
  let types = '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>';
  let book = `<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="${MAIN}" xmlns:r="${REL}"><sheets>`;
  let rels = '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">';
  sheets.forEach((sh, i) => {
    const k = i + 1;
    book += `<sheet name="${esc(sh.name)}" sheetId="${k}"${sh.state ? ` state="${sh.state}"` : ''} r:id="rId${k}"/>`;
    rels += `<Relationship Id="rId${k}" Type="${REL}/worksheet" Target="worksheets/sheet${k}.xml"/>`;
    types += `<Override PartName="/xl/worksheets/sheet${k}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`;
    files[`xl/worksheets/sheet${k}.xml`] = `<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="${MAIN}" xmlns:r="${REL}">${sh.xml}</worksheet>`;
  });
  book += '</sheets></workbook>';
  rels += `<Relationship Id="rId99" Type="${REL}/styles" Target="styles.xml"/></Relationships>`;
  types += '</Types>';
  return zip({
    '[Content_Types].xml': types,
    '_rels/.rels': `<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    'xl/workbook.xml': book,
    'xl/_rels/workbook.xml.rels': rels,
    'xl/styles.xml': `<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="${MAIN}">${STYLES}</styleSheet>`,
    ...files,
  });
}

/**
 * **A seed order** (fictional): a title merged over A1:D1, a header row, three items whose totals are formulas
 * with no results, a total, prices in pounds, a date, a share in per cent, the header rows frozen, two widths;
 * a sheet of notes and a hidden one. `kale`: how many packets of kale (a second version changes it).
 */
export function seedOrderXlsx({ kale = 4 } = {}) {
  const budget = [
    '<sheetViews><sheetView workbookViewId="0"><pane ySplit="2" topLeftCell="A3" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>',
    '<cols><col min="1" max="1" width="20" customWidth="1"/><col min="2" max="2" width="8" customWidth="1"/><col min="5" max="5" width="12" customWidth="1"/></cols>',
    '<sheetData>',
    `<row r="1" ht="21" customHeight="1">${s('A1', 'Seed order', 2)}<c r="B1" s="2"/><c r="C1" s="2"/><c r="D1" s="2"/></row>`,
    `<row r="2">${s('A2', 'Item', 3)}${s('B2', 'Qty', 3)}${s('C2', 'Price', 3)}${s('D2', 'Total', 3)}${s('E2', 'Ordered', 3)}${s('F2', 'Share', 3)}</row>`,
    `<row r="3">${s('A3', 'Carrots')}${n('B3', 3)}${n('C3', 2.5, 4)}${f('D3', 'B3*C3', 4)}${n('E3', 46341, 5)}${f('F3', 'D3/D$6', 6)}</row>`,
    `<row r="4">${s('A4', 'Runner beans')}${n('B4', 2)}${n('C4', 1.75, 4)}${f('D4', 'B4*C4', 4)}${f('F4', 'D4/D$6', 6)}</row>`,
    `<row r="5">${s('A5', 'Kale', 8)}${n('B5', kale)}${n('C5', 3, 4)}${f('D5', 'B5*C5', 4)}${f('F5', 'D5/D$6', 6)}</row>`,
    `<row r="6">${s('A6', 'Total', 1)}${f('D6', 'SUM(D3:D5)', 4)}</row>`,
    `<row r="8">${s('A8', 'Sown in the polytunnel from March, planted out in May once the frosts are over.')}</row>`,
    '</sheetData>',
    '<mergeCells count="1"><mergeCell ref="A1:D1"/></mergeCells>',
  ].join('');
  const notes = `<sheetData><row r="1">${s('A1', 'Order by Friday', 7)}</row><row r="2">${s('A2', 'Plot 7 & plot 9')}</row></sheetData>`;
  const old = `<sheetData><row r="1">${s('A1', 'Last year')}</row></sheetData>`;
  return xlsxOf([
    { name: 'Budget', xml: budget },
    { name: 'Notes', xml: notes },
    { name: 'Old', state: 'hidden', xml: old },
  ]);
}

/** A big sheet: `rows` rows of five columns (an id, a bed, a crop, rows, plants as a formula), the header frozen. */
export function bigXlsx(rows) {
  const crops = ['Carrots', 'Kale', 'Leeks', 'Onions', 'Peas'];
  const out = ['<sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetData>'];
  out.push(`<row r="1">${s('A1', 'Id', 1)}${s('B1', 'Bed', 1)}${s('C1', 'Crop', 1)}${s('D1', 'Rows', 1)}${s('E1', 'Plants', 1)}</row>`);
  for (let r = 2; r <= rows; r++) out.push(`<row r="${r}">${n(`A${r}`, r - 1)}${s(`B${r}`, `Bed ${r % 40}`)}${s(`C${r}`, crops[r % 5])}${n(`D${r}`, (r % 7) + 1)}${f(`E${r}`, `D${r}*12`)}</row>`);
  out.push('</sheetData>');
  return xlsxOf([{ name: 'Beds', xml: out.join('') }]);
}
