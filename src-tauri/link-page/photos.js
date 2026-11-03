// A question's pictures, from the phone's camera or its photos: made
// smaller here (a phone's photo is 4–12 MB; the PC needs no more than 2,048
// px on the longest side, Chat makes its own copy for the model from it),
// turned upright as the phone shows them (`imageOrientation: 'from-image'`),
// on white (no see-through corners), as a JPEG of at most MAX_BYTES; and,
// when the question goes, sent as the protocol says (docs/architecture/
// link.md): a `picture {n, size}` message, then the bytes in `0x03` frames
// of at most PIECE each. The PC keeps the caps too (link/pictures.rs) and
// reads each as a picture, as Chat reads one pasted (snip/attach.rs).

/** The most pictures a question carries (Chat's own). */
export const MAX_PICTURES = 4;
/** A picture's longest side, at most. */
export const MAX_EDGE = 2048;
/** A picture's bytes, at most (the PC refuses more: link/wire.rs MAX_PICTURE_BYTES). */
export const MAX_BYTES = 3 * 1024 * 1024;
/** A frame's share of a picture: within the PC's 32,005-byte frames. */
export const PIECE = 32000;
/** A picture's frame kind. */
export const KIND_PICTURE = 0x03;

/** `w` × `h` fitted within `edge` on its longest side (never made bigger). Pure. */
export function fit(w, h, edge) {
  const long = Math.max(w, h);
  if (long <= edge) return { w, h };
  const k = edge / long;
  return { w: Math.max(1, Math.round(w * k)), h: Math.max(1, Math.round(h * k)) };
}

/** The tries, best first, until the JPEG is small enough: [longest side, quality]. */
const TRIES = [
  [MAX_EDGE, 0.85],
  [MAX_EDGE, 0.72],
  [1600, 0.72],
  [1280, 0.65],
  [1024, 0.6],
];

async function bitmap(file) {
  try {
    return await createImageBitmap(file, { imageOrientation: 'from-image' });
  } catch (err) {
    // An older browser without the option still turns a JPEG upright itself.
    if (err && err.name === 'TypeError') return createImageBitmap(file);
    throw err;
  }
}

function canvasOf(w, h) {
  if (typeof OffscreenCanvas === 'function') return new OffscreenCanvas(w, h);
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  return c;
}

function jpeg(canvas, quality) {
  if (canvas.convertToBlob) return canvas.convertToBlob({ type: 'image/jpeg', quality });
  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('NoBlob'))), 'image/jpeg', quality),
  );
}

/**
 * A photo made ready to send: `{blob, buf, url, w, h, bytes}` (`url` a
 * blob: URL for its thumbnail). Throws when it isn't a picture the browser
 * can read.
 */
export async function prepare(file) {
  const bmp = await bitmap(file);
  try {
    let out = null;
    for (const [edge, quality] of TRIES) {
      const { w, h } = fit(bmp.width, bmp.height, edge);
      const canvas = canvasOf(w, h);
      const ctx = canvas.getContext('2d');
      ctx.fillStyle = '#ffffff';
      ctx.fillRect(0, 0, w, h);
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(bmp, 0, 0, w, h);
      const blob = await jpeg(canvas, quality);
      out = { blob, w, h };
      if (blob.size <= MAX_BYTES) break;
    }
    if (!out || out.blob.size > MAX_BYTES) throw Object.assign(new Error('TooBig'), { name: 'TooBig' });
    const buf = await out.blob.arrayBuffer();
    return { ...out, buf, bytes: buf.byteLength, url: URL.createObjectURL(out.blob) };
  } finally {
    if (bmp.close) bmp.close();
  }
}

/** Picture `n`'s frames: kind 0x03, the piece's number (u32 LE), its bytes. Pure. */
export function frames(buf) {
  const bytes = new Uint8Array(buf);
  const out = [];
  for (let at = 0, seq = 0; at < bytes.length; at += PIECE, seq++) {
    const piece = bytes.subarray(at, Math.min(at + PIECE, bytes.length));
    const f = new Uint8Array(5 + piece.length);
    f[0] = KIND_PICTURE;
    new DataView(f.buffer).setUint32(1, seq, true);
    f.set(piece, 5);
    out.push(f.buffer);
  }
  return out;
}

/**
 * Sends `pictures` (each `{buf}`) before the question that takes them:
 * `send(msg)` for a text message, `sendBinary(buf)` for a frame. False when
 * the socket wasn't there for them.
 */
export function sendPictures(pictures, send, sendBinary) {
  for (let n = 0; n < pictures.length; n++) {
    const p = pictures[n];
    if (!send({ type: 'picture', n, size: p.buf.byteLength })) return false;
    for (const f of frames(p.buf)) if (!sendBinary(f)) return false;
  }
  return true;
}
