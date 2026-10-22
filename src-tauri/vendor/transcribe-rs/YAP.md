# transcribe-rs 0.3.11, as Yap builds it

A copy of [transcribe-rs](https://github.com/cjpais/transcribe-rs) 0.3.11
(MIT, `LICENSE`) from crates.io, used through `[patch.crates-io]` in
`src-tauri/Cargo.toml`. Only `Cargo.toml`, `build.rs`, `README.md`, `LICENSE`
and `src/` are kept; the `[[example]]` and `[[test]]` targets were taken out of
`Cargo.toml` because their files aren't here. Everything else is unchanged
except the two DirectML fixes below (2026-10-22), measured with `stt_speed.rs`'s
`--ignored` `profile_parakeet` and `bench_this_pc` on an RTX 5070 Ti.

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
