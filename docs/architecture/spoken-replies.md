# Spoken replies

Reading answers aloud: Kokoro and Windows' voices on this PC, or ElevenLabs on the person's own key; text prepared for speech, the speaker queue and when Yap talks. ("§Current status" below means [status.md](./status.md).)

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`tts.rs`** — **spoken replies** (Settings → Voice Assistant, Chat's Voice
  replies toggle, ▶ on answers). **Who speaks** (`config.tts_provider`,
  `voicing(cfg)` → `Voicing {engine, voice, cloud}`): "local" (the
  default — the engines below, `chosen(cfg)`) or a **cloud voice**,
  ElevenLabs (`tts/elevenlabs.rs`, below) — only when it's picked **and**
  has a key (`tts_cloud_keys`); without one the local voice speaks. A cloud
  voicing carries a `CloudPlan`: the local voice that can stand in
  (`local_voice`: Kokoro installed, or Windows voices chosen — not Windows
  voices as a stand-in for an undownloaded Kokoro) and the key/choice
  fingerprints. `speak(text, id)` is the one way in — an answer's
  markdown or any plain text (a later "read the selection aloud" reuses
  it). **Engines** (`Engine` → `Synth`, text → `Pcm`; `Synth::stream`
  hands audio on as it's made — a cloud voice's first piece plays before
  the sentence is all made; the default makes the whole sentence;
  `prepare` (Kokoro and ElevenLabs join sentences under 3 words to the
  next), `cancel`): **Kokoro** (`tts/kokoro.rs`, below; the default) and
  **Windows voices** (`WindowsVoices`: WinRT `SpeechSynthesizer`,
  `SynthesizeTextToStreamAsync` → WAV → `parse_wav`; rate via
  `SpeakingRate`, minimal appended silence). `chosen(cfg)`: Kokoro with
  `tts_voice` when `tts_engine` isn't "windows" and Kokoro can speak
  (installed, in this build), else Windows voices with `tts_windows_voice`
  — so Windows voices speak until Kokoro is downloaded, and Kokoro takes
  over by itself once it is unless Windows was chosen. **Which Windows
  voices apps see**: only the OneCore voices
  (`HKLM\…\Speech_OneCore\Voices`, e.g. this PC's en-GB George, Hazel,
  Susan — the default is George); not the old SAPI-only desktop voices
  (Zira here) and not Windows 11's natural Narrator voices (Aria, Jenny,
  Guy…), which are Narrator-only. **Text prep** (pure, unit-tested):
  `speakable_blocks` (markdown → one block per heading / list item / table
  row / paragraph, each ending like a sentence; a fenced code block →
  "(code block)", links → their text, bare URLs → their domain, `[x]`/☑ →
  "Done: …", emphasis, HTML, emoji and arrows out, "→" read "to", a web
  answer's `[1]` / `[2, 3]` / gpt-oss `【…】` markers never read), then
  **`tts/normalize.rs`** (`for_speech`, every engine, UK style,
  conservative — a clear pattern or nothing: £ $ € amounts incl. pence/cents
  and k/m/bn ("twelve thousand four hundred pounds", "three dollars fifty"),
  numbers with separators, decimals ("three point one four"), percentages,
  ordinals, dates ("18 Oct" → "the eighteenth of October", "October 14th" →
  "October the fourteenth", ISO and dd/mm/yyyy — month-first only when the
  day can't be a month —, "Sat" before a date, years after "in"/"since"/a
  month, "the 1990s"), times ("2:30 pm" → "two thirty p.m.", "14:00" →
  "fourteen hundred"), ranges ("10-15" climbing only), units after a number
  (km, kg, GB, °C, ms…), versions ("v2.1", "1.2.3"), "Q3" → "Q three", "4K",
  e.g./i.e./etc./vs./approx./Dr/Mr/No./#/&/w/, emails ("… at example dot
  com") and domains ("github dot com"), codes and long numbers digit by
  digit) and `sentences` (split after `.?!…` + space, never after an
  abbreviation or an initial — `app_kind::is_abbreviation` — over 220
  chars cut at a comma); `utterances` = all three. **The speaker**
  (`Speaker`, unit-tested on a simulated clock): one utterance at a time on
  a worker thread — "speaking" at once, the output opened on the worker
  (rodio on `config.output_device`), each sentence made while the one before
  plays; `stop()` empties the sink and drops the queue at once; a new
  utterance replaces the old; it checks before every sentence and while one
  plays that no dictation records (`pipeline::dictation_state`), refuses to
  start over one, and refuses while a meeting records (the loopback would
  put Yap in "Them"; `meeting::start` stops it). Each sentence is
  `stream`ed into the sink piece by piece (each piece checked: stopped, or
  a dictation started → the stream ends). **A cloud voice failing**
  (`Job.fallback`: `Fallback {local, notify}`): the rest of that answer —
  the failed sentence included — goes to the local voice, if there's one,
  else the answer stops there; the failing engine is never asked again for
  that answer (no retry loop), and `notify` runs once:
  `cloud_failed` → `alert::show_soon` kind **"voiceFallback"** (the main
  window's toast while it's focused, else a Yap bar card, else a Windows
  notification; icon "assistant", **Open settings** →
  `OPEN_VOICE_SETTINGS` → Settings → Voice Assistant `voice#voice-provider`):
  "ElevenLabs didn't answer (quota used up)" + "Speaking with Yap's own
  voice." / "Yap's own voice isn't downloaded, so this answer wasn't read
  aloud." + what to do — said once per reason in 10 minutes
  (`cloud::Health::announce`); the state then carries no error (no second
  toast). `yap-tts-state {speaking, messageId, error}` on every change. **When to speak** (pure):
  `auto_speak(speak_replies, speak_when, asked_by_voice, dictating,
  meeting)` (chat_send's `speak`), `sends_by_itself(speak_replies,
  hands_free, said_enter)` — the Voice Assistant's answers ignore both
  (`assistant::reply`), and every state change is passed to
  `assistant::on_tts_state` (its answer ended). Commands `tts_voices` (`{provider,
  providers: [{id, name, keysPage}], engine, active,
  engines, kokoro: {voices, defaultVoice}, kokoroStatus, windows: {voices,
  defaultVoice, error}}`), the cloud voice's `tts_cloud_voices` /
  `tts_cloud_models` / `tts_cloud_usage` / `tts_cloud_preview {provider,
  voice, apiKey?, model?, rate?}` (the key Settings has now, else the saved
  one), `tts_speak {text, messageId}`, `tts_preview
  {voice, rate, engine?}` (Kokoro not installed → "Download Kokoro to hear
  its voices"), `tts_stop`, `tts_state`, `tts_kokoro_status` /
  `tts_kokoro_download` (`tts-download-progress {percent, downloadedBytes,
  totalBytes}`, then `tts-kokoro-changed`) / `tts_kokoro_delete` (stops
  speaking, unloads), `voice_replies_set {on}` (Chat's toggle: saves like
  Settings, then `yap-voice-replies {on}` for Settings' copy). A
  `tts-idle` thread unloads Kokoro after the speech model's unload time (10
  min when that's "never"). ⚠ **No echo cancellation**: Yap's voice from
  speakers can reach the mic; the barge-in stops it before a dictation and
  drops the pre-roll, but a mic hearing the speakers (no headset) during an
  answer is a later job. Test mode: stand-in Windows voices, Kokoro as
  silence once installed from the suite's local server, a simulated clock
  (`SimPlayback`), no device; what would have been said (with its `engine`)
  goes to the debug-only `tts_e2e_spoken {clear?}` (a cloud voice's
  stand-in taking over adds a record with its `fallback` error; `clear`
  also forgets the cloud's last failure). An `#[ignore]`d
  `windows_voices_on_this_pc` lists the voices and synthesises a sentence
  to a WAV in a temp folder (never played); `first_audio_latency`
  (`--features engines`) times `speak` → first piece at the output for
  Kokoro and for ElevenLabs' path against a local stand-in
  (`YAP_TTS_STANDIN_MS`).
  - **`tts/cloud.rs`** — what any cloud voice shares (another service —
    OpenAI's, Azure's — slots in beside ElevenLabs): `PROVIDERS` ("local"
    "On this PC", "elevenlabs" "ElevenLabs"), `Failure` (BadKey,
    NotAllowed, Quota, RateLimited, ServiceDown, Offline, TimedOut,
    VoiceGone, ModelGone, Rejected(their words)) with its `reason` ("quota
    used up"), `advice` and `skip_for`, `notice_text`, and **`Health`**
    (pure, unit-tested on a simulated clock): the last failure per
    provider by key and key + voice + model fingerprint (`fingerprint`,
    in memory only, never the key); `announce` says a reason once per
    `ANNOUNCE_EVERY` (10 min); `skipping` sends the next answers straight
    to the local voice — when there is one — for `skip_for` (30 s after a
    rate limit, 60 s offline / timed out / 5xx, 5 min refused, 15 min quota,
    30 min bad key / not allowed / voice or model gone); another key, voice
    or model, or any request that works (Settings' list, ▶), starts afresh.
  - **`tts/elevenlabs.rs`** — **ElevenLabs**, on the person's own key (API
    as of 22 Oct 2026). Speech: `POST /v1/text-to-speech/{voice}/stream?
    output_format=pcm_24000` with `{text, model_id, voice_settings?:
    {speed}}` (raw 16-bit LE mono PCM at 24 kHz, every plan; `speed` only
    when Yap's isn't 1×, clamped to ElevenLabs' 0.7–1.2) — `stream_speech`
    decodes it as it arrives (`S16Le`, a sample split across pieces kept)
    and hands on ≥ 100 ms at a time, then 150 ms of quiet; 4 s to connect,
    6 s to the first audio, 6 s between pieces (`TimedOut`); a stop
    (`cancel`: a counter + a tokio `Notify`) ends a sentence still being
    fetched at once. Model: **`eleven_flash_v2_5`** by default (the docs'
    lowest latency, ~75 ms; 32 languages); the picker reads `GET
    /v1/models` (`can_do_text_to_speech`, no alpha), else `KNOWN_MODELS`.
    Voices: `GET /v2/voices?page_size=100` (≤ 3 pages; the account's own
    voices plus ElevenLabs' defaults on page 0), the account's own first;
    default voice **George** (`JBFqnCBsd6RMkjVDRZzb`). Usage: `GET
    /v1/user/subscription` → `character_count` / `character_limit` /
    `next_character_count_reset_unix`; any error → none shown. ▶ plays a
    voice's own `preview_url` (`Sample` engine: fetched on the speaker's
    thread when clicked, without the key, https only — a test run: only
    the stand-in —, ≤ 4 MB, decoded by Symphonia) from the list Yap read
    last (`PREVIEWS`: never a URL a page hands over); a voice without one
    says the preview sentence through the service. **The key**: `Key`
    (`Debug` redacted), only in the `xi-api-key` header, never a URL, a log
    line or an error (`tidy` cuts it from anything the service says back);
    keyed requests follow no redirect. Errors: `classify` reads `{detail:
    {code | status, message}}` — `invalid_api_key` (401) → BadKey,
    `insufficient_credits` (402) / the older 401 `quota_exceeded` → Quota,
    `missing_permissions` / `insufficient_permissions` / 403 → NotAllowed,
    429 → RateLimited, `voice_not_found` / 404 → VoiceGone, 5xx →
    ServiceDown. Debug-only `YAP_ELEVENLABS_URL` names a test run's
    stand-in (`e2e/support/fake-elevenlabs.js`); without it a test run
    never reaches the internet. Unit tests run against an in-process
    stand-in (`standin`: chunked PCM in odd pieces, errors, silence).
  - **`tts/kokoro.rs`** — **Kokoro-82M v1.0** (hexgrad, Apache-2.0), the
    natural voice: onnx-community's **fp32** export (the fp16 one turns
    longer British sentences into NaN — `checks::nan_probe`; int8 is 3.5–4×
    slower on x86; DirectML can't run it) on Yap's own `ort` (pinned
    `=2.0.0-rc.12`, the `engines` feature, so one ONNX Runtime), **CPU**,
    `min(4, logical cores / 2)` intra-op threads that never spin, at
    **24,000 Hz** (Voice Mirror's 22,050 played it ~8% slow and low).
    Loaded lazily the first time it speaks (~0.6 s), kept warm, unloaded
    when idle (`idle_unload`, never under an open synthesiser); every run
    has its own `RunOptions`, so `cancel` (a stop, a new answer, a dictation
    starting: `Speaker::stop` → `Engine::cancel`) ends a sentence being made
    within ~10 ms. 54 voices (`VOICES`, `voices/<id>.bin`: 510×256 styles, the
    row for the call's token count; grouped by accent and gender, af_bella
    the default); the phonemes go through Kokoro's own symbol table
    (`VOCAB`, `tokenize`), ≤ 510 tokens a call (`chunk`, cut at a space),
    Kokoro's padding trimmed and a 180 ms pause added (`trim_and_pad`).
    Test mode: silence once installed; a stub build outside test mode has
    none (`in_build`, `NOT_IN_BUILD`) and Windows voices speak. `#[ignore]`d
    `checks::kokoro_on_this_pc` (engines build; `YAP_TTS_MODELS`,
    `YAP_TTS_SAMPLES`): speed at 1/2/4 threads, a cancel, and WAVs of the
    research's answer and a names text with both G2P ways, raw and tidied
    (measured 2026-10-18: §"Current status").
  - **`tts/g2p.rs`** — text → Kokoro's phonemes, **dictionary first**: the
    misaki lexicons Kokoro was trained on (US/GB, gold then silver), looked
    up with misaki's `en.Lexicon` rules ported (no spaCy tagger: capitals,
    `'s`/`-s`/`-ed`/`-ing` from the stem, short unknown all-caps words
    spelled out, "the"/"to"/"a"/"an"/"in"/"am" by the next sound, hyphened
    parts, leftover digits as words, US flaps as "T"); only words neither
    lexicon has (names: "Priya", "Siobhan") go to **eSpeak NG as a separate
    program** (`Espeak::ipa`: `espeak-ng.exe -q --ipa --tie=^ -b 1`, one line
    in → one line out, `ESPEAK_DATA_PATH`, hidden window, 5 s limit; never
    linked: GPL-3.0) in one call per sentence, its IPA mapped to misaki's
    symbols (`espeak_english`, misaki's E2M, its dead GB "e^ə" rule fixed);
    no eSpeak → spelled out. Kokoro's es/fr/hi/it/pt-BR voices read their
    text all through eSpeak (`Phonemizer::Espeak`, misaki's `EspeakG2P`
    map; punctuation kept by phonemizing between it); ja/zh voices read
    English. `EspeakEnglish` (all of it through eSpeak, Voice Mirror's way)
    only if a lexicon won't read, and for the comparison. Why dictionary
    first: it's what Kokoro was trained on, and eSpeak runs only for the
    odd name (both scored alike on Parakeet).
  - **`tts/registry.rs`** — Kokoro's download into `<data>/models/tts/`
    (debug: `YAP_TTS_MODELS`): `kokoro-v1.0/` (model.onnx 325.5 MB + 54
    voices from **Hugging Face onnx-community/Kokoro-82M-v1.0-ONNX @
    1939ad2a**, the misaki lexicons + LICENSE from **hexgrad/misaki @
    fba12365**, NOTICE.txt) and `espeak-ng-1.52.0/` (eSpeak NG's own 1.52.0
    **MSI**, unpacked with `msiexec /a` — an administrative install, files
    only, nothing registered, no admin rights; only the exe, the DLL and the
    data of the languages Kokoro reads kept, 2.2 MB — + `COPYING` (GPL-3.0)
    from its repo at the tag + NOTICE.txt with the source link): ~379 MB
    down, ~352 MB on disk, every file pinned by size + SHA-256 (the
    diarization registry's `download`/`verify`, resumable), staged in
    `<folder>.installing/` then renamed; a file already installed at its
    hash is kept (hard link); a failure leaves no half folder. Test runs
    only fetch from 127.0.0.1 (`YAP_TTS_SOURCES` stand-ins: `{package:
    {dest: {url, sha256, size}}}`; a debug build takes a zip for the MSI).
    `#[ignore]`d `real_download_from_the_pinned_sources`
    (`YAP_TTS_DOWNLOAD_DIR`).
