# Spoken replies

Reading answers aloud: Kokoro and Windows' voices on this PC, ElevenLabs on the person's own key, or OpenAI, Gemini and Groq on the key Language Models already has for them; text prepared for speech, the speaker queue and when Yap talks; other apps turned down while Yap speaks; and **Read aloud, anywhere** — any selected text read in that voice with a karaoke reading card on the Yap bar. ("§Current status" below means [status.md](./status.md).)

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`voice/tts.rs`** — **spoken replies** (Settings → Voice Assistant, Chat's Voice
  replies toggle, ▶ on answers). **Who speaks** (`config.tts_provider`,
  `voicing(cfg)` → `Voicing {engine, voice, cloud}`): "local" (the
  default — the engines below, `chosen(cfg)`) or a **cloud voice** —
  ElevenLabs (`voice/tts/elevenlabs.rs`, below) on its own key
  (`tts_cloud_keys`), or OpenAI, Gemini or Groq (`voice/tts/hosted.rs`,
  below, 2026-10-31) on the key Language Models has for that provider
  (`provider_key` → `ai_model::provider_key`: Chat's own, else the shared
  one) — only when it's picked **and** has a key; without one the local
  voice speaks. A cloud
  voicing carries a `CloudPlan`: the local voice that can stand in
  (`local_voice`: Kokoro installed, or Windows voices chosen — not Windows
  voices as a stand-in for an undownloaded Kokoro) and the key/choice
  fingerprints. `speak(text, id)` is the way in for spoken replies — an
  answer's markdown or any plain text; `read(track, from, id, rate)` for
  reading aloud (`voice/read_aloud.rs`, below). **Engines** (`Engine` → `Synth`, text → `Pcm`; `Synth::stream`
  hands audio on as it's made — a cloud voice's first piece plays before
  the sentence is all made; the default makes the whole sentence;
  `prepare` (Kokoro and ElevenLabs join sentences under 3 words to the
  next), `cancel`): **Kokoro** (`voice/tts/kokoro.rs`, below; the default) and
  **Windows voices** (`WindowsVoices`: WinRT `SpeechSynthesizer`,
  `SynthesizeTextToStreamAsync` → WAV → `parse_wav`; rate via
  `SpeakingRate`, minimal appended silence; `IncludeWordBoundaryMetadata`
  on, so each sentence's stream carries a `SpeechCue` per word —
  `StartPositionInInput` (UTF-16) and `StartTime` — handed to the speaker
  by `Synth::marks`). `chosen(cfg)`: Kokoro with
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
  **`voice/tts/normalize.rs`** (`for_speech`, every engine, UK style,
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
  plays; `stop()` empties the player and drops the queue at once; a new
  utterance replaces the old; it checks before every sentence and while one
  plays that no dictation records (`pipeline::dictation_state`), refuses to
  start over one, and refuses while a meeting records (the loopback would
  put Yap in "Them"; `meeting::start` stops it). Each sentence is
  `stream`ed into the player piece by piece (each piece checked: stopped, or
  a dictation started → the stream ends). **A streamed answer's job**
  (`Job.feed`, `speak_streamed` → a `Feed`; the Voice Assistant,
  2026-10-22): it starts with no sentences and is "speaking" at once; the
  worker says each sentence pushed (`Feed::push`, prepared per batch),
  waits while the feed is open (15 ms polls; ≤ 3 min without a sentence,
  `FEED_PATIENCE`), and ends after the last once it's closed
  (`Feed::close`); a stop, a newer utterance or a dictation ends it and
  `push` says so (`false`). `ready_sentences(md, n, done)`: the sentences
  of a streamed answer's first block the voice can have now — never the
  one still being written. Test mode records the whole streamed utterance
  when its feed closes (`streamed: true`). **Somewhere else than this PC's
  speakers** (Yap Link, 2026-10-31, [link.md](./link.md)):
  `speak_streamed_to(message_id, voicing, opener)` is the same streamed job
  with the caller's output — the phone's `PhonePlayback` (`link/voice.rs`:
  each piece brought to mono 24 kHz and sent down its socket; "done" on a
  clock of the audio's length; `Opened.keep` a guard whose drop says
  `speak_end`) — and `phone_voicing(cfg)` its voice: the one Settings picked
  with "Speak replies" on, else Kokoro if downloaded, else Windows voices
  (never a cloud voice not switched on). One speaker still: it stops what this
  PC was saying, and a dictation or a meeting stops it. **A read-aloud job** (`Job.track`
  + `first`: the `Track`'s shown sentences from there): each sentence is
  said as `said_text` writes it (`inline_text` + `normalize::for_speech`),
  made only once the one before it **plays** (`wait_turn`: one sentence
  ahead of the voice, never the whole text), its pieces appended
  **tagged** (`Playback::append_tagged`, `Tag {sentence, offset}`) — rodio
  through a `Tagged` source that moves an atomic position on with every
  sample the output takes (frozen while paused), `SimPlayback` on its
  clock, which stands still while paused — and, once made, its words'
  start times kept on the track (`Track::record` → `Made {starts, timing,
  engine}`, the 32 sentences round the voice): the engine's own
  (`Synth::marks`), else from its audio (`marks::from_audio`), mapped back
  from the said words to the shown ones (`marks::align`). `position()`,
  `pause()`, `resume()` (the playback held where it is: rodio's
  `Player::pause`). **A cloud voice failing**
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
  meeting)` (chat_send's `speak`; what's said, `spokenText`, as much as
  "How much to read aloud" says, `assistant::spoken_in_chat`),
  `sends_by_itself(speak_replies,
  hands_free, said_enter)` — the Voice Assistant's answers ignore both
  (`assistant::reply`), and every state change is passed to
  `assistant::on_tts_state` (its answer ended). Commands `tts_voices` (`{provider,
  providers: [{id, name, keysPage, hosted, connected, defaultVoice?,
  defaultModel?}]` — `providers_for`: every provider, OpenAI, Gemini and
  Groq `connected` once their key's there; Settings filters on its own
  copy's keys too —, engine, active,
  engines, kokoro: {voices, defaultVoice}, kokoroStatus, windows: {voices,
  defaultVoice, error}}`), the cloud voice's `tts_cloud_voices` /
  `tts_cloud_models` / `tts_cloud_usage` / `tts_cloud_preview {provider,
  voice, apiKey?, model?, rate?}` (the key Settings has now, else the saved
  one; for OpenAI, Gemini and Groq their own lists — no request —, no usage
  line, and ▶ says the preview sentence through the provider), `tts_speak {text, messageId}`, `tts_preview
  {voice, rate, engine?}` (Kokoro not installed → "Download Kokoro to hear
  its voices"), `tts_stop`, `tts_state`, `tts_kokoro_status` /
  `tts_kokoro_download` (`tts-download-progress {percent, downloadedBytes,
  totalBytes, bytesPerSec, etaSecs, cancelId: "kokoro"}`, then
  `tts-kokoro-changed`; Cancel = `download_cancel {id: "kokoro"}`, the
  shared helper's: app-shell.md `shell/download.rs`) / `tts_kokoro_delete` (stops
  speaking, unloads), `voice_replies_set {on}` (Chat's toggle: saved in one step, `settings::change_config`, applied like
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
  also forgets the cloud's last failure; a read-aloud records all its
  said sentences when it starts). An `#[ignore]`d
  `windows_voices_on_this_pc` lists the voices and synthesises a sentence
  to a WAV in a temp folder (never played); `windows_word_cues_on_this_pc`
  makes a sentence into memory and prints each word's cue beside the
  estimate from its audio alone (measured 2026-10-22, the default voice:
  cues at 101 / 436 / 761 / 1386 … ms, the comma's pause pinned within
  4 ms; `voice/tts/marks.rs` for the numbers); `first_audio_latency`
  (`--features engines`) times `speak` → first piece at the output for
  Kokoro and for ElevenLabs' path against a local stand-in
  (`YAP_TTS_STANDIN_MS`).
  - **`voice/tts/cloud.rs`** — what any cloud voice shares: `PROVIDERS`
    ("local" "On this PC", "elevenlabs" "ElevenLabs", "openai" "OpenAI",
    "gemini" "Gemini", "groq" "Groq"; `is_hosted`: the three on a Language
    Models key, whose notices point there — "Check your OpenAI API key in
    Settings → Language Models.", "Your Gemini account is out of credit, or
    at its limit for today."), `Failure` (BadKey,
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
    **`cloud/stream.rs`** (2026-10-31, out of `elevenlabs.rs`): what every
    cloud voice's request shares — `Key` (`Debug` "[redacted]", only ever
    in a header; `fingerprint`), `tidy` (the key cut from anything said
    back), `API_HTTP` (4 s to connect, no redirects), the `Decode`rs (`RawPcm`
    — 16-bit LE mono, a sample split across pieces kept, `S16Le` —,
    `WavOrRaw` — a WAV read as its bytes come, its header's rate and
    channels, a `data` chunk of unknown length to the end, raw PCM when
    there's no header —, `SseLines` — server-sent events' `data:` lines, a
    line split across pieces kept; each says the tokens a stream ended
    with, `usage`), `cancel` (one counter and `Notify` for every cloud
    voice: a stop ends any sentence being fetched), `ask` (send; a stop
    first → nothing; an error answer → `classify`) and `pump` (a `Turn`:
    the audio handed on ≥ 100 ms at a time, then the turn's quiet; no audio
    within its first-audio limit, or a gap over its limit → `TimedOut`;
    nothing at all → "sent no audio"). Unit tests: a WAV and events in any
    split, a bad header said, the key never shown.
  - **`voice/tts/hosted.rs`** — **OpenAI, Gemini and Groq as voices**
    (2026-10-31), on the key Language Models has for each (`tts::provider_key`
    → `ai_model::provider_key`: Chat's own for it, else the shared one —
    no key of its own, no new field). One engine, `Hosted {svc, key, model,
    keep_health}`, over a `Service` each provider fills in (`openai.rs`,
    `gemini.rs`, `groq.rs`: its request — pure —, its decoder by the
    answer's `Content-Type`, `classify`, its `voices()` and `models()` —
    Yap's lists, nothing to fetch —, `max_chars`, `limits`). A sentence:
    `speak` → `split(text, max_chars)` (Groq's 200: cut after a comma,
    semicolon or colon in reach, else at a space; a word longer than that
    cut, never lost), one request a piece (`ask` + `pump`, the 150 ms pause
    after the last only), each counted in the AI usage tracker
    (`usage::record_speech`: "Spoken replies", its tokens or characters),
    the network receipt "Voice · OpenAI" (`Purpose::Voice`). A voice or
    model Yap doesn't know → the provider's default (`voice_or_default`,
    `model_or_default`); `prepare` joins sentences under 3 words to the
    next (as Kokoro and ElevenLabs); `cancel` → `cloud::stream::cancel`.
    `Health` and the local stand-in as for ElevenLabs (`choice_fingerprint`
    of provider + key + voice + model). No word timing from any of the
    three: a read-aloud's words come from their audio (`marks::from_audio`).
    `classify_openai_shape` (OpenAI's and Groq's `{error: {message, type,
    code}}`: `invalid_api_key` / 401 → BadKey, `insufficient_quota` →
    Quota, `rate_limit_exceeded` / 429 / 498 → RateLimited,
    `model_not_found` / `model_decommissioned` / 404 → ModelGone, 403 →
    NotAllowed, 5xx → ServiceDown, else their words, tidied — Groq's
    "accept the model's terms" among them). Settings: `voices_of`,
    `models_of`, `sample_engine` (▶: the preview sentence through the
    provider, its error shown, not kept). Debug-only
    `YAP_OPENAI_TTS_URL` / `YAP_GEMINI_TTS_URL` / `YAP_GROQ_TTS_URL` name a
    test run's stand-in (`desktop/e2e/support/fake-cloud-tts.js`); without one a
    test run never reaches the internet. Unit tests against the in-process
    stand-in (`elevenlabs::standin`'s `Reply::Body`: any content type,
    chunked in odd pieces), and the speaker's
    `a_hosted_voice_streams_then_hands_over_to_the_local_voice_when_it_fails`.
    A real check on the maintainer's own key (never in a normal run; a
    fraction of a penny): the `#[ignore]`d
    `hosted::tests::a_real_sentence_on_your_key` (`YAP_REAL_TTS_PROVIDER`
    openai | gemini | groq, `YAP_REAL_TTS_KEY`, optional `_VOICE` /
    `_MODEL`): one sentence into `%TEMP%/yap-tts-check/<provider>-voice.wav`,
    nothing played, its first-audio time printed.
    - **`voice/tts/openai.rs`** (API as of 31 Oct 2026, developers.openai.com
      speech reference + text-to-speech guide): `POST /v1/audio/speech`,
      `Authorization: Bearer`, `{model, input, voice, response_format:
      "pcm", stream_format: "sse", speed? (0.25–4, only when not 1×)}`;
      server-sent `speech.audio.delta {audio: base64 PCM, 24 kHz 16-bit
      mono}` then `speech.audio.done {usage {input_tokens, output_tokens}}`
      (`Events`; an answer that isn't events → raw PCM). Models
      `gpt-4o-mini-tts` (default) and `gpt-4o-mini-tts-2025-12-15`; ⚠ OpenAI
      retires `gpt-4o-mini-tts`, `tts-1` and `tts-1-hd` on **6 January
      2027** (its successor is Realtime-only): after that a request fails
      "model not available" and the local voice reads, said once. Voices:
      marin and cedar ("Best quality", OpenAI's recommendation), alloy,
      ash, ballad, coral, echo, fable, nova, onyx, sage, shimmer, verse;
      default marin. 6 s to the first audio. $0.60 text in / $12 audio out
      a million tokens.
    - **`voice/tts/gemini.rs`** (API as of 31 Oct 2026, ai.google.dev speech
      generation + streaming + pricing): the **Interactions API**, `POST
      /v1beta/interactions`, `x-goog-api-key`, `{model, input: [{type:
      "user_input", content: [{type: "text", text, annotations?: [{type:
      "speech_metadata", style}]}]}], response_format: {type: "audio"},
      generation_config: {speech_config: [{voice}]}, stream: true}`;
      server-sent `step.delta {delta: {type: "audio", data: base64}}`
      (headerless 24 kHz 16-bit mono; a WAV header read past),
      `interaction.completed {interaction: {usage: {total_input_tokens,
      total_output_tokens}}}`, `error {error: {message}}`, `[DONE]`. No
      speed setting: Yap's speed asked for in the style (`style_for`: "Speak
      quickly" ≥ 1.45×, "Speak at a brisk pace" ≥ 1.15×, "Speak at an
      unhurried pace" ≤ 0.85×, "Speak slowly" ≤ 0.65×). Models
      `gemini-3.8-flash-lite-tts` (default: "fast, cost-efficient") and
      `gemini-3.8-flash-tts`; 30 voices with Google's one-word descriptions,
      default Kore. Google's errors (`{error: {code, message, status,
      details}}`): "API key not valid" (a 400) / `API_KEY_*` → BadKey,
      PERMISSION_DENIED → NotAllowed, RESOURCE_EXHAUSTED → RateLimited, or
      Quota when it's a day's (`PerDay`) or billing, NOT_FOUND → ModelGone.
      12 s to the first audio. Paid tier: Flash-Lite TTS $0.50 / $6, Flash
      TTS $0.50 / $9 a million tokens to 31 Dec 2026, then double; 25 audio
      tokens a second; the free tier costs nothing.
    - **`voice/tts/groq.rs`** (API as of 31 Oct 2026, console.groq.com text
      to speech + Orpheus): `POST /openai/v1/audio/speech`, `Authorization:
      Bearer`, `{model, input (≤ 200 characters), voice, response_format:
      "wav"}` — WAV only, not streamed (read as it comes, `WavOrRaw`). Model
      `canopylabs/orpheus-v1-english` (its Saudi Arabic one isn't offered:
      Yap's text for the voice is English-shaped); no speed setting
      (Settings says it speaks at its own pace). Voices hannah (default),
      autumn, diana (female), austin, daniel, troy (male). $22 a million
      characters (`prices::char_cost_micros`).
  - **`voice/tts/elevenlabs.rs`** — **ElevenLabs**, on the person's own key (API
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
    ServiceDown. **Word timing** (reading aloud: the engine built
    `.timed(true)` by `voicing_for(cfg, true)`): the same request to
    `POST /v1/text-to-speech/{voice}/stream/with-timestamps?output_format=
    pcm_24000` (`timed_request`; the docs, 22 Oct 2026: a stream of JSON,
    one object a line — `audio_base64` + `alignment {characters,
    character_start_times_seconds, character_end_times_seconds}` for the
    text as sent), parsed as it comes by `Timestamped` (a line split
    across pieces kept, the base64 PCM through the same `S16Le`): the
    audio handed on as before, every character's start kept for
    `Synth::marks`; times are on the sentence's clock (ElevenLabs' own
    example adds the pieces' lists up as they are) — a piece whose times
    start again near zero is put after the audio before it.
    Debug-only `YAP_ELEVENLABS_URL` names a test run's
    stand-in (`desktop/e2e/support/fake-elevenlabs.js`, which serves both
    endpoints); without it a test run
    never reaches the internet. Unit tests run against an in-process
    stand-in (`standin`: chunked PCM in odd pieces, timed JSON lines on
    either clock, errors, silence).
  - **`voice/tts/kokoro.rs`** — **Kokoro-82M v1.0** (hexgrad, Apache-2.0), the
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
    within ~10 ms. 54 voices downloaded (`VOICES`, `voices/<id>.bin`: 510×256 styles, the
    row for the call's token count; grouped by accent and gender, af_bella
    the default), **41 offered** (2026-10-25, `listed`): its 5 Japanese and
    8 Mandarin voices aren't — Yap's phonemes for them are English
    (misaki's English lexicon, else eSpeak's English), while Kokoro learnt
    misaki's Japanese (pyopenjtalk) and Mandarin (pypinyin + jieba, tones)
    phonemes, neither runnable here without a dictionary and tokenizer per
    language (tens of MB), and eSpeak NG's `ja` / `cmn` IPA is nothing it
    saw: they read text as heavily accented English. One already chosen
    speaks as af_bella (`spoken_voice`), its saved setting untouched (no
    migration: an older Yap sharing the folder still has it), and their
    files still download (that older Yap may use one); the phonemes go through Kokoro's own symbol table
    (`VOCAB`, `tokenize`), ≤ 510 tokens a call (`chunk`, cut at a space),
    Kokoro's padding trimmed and a 180 ms pause added (`trim_and_pad`).
    Test mode: silence once installed; a stub build outside test mode has
    none (`in_build`, `NOT_IN_BUILD`) and Windows voices speak. `#[ignore]`d
    `checks::kokoro_on_this_pc` (engines build; `YAP_TTS_MODELS`,
    `YAP_TTS_SAMPLES`): speed at 1/2/4 threads, a cancel, and WAVs of the
    research's answer and a names text with both G2P ways, raw and tidied
    (measured 2026-10-18: §"Current status").
  - **`voice/tts/g2p.rs`** — text → Kokoro's phonemes, **dictionary first**: the
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
    map; punctuation kept by phonemizing between it); ja/zh voices would read
    English (not offered, above). `EspeakEnglish` (all of it through eSpeak, Voice Mirror's way)
    only if a lexicon won't read, and for the comparison. Why dictionary
    first: it's what Kokoro was trained on, and eSpeak runs only for the
    odd name (both scored alike on Parakeet).
  - **`voice/tts/marks.rs`** — **word timing** for the reading card's karaoke,
    pure and unit-tested. `words` (a sentence's words as char spans, the
    punctuation at their edges left out). Per engine, best first (`Timing`,
    on the card as `data-timing` and the chip's tooltip): **Engine** —
    Windows voices' word cues, ElevenLabs' character alignment
    (`from_chars`: a word starts with its first timed character, an untimed
    one between its neighbours; `None` if under half the words are timed);
    **Audio** — **Kokoro**: its onnx-community export has one output,
    `waveform` (checked on this PC's model: inputs `input_ids`, `style`,
    `speed`; no durations), so `from_audio` finds the speech in the
    sentence's own audio (`shape`: 10 ms frames, quiet under 8 % of the
    90th-percentile frame and −48 dBFS, pauses ≥ 120 ms), spreads the words
    over it by weight (syllables + 0.2 × letters, little words like "and"
    × 0.6, a comma's pause 2.5, a full stop's 4 — fitted to Windows' cues)
    and pins each pause in the audio to the punctuation whose estimated
    place is nearest (`pinned`), the words between two pins spread between
    them, each but a segment's first lit 65 ms earlier (`SPREAD_LEAD`: the
    spread lands late by that much). Measured 2026-10-22 (`kokoro::real::
    checks::word_timing_against_parakeet` / `windows_cues_against_parakeet`,
    `--features engines`, nothing played): Parakeet's word times sit −21 ms
    (median; |59| ms) from Windows' real cues, a fair yardstick; against it,
    Kokoro's audio estimate (3 voices × 4 sentences, 171 words) is off by
    a median 100 ms (+36 signed), 90th 297 ms, 71 % within 150 ms — the
    plain spread over the length 154 ms, 49 %; on Windows' voice the audio
    estimate is +10 ms (median |70|, 90th 184) from the real cues; **Estimate** — no speech in the audio (test mode's silent
    stand-in): the same spread over its whole length (`estimate`). What's
    said isn't always what's shown ("£12,400" → "twelve thousand four
    hundred pounds"): `align` pairs the words the same on both sides (LCS,
    lower case, letters and digits) and gives the said words between two
    pairs to the shown words between them; `shown_starts` → each shown
    word's start, forwards in time; `word_at(starts, t)` the word being
    said. `utf16_to_char` for Windows' cues.
  - **`voice/read_aloud.rs`** — **Read aloud, anywhere** (2026-10-22; the
    maintainer asked for reading help, dyslexia in mind; Wispr Flow has
    none). Ways in: the **Read aloud shortcut** (`config.read_hotkey`,
    unbound by default; `input_hook` binding `read-key-pressed`, press
    only, never clashing — `configure_read_from`), **🔊 on the Yap bar**
    (`bar_action "read-aloud"`; ■ `read-stop` while reading), the **tray's
    "Read selection aloud"** ("Stop reading aloud" while it reads; the app
    window on top found as "Paste last transcript" finds it and brought
    back first), and in Yap: a **note's Read aloud** (its button beside
    Export, its row menu), **Chat's and the Ask bars' ▶ / Read aloud on
    answers** (`speech.svelte.js` → `read_aloud_text`: the same card, the
    answer's id kept so ▶ shows ■), and the shortcut pressed in a Yap
    window (the in-page fallback: `readPageSelection`, the page's own
    selection). **The selection** (`read_selection_now`, a thread of its
    own): the app in front (`current_foreground`), `selection::
    read_selected(target, copy_fallback)` with Ctrl+C allowed only outside
    a terminal (`app_kind::of_window` — the raw kind, whatever "Safer
    pasting" says); none, or the same selection again while reading
    (a fingerprint, never the text) → stop (the shortcut is its own Stop);
    nothing and not reading → a note on the card, "Nothing selected" with
    the shortcut named (`{key}`). Test mode reads no other app's selection
    (robot mode: only the test's windows). **The text** (`sentences_for`,
    pure): markdown (notes, answers) through `speakable_blocks`; plain text
    (a selection) through `plain_blocks` — paragraphs, a list item or a line
    under 40 characters its own block, a paragraph's wrapped lines joined
    (a PDF's, an email's) and a hyphen at a wrapped line's end mended —
    then `sentences`; up to `MAX_CHARS` (200,000; the card says when it was
    cut); a run with no letter or digit isn't read; then `prepare_for_reading`
    (the voice's own joining of short sentences). **Reading** (`start`):
    `tts::read` from sentence 0 (a meeting or a call on → a note, "Not
    now"), the session's `restarting` flag held over every (re)start so the
    old voice's end isn't taken for this one's, a **ticker** (40 ms) that
    follows `tts::position` and the track's `Made` starts: a new sentence →
    `yap-read` (the whole card), a new word → `yap-read-word {session,
    index, word}`. Controls (`read_aloud_action`): pause / resume / toggle
    (the playback held, `tts::pause`), next / back (back: this sentence's
    start once it's 1.5 s in, else the one before), faster / slower
    (`SPEEDS` 0.5–2×; restarts the sentence at the new speed and keeps it
    as `tts_rate`, `yap-voice-rate` for Settings' copy), stop / dismiss.
    The voice ending (to the end, Chat's ■, a dictation, a meeting) ends
    the reading (`on_tts_state`); an error is said on the card. `View`
    (`read_aloud_state`): `phase` reading | paused | note | idle, the
    sentence as `parts` (words numbered), `word`, `index`/`count`,
    `rate`, `timing`, `engine`, `note`, `cut`, `made` (sentences made so
    far: never far ahead), `hotkey`. `bar::set_reading` keeps the bar up
    while the card is (bar on or off), with Esc watched. Logs: lengths,
    counts, `via` and the outcome only (`privacy/log_privacy.rs`); reading with
    ElevenLabs sends the text there, as Settings' voice line already says.
    Debug-only `realtest_read_selection {hwnd}` (robot mode) runs the
    shortcut's code and returns `Picked {via, chars, terminal,
    copyAllowed, outcome, text}`. Tests: unit (`plain_blocks`,
    `sentences_for`, `parts`, speeds, ⏮), the speaker's (`a_read_aloud_is_
    made_a_sentence_ahead…`, `a_held_read_aloud…`, `an_engines_own_word_
    timing…`), `desktop/e2e/read-aloud.spec.js` (the card, karaoke, ⏸ ⏭ ⏮, speed,
    Stop, Esc, Dark + OpenDyslexic, a 30,000-character text made a sentence
    ahead, nothing selected and 🔊, a note, Chat's id, the in-page shortcut,
    Settings) and `desktop/e2e-real/read-aloud.real.js` (the test pad's text box
    and rich box, Notepad, an Edge page's own text, a terminal never by
    Ctrl+C).
  - **`voice/duck.rs`** — **other apps turned down while Yap speaks**
    (2026-10-31; Settings → Voice Assistant → Voice: "Turn other apps down
    while Yap speaks", `config.duck_other_apps`, on by default). While Yap's
    voice plays on this PC — spoken replies, the Voice Assistant, Read
    aloud, ▶ — every other app's audio session goes to 30 % of its own level
    (`other_apps::VOICE_SHARE`), and back to exactly that level after. Only
    the output `playback.rs`'s rodio opener makes holds a `Duck`
    (`RodioPlayback.duck`): Yap Link's `PhonePlayback` never does, so an
    answer said on a phone turns nothing down here. A `Duck` holds from the
    first audio appended (a streamed answer waiting on its first sentence
    has nothing to hear yet), lets go on ⏸ (and holds again on ⏯ while
    there's audio), on a stop and when the output closes; `HOLDING` counts
    the outputs holding, and its 0 → 1 and 1 → 0 kick `settle` on a thread
    of its own (the COM work never on the speaker's), the letting go
    `LET_GO_AFTER` (600 ms) later — so answers in a row, or a new answer
    replacing one, never bounce the music, and a dictation started
    mid-speech finds the sessions still down. `settle` (under
    `other_apps::ledger`, so any order of kicks ends right; `next_step`,
    pure): speaking and not down → `lower(Voice)` once per stretch of speech
    (a level the person moves meanwhile stays put); not speaking, or the
    setting off → if a dictation is recording with "Silence other apps" on
    (`mute::takes_over`), `hand_over(Voice → Dictation)` — the sessions stay
    at 30 % through its start sound, go silent after it, and come back to
    their level from before Yap spoke after its stop sound — else
    `release(Voice)`. The sessions, the rules (never Yap's own, a call
    app's, system sounds, a meeting's browser), the exact restore, the
    crash file and quitting's restore are `dictation/other_apps.rs`'s,
    shared with a dictation's silence. `sync(cfg)` on every save and at
    start (switched off mid-answer: back at once). Test runs touch nothing
    (`realtest::other_apps` = Nobody; `cargo test` never kicks). Unit tests:
    `next_step`, a `Duck` held and let go once; the ledger's arithmetic and
    hand-over in `other_apps.rs`'s. E2E: `desktop/e2e/voice-cloud.spec.js` (the
    switch, on by default, saved).
  - **`voice/tts/registry.rs`** — Kokoro's download into `<data>/models/tts/`
    (debug: `YAP_TTS_MODELS`): `kokoro-v1.0/` (model.onnx 325.5 MB + 54
    voices from **Hugging Face onnx-community/Kokoro-82M-v1.0-ONNX @
    1939ad2a**, the misaki lexicons + LICENSE from **hexgrad/misaki @
    fba12365**, NOTICE.txt) and `espeak-ng-1.52.0/` (eSpeak NG's own 1.52.0
    **MSI**, unpacked with `msiexec /a` — an administrative install, files
    only, nothing registered, no admin rights; only the exe, the DLL and the
    data of the languages Kokoro reads kept, 2.2 MB — + `COPYING` (GPL-3.0)
    from its repo at the tag + NOTICE.txt with the source link): ~379 MB
    down, ~352 MB on disk, every file pinned by size + SHA-256 (the
    diarization registry's `download`/`verify` — the shared, resumable,
    cancellable helper `shell/download.rs` since 2026-10-25), staged in
    `<folder>.installing/` then renamed; a file already installed at its
    hash is kept (hard link); a failure leaves no half folder. Test runs
    only fetch from 127.0.0.1 (`YAP_TTS_SOURCES` stand-ins: `{package:
    {dest: {url, sha256, size}}}`; a debug build takes a zip for the MSI).
    `#[ignore]`d `real_download_from_the_pinned_sources`
    (`YAP_TTS_DOWNLOAD_DIR`).
