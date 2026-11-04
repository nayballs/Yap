// Speech for the robot's microphone tests: Windows' own text-to-speech
// (System.Speech) written straight to a 16 kHz mono WAV file — never
// played through the speakers. Cached by text under test-results/real/cache.
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { CACHE } from './targets.js';
import { writeWav } from '../../e2e/support/wav.js';

export function speechWav(text) {
  const file = path.join(CACHE, 'tts', `${crypto.createHash('sha1').update(text).digest('hex').slice(0, 12)}.wav`);
  if (fs.existsSync(file) && fs.statSync(file).size > 44) return file;
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const script = `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$s.SetOutputToWaveFile($env:YAP_TTS_FILE, $fmt)
$s.Rate = -1
$s.Speak($env:YAP_TTS_TEXT)
$s.Dispose()
`;
  execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    env: { ...process.env, YAP_TTS_FILE: file, YAP_TTS_TEXT: text },
    windowsHide: true,
  });
  return file;
}

/** `seconds` of digital silence (every sample exactly 0: a muted mic's). */
export function silenceWav(seconds) {
  const file = path.join(CACHE, 'tts', `silence-${seconds}s.wav`);
  if (!fs.existsSync(file)) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    writeWav(file, { seconds, amplitude: 0 });
  }
  return file;
}
