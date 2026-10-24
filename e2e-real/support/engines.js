// The robot's real-STT build and model (stt.real.js): where the `engines`
// build is, and the Parakeet model copied READ-ONLY from your installed
// Yap's models folder into a cache, then hard-linked into the robot's own
// portable data dir. Nothing else in %APPDATA%\yap is read.
import fs from 'node:fs';
import path from 'node:path';
import { REPO } from '../../e2e/support/yap.js';
import { CACHE } from './targets.js';

/** The engines build's Cargo target dir: short, on the repo's drive (see build.mjs). */
export const ENGINES_TARGET_DIR =
  process.env.YAP_REAL_ENGINES_TARGET_DIR || path.join(path.parse(REPO).root, 't-real');

export const ENGINES_EXE = process.env.YAP_REAL_ENGINES_EXE || path.join(ENGINES_TARGET_DIR, 'debug', 'yap.exe');

/** Yap's default model (dictation/stt.rs: id "parakeet-tdt-0.6b-v3", folder below). */
export const MODEL_ID = 'parakeet-tdt-0.6b-v3';
export const MODEL_DIR = 'parakeet-tdt-0.6b-v3-int8';
export const MODEL_SRC =
  process.env.YAP_REAL_MODEL_DIR || path.join(process.env.APPDATA || '', 'yap', 'models', MODEL_DIR);

/**
 * A Whisper model for the specs that compare (screen-names.real.js): dictation/stt.rs'
 * "large" (large-v3, q5_0), from the same folder, if it's there.
 */
export const WHISPER_ID = 'large';
export const WHISPER_FILE = 'ggml-large-v3-q5_0.bin';
export const WHISPER_SRC =
  process.env.YAP_REAL_WHISPER_FILE || path.join(process.env.APPDATA || '', 'yap', 'models', WHISPER_FILE);

/** Whether the Whisper model is there to link. */
export const hasWhisper = () => fs.existsSync(WHISPER_SRC);

/**
 * Put the Whisper model in a robot's data dir as `linkModel` does Parakeet:
 * copied once into the cache (reading the source only), then hard-linked.
 */
export function linkWhisper(dataDir) {
  const cache = path.join(CACHE, 'models');
  fs.mkdirSync(cache, { recursive: true });
  const cached = path.join(cache, WHISPER_FILE);
  if (!fs.existsSync(cached) || fs.statSync(cached).size !== fs.statSync(WHISPER_SRC).size) {
    fs.copyFileSync(WHISPER_SRC, cached);
  }
  const target = path.join(dataDir, 'models', WHISPER_FILE);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  try {
    fs.linkSync(cached, target);
  } catch {
    fs.copyFileSync(cached, target);
  }
}

/** The model's files (not the "._*" tarball leftovers some folders carry). */
function modelFiles(dir) {
  return fs.readdirSync(dir).filter((f) => !f.startsWith('._') && fs.statSync(path.join(dir, f)).isFile());
}

/** Why the STT spec can't run, or null. */
export function sttUnavailable() {
  if (!fs.existsSync(ENGINES_EXE)) return `No engines build at ${ENGINES_EXE} (npm run test:real:build -- --engines)`;
  if (!fs.existsSync(MODEL_SRC) || modelFiles(MODEL_SRC).length === 0) {
    return `No Parakeet V3 model at ${MODEL_SRC} (download it in Yap, or set YAP_REAL_MODEL_DIR)`;
  }
  return null;
}

/**
 * Put the model in a robot's data dir: copied once into the cache (reading
 * the source only), then hard-linked (a copy if links can't be made).
 */
export function linkModel(dataDir) {
  const cache = path.join(CACHE, 'models', MODEL_DIR);
  fs.mkdirSync(cache, { recursive: true });
  for (const f of modelFiles(MODEL_SRC)) {
    const src = path.join(MODEL_SRC, f);
    const dst = path.join(cache, f);
    if (!fs.existsSync(dst) || fs.statSync(dst).size !== fs.statSync(src).size) fs.copyFileSync(src, dst);
  }
  const target = path.join(dataDir, 'models', MODEL_DIR);
  fs.mkdirSync(target, { recursive: true });
  for (const f of modelFiles(cache)) {
    try {
      fs.linkSync(path.join(cache, f), path.join(target, f));
    } catch {
      fs.copyFileSync(path.join(cache, f), path.join(target, f));
    }
  }
}
