// An app's name as people know it, from the process name Yap records with a
// dictation (history.json's `app`: "ms-teams.exe", "winword.exe"). Home's
// feed, Insights' "Where you dictate" and Settings' per-app rules show it;
// what's stored stays the process name. Pure, shared.

const KNOWN = {
  'ms-teams': 'Teams',
  msteams: 'Teams',
  teams: 'Teams',
  msedge: 'Edge',
  chrome: 'Chrome',
  firefox: 'Firefox',
  brave: 'Brave',
  opera: 'Opera',
  vivaldi: 'Vivaldi',
  arc: 'Arc',
  code: 'VS Code',
  'code - insiders': 'VS Code Insiders',
  cursor: 'Cursor',
  windsurf: 'Windsurf',
  devenv: 'Visual Studio',
  idea64: 'IntelliJ IDEA',
  pycharm64: 'PyCharm',
  rider64: 'Rider',
  sublime_text: 'Sublime Text',
  'notepad++': 'Notepad++',
  notepad: 'Notepad',
  windowsterminal: 'Windows Terminal',
  wt: 'Windows Terminal',
  powershell: 'PowerShell',
  pwsh: 'PowerShell',
  cmd: 'Command Prompt',
  conhost: 'Console',
  mintty: 'Git Bash',
  'wezterm-gui': 'WezTerm',
  alacritty: 'Alacritty',
  winword: 'Word',
  excel: 'Excel',
  powerpnt: 'PowerPoint',
  outlook: 'Outlook',
  olk: 'Outlook',
  onenote: 'OneNote',
  slack: 'Slack',
  discord: 'Discord',
  whatsapp: 'WhatsApp',
  'whatsapp.root': 'WhatsApp',
  telegram: 'Telegram',
  signal: 'Signal',
  zoom: 'Zoom',
  obsidian: 'Obsidian',
  notion: 'Notion',
  claude: 'Claude',
  chatgpt: 'ChatGPT',
  explorer: 'File Explorer',
  thunderbird: 'Thunderbird',
  spotify: 'Spotify',
  yap: 'Yap',
};

/** "ms-teams.exe" → "Teams", "winword.exe" → "Word", "myapp.exe" → "Myapp". */
export function appLabel(app) {
  const base = String(app || '')
    .trim()
    .replace(/\.exe$/i, '');
  if (!base) return '';
  return KNOWN[base.toLowerCase()] ?? base.charAt(0).toUpperCase() + base.slice(1);
}
