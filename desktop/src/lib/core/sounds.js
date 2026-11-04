// Yap's sounds (Settings → Dictation → Sounds): the packs and the moments
// with a sound, in the words Settings shows. The sounds themselves are made
// in Rust (desktop/src-tauri/src/dictation/sound/packs.rs), by these ids.

export const SOUND_PACKS = [
  { id: 'classic', name: 'Classic', desc: 'Two bright notes, as Yap has always played' },
  { id: 'soft', name: 'Soft', desc: 'Lower, warmer notes that fade gently' },
  { id: 'crisp', name: 'Crisp', desc: 'Short glassy bells with a light tick' },
  { id: 'minimal', name: 'Minimal', desc: 'One quick blip, rising or falling' },
];

// Each moment's switch (its config field) and the sounds ▶ plays for it.
export const SOUND_MOMENTS = [
  { field: 'soundStart', cues: ['start'], label: 'Dictation starts', desc: 'When Yap starts hearing you', setting: 'sound-start' },
  { field: 'soundStop', cues: ['stop'], label: 'Dictation stops', desc: 'When you finish and Yap writes it up', setting: 'sound-stop' },
  { field: 'soundLocked', cues: ['locked'], label: 'Hands-free', desc: 'When a double-tap keeps push-to-talk recording', setting: 'sound-locked' },
  { field: 'soundCancelled', cues: ['cancelled'], label: 'Dictation cancelled', desc: 'When you cancel with ✕ or from the tray', setting: 'sound-cancelled' },
  { field: 'soundError', cues: ['error'], label: 'Something went wrong', desc: "When a dictation couldn't be pasted or transcribed", setting: 'sound-error' },
  { field: 'soundSlow', cues: ['slow'], label: 'Taking longer than usual', desc: 'When a dictation is slower than it usually is', setting: 'sound-slow' },
  { field: 'soundMeeting', cues: ['meetingStart', 'meetingStop'], label: 'Meeting notes start and stop', desc: 'As Yap starts and stops recording a meeting', setting: 'sound-meeting' },
  { field: 'soundNotice', cues: ['notice'], label: 'A notice on the Yap bar', desc: 'When a card comes up above the bar', setting: 'sound-notice' },
];
