# transcribe-rs 0.3.11, as Yap builds it

A copy of [transcribe-rs](https://github.com/cjpais/transcribe-rs) 0.3.11
(MIT, `LICENSE`) from crates.io, used through `[patch.crates-io]` in
`src-tauri/Cargo.toml`. Only `Cargo.toml`, `build.rs`, `README.md`, `LICENSE`
and `src/` are kept; the `[[example]]` and `[[test]]` targets were taken out of
`Cargo.toml` because their files aren't here. Everything else is unchanged
except the two DirectML fixes below (2026-10-22), measured with `stt_speed.rs`'s
`--ignored` `profile_parakeet` and `bench_this_pc` on an RTX 5070 Ti; threads
and the DirectML adapter (2026-10-24); and Whisper's language probabilities
(2026-10-25).

## What changed

`src/onnx/session.rs`
- `build_session_on(…, cpu_only)`: `build_session` with an option to put a
  session on the CPU alone (only the CPU execution provider, parallel
  execution as usual) whatever the global accelerator.
- `create_cpu_session(path)` and `directml_active()` (the global accelerator is
  DirectML and the `ort-directml` feature is on).

`src/onnx/parakeet/mod.rs` (only when `directml_active()` at load)
1. **The decoder/joiner and the preprocessor load on the CPU**
   (`create_cpu_session`); the encoder stays on DirectML. The TDT decode loop
   runs `decoder_joint` once per encoder frame and once per emitted token
   (~210 runs for 11 s of speech, ~665 for 36 s); on DirectML each run was a
   GPU round trip of ~1.4 ms against ~0.2 ms on the CPU (300 ms vs 47 ms for
   11 s of speech). The preprocessor costs the same on either (3–33 ms) but
   its first DirectML run took ~150 ms.
2. **The encoder always gets the input length it planned for.** DirectML ran
   the encoder in ~55 ms (11 s of speech) at the length of its first run, and
   in ~350 ms at any other length, repeated or not — so every dictation (each
   a new length) paid ~300 ms. The waveform is now zero-padded to
   `DIRECTML_WAVEFORM` (30 s plus the 250 ms of leading silence; longer audio
   goes as it is) with `waveforms_lens` still the real length, so the
   preprocessor and the encoder mask the padding and the decode loop stops at
   the real `encoded_lengths`: the same tokens, checked against the CPU on the
   bundled clips. `load` transcribes one second of silence so the encoder plans
   for that length at load, not in the first dictation (~0.5 s).

The CPU path (`CpuOnly`) and every other model and accelerator are untouched.

## Whisper's language probabilities (2026-10-25)

`src/whisper_cpp/mod.rs`
- `WhisperEngine::detect_language(samples, n_threads)`: whisper.cpp's own
  language detection (`pcm_to_mel` then `whisper_lang_auto_detect_with_state`
  at offset 0, on the loaded backend) — exactly what `full()` runs on its
  first 30 s when it's given no language — returned as `(code, probability)`
  pairs, the most likely first. `Err` for an English-only (`.en`) model.
- `WhisperEngine::is_multilingual()`.

Yap's "My languages" (`dictation/languages.rs`) calls it before a
dictation's transcription and then passes the language it picked (the most
likely of the person's languages) to `transcribe_with`, so `full()` doesn't
detect a second time: the same work as before, plus the probabilities.

## Threads and the DirectML adapter (2026-10-24)

`src/accel.rs`
- `set_ort_intra_threads(n)` / `get_ort_intra_threads()`: an intra-op thread
  count for every ORT session built afterwards that doesn't set its own (`0`,
  the default: ONNX Runtime's own choice, one thread per physical core).
- `set_ort_directml_device(i)` / `get_ort_directml_device()`: the DXGI adapter
  index DirectML sessions use (`-1`, the default: DirectML's default device,
  DXGI's first adapter).

`src/onnx/session.rs`: `build_session_on` reads both (`with_intra_threads`
when the caller gave no count; `DirectML::with_device_id` when one is set).

Yap sets the adapter at start to the graphics card with the most memory of
its own when DXGI doesn't list it first (a hybrid laptop lists its
integrated GPU first; `gpu::speech_adapter`), and `yap.exe bench` uses both
to emulate a smaller PC (4 or 2 threads) and to time an integrated GPU or
Windows' software renderer (`bench.rs`). Defaults leave everything as it was.
