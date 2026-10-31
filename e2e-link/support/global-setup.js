// Before any browser starts: the fake microphone's WAV files.
import { writeAudio } from './audio.js';

export default function globalSetup() {
  writeAudio();
}
