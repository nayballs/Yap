# How fast is Yap? Benchmarks you can reproduce

Every number here comes from `yap.exe bench`, a command in every Yap build
(release builds too). It times what you wait for: the speech model turning
your words into text, the AI cleanup after it, and Yap starting. Run it on
your own PC and send us the result: [share your numbers](#share-your-numbers).

**Be careful what you read into these numbers.** We have one test PC, a fast
desktop. The "laptop" rows are an **emulation** on that desktop, not a real
laptop: the same processor held to 4 or 2 threads. A real laptop's cores are
slower than this desktop's, so expect a real 4-thread laptop to be slower
than our 4-thread row. See [how the emulation works](#how-the-emulation-works-and-where-it-falls-short).

## The short version (24 October 2026, Yap 0.1.1)

| | Graphics card | Processor, 16 threads | **4 threads (emulated)** | **2 threads (emulated)** |
|---|---|---|---|---|
| Speech to text, an 11 s dictation (Parakeet V3) | 0.13 s | 0.34 s | 0.38 s | 0.60 s |
| Speech to text, a 3 s dictation | 0.10 s | 0.10 s | 0.13 s | 0.18 s |
| AI cleanup on this PC (Qwen2.5 1.5B) | 0.08 s | 0.9 s | 1.1 s | 1.6 s |
| AI cleanup on this PC (Qwen3.5 4B) | 0.16 s | 2.5 s | 2.8 s | 3.9 s |
| AI cleanup in the cloud (Groq, gpt-oss-20b) | 0.5–0.8 s | same | same | same |

- **Speech to text is fast without a graphics card.** Parakeet V3 runs on
  the processor (int8) in under half a second for a typical dictation on 4
  threads, and in about 0.6 s on 2. Yap picks the processor on a PC
  without a real graphics card (see [what changed](#what-these-numbers-changed)).
- **AI cleanup on the processor costs about a second.** The 1.5B model adds
  about 1.1 s to every dictation on 4 threads of this desktop, more on a
  real laptop. On a PC with fewer than 8 threads, Yap now offers a cloud
  provider first, with both times said, and the private model one click
  away.
- **A weak integrated GPU is the slowest option.** Parakeet on this
  processor's own 2-unit Radeon took about **4 s** for any dictation, ten
  times the processor's time.

## The reference PC

AMD Ryzen 7 9800X3D (8 cores, 16 threads), NVIDIA GeForce RTX 5070 Ti
(16 GB), AMD Radeon integrated graphics (2 compute units, 2 GB carve-out),
64 GB of memory, Windows 11. Yap 0.1.1, a release build with the speech
engines. Each speech number is the median of 3 warm runs. The PC was in
everyday use with other work running, so before each run the bench waited
for the processor to be under 30 % busy. A `*` marks a run where it was
busier than that just before or just after.

## Speech to text

Milliseconds from the end of a dictation to its text (warm median), for
dictations of about 3, 11 and 29 seconds. *Load* is the model loading;
*first* is the first 11 s dictation after it. *WER* is the share of words
wrong against the script the clip reads (most "errors" are a number
written as a word).

| Model | Where | Load | First | 3 s | 11 s | 29 s | WER |
|---|---|---:|---:|---:|---:|---:|---:|
| Parakeet V3 | Graphics card (DirectML) | 2652 | 129 | 104 | 130 | 184 | 1.7 % |
| Parakeet V2 (English) | Graphics card (DirectML) | 3288 | 142 | 108 | 131 | 183 | 1.7 % |
| Whisper Large v3 * | Graphics card (Vulkan) | 1496 | 967 | 499 | 1064 | 3818 | 1.7 % |
| Moonshine Base * | Graphics card (DirectML) | 1088 | 1213 | 160 | 203 | 257 | **100 %** |
| Parakeet V3 | Processor, 16 threads | 788 | 327 | 102 | 340 | 1103 | 2.1 % |
| Parakeet V2 (English) | Processor, 16 threads | 931 | 334 | 109 | 344 | 1163 | 1.7 % |
| Moonshine Base * | Processor, 16 threads | 650 | 379 | 171 | 707 | 2872 | 1.7 % |
| Parakeet V3 | **4 threads (emulated)** | 884 | 395 | 129 | 375 | 1256 | 2.1 % |
| Parakeet V2 (English) | **4 threads (emulated)** | 1130 | 403 | 115 | 390 | 1204 | 1.7 % |
| Moonshine Base | **4 threads (emulated)** | 643 | 368 | 99 | 377 | 1707 | 1.7 % |
| Parakeet V3 | **2 threads (emulated)** | 911 | 609 | 182 | 604 | 1962 | 2.1 % |
| Parakeet V2 (English) | **2 threads (emulated)** | 758 | 546 | 159 | 561 | 1954 | 1.7 % |
| Moonshine Base * | **2 threads (emulated)** | 700 | 494 | 125 | 534 | 2571 | 1.7 % |
| Parakeet V3 | **Integrated GPU + 4 threads (emulated)** | 6099 | 3961 | 3933 | 3957 | 4014 | 2.1 % |
| Parakeet V2 (English) * | **Integrated GPU + 4 threads (emulated)** | 5874 | 3930 | 4017 | 4077 | 4152 | 1.7 % |
| Moonshine Base * | **Integrated GPU + 4 threads (emulated)** | 960 | 1162 | 388 | 414 | 1016 | **100 %** |
| Parakeet V3 * | **No graphics driver + 4 threads (emulated)** | 2620 | 1000 | 966 | 1014 | 1042 | 2.1 % |
| Parakeet V2 (English) | **No graphics driver + 4 threads (emulated)** | 2325 | 1097 | 1035 | 974 | 979 | 1.7 % |
| Moonshine Base * | **No graphics driver + 4 threads (emulated)** | 716 | 417 | 121 | 426 | 1868 | 1.7 % |

Whisper Large v3 on the processor wasn't timed: whisper.cpp needs minutes
per dictation there (`--whisper-cpu` times it anyway).

What the table says:

- **Parakeet is the right model everywhere.** It's the most accurate of
  the quick models and the quickest on the processor at every length.
  Moonshine Base (55 MB) is no quicker on the processor for 11 s, slower
  for long dictations, and scores lower on the Open ASR Leaderboard (this
  clip is clean speech, so every model gets it nearly right). On 2 threads the
  English-only Parakeet V2 is a little quicker than V3 (561 against 604 ms
  at 11 s).
- **Moonshine on a real GPU writes nothing useful** (DirectML on the RTX
  and the Radeon: 100 % of words wrong; on the processor and on Windows'
  software renderer it's fine). Yap's own device check compares each
  device's words, so it never picks a device that gets them wrong.
- **Integrated graphics:** Parakeet's encoder runs at one planned length
  on DirectML (30 s, so every dictation costs the same; it's what makes
  the graphics card quick), and on this 2-unit GPU that costs ~4 s. Bigger
  integrated GPUs (Iris Xe, Radeon 780M) are several times faster, but the
  processor is the safer start. Yap measures both a minute after it starts
  and switches if the GPU wins.
- **No graphics driver:** DirectML falls back to Windows' software
  renderer, about 1 s a dictation (2 s while the PC was busy), against
  0.4 s on the processor directly.

## AI cleanup

Milliseconds a dictation's cleanup takes (the first after the model loaded
and warmed up, then the median of the rest; four short dictations, twice).
*Load* is the server starting and answering.

| Model | Where | Load | First | Median |
|---|---|---:|---:|---:|
| Qwen2.5 1.5B | Graphics card | 1812 | 48 | 80 |
| Qwen3.5 4B | Graphics card | 3132 | 121 | 159 |
| Qwen2.5 1.5B | Processor, 16 threads | 774 | 501 | 928 |
| Qwen2.5 1.5B | **4 threads (emulated)** | 776 | 606 | 1082 |
| Qwen2.5 1.5B | **2 threads (emulated)** | 770 | 813 | 1606 |
| Qwen3.5 4B * | Processor, 16 threads | 1298 | 1419 | 2483 |
| Qwen3.5 4B | **4 threads (emulated)** | 1312 | 1669 | 2840 |
| Qwen3.5 4B | **2 threads (emulated)** | 1564 | 2273 | 3864 |
| openai/gpt-oss-20b | Cloud (Groq) | — | 474 | 849 |

(A first full run, with other builds starting on the PC mid-way, measured
the 1.5B at 1486 ms on 4 threads and 1679 ms on 2: busy PCs make the
processor rows slower, and a laptop's own background work will too.)

What the table says: on a graphics card the on-device model is as quick as
the cloud or quicker. On the processor the 1.5B adds about a second, and
more on fewer threads; the 4B adds three to four seconds and wants a card
(Yap only recommends it with one).

The cloud row is Groq's free tier, three cleanups (474, 849 and 768 ms);
it doesn't depend on the PC, only on the connection and the provider.

## Starting Yap

- **Cold start to the first-run window on screen** (a copy of Yap with an
  empty data folder): **0.7–1.2 s** over four launches, its page loaded at
  0.50–0.60 s. The first launch on a PC builds Yap's WebView2 profile; it
  measured the same as later ones here.
- **Home's first paint** (the main window reloaded, a debug build): Home
  had its data **73 ms** after the page started, down from 132 ms, by
  asking for it in one call before anything else (`home_snapshot`).

## The first run, step by step

A fresh portable copy with an empty data folder, the first-run window
walked to the first dictation (a release-optimised build in test mode,
2026-10-24), on a fast connection (the model came down at ~45 MB/s):

| Step | Time | What happens |
|---|---|---|
| Launch to the first-run window | ~0.7–1.2 s | See above. |
| 1. Speech model: "Download Parakeet V3 & continue" | **~15–18 s** | 478 MB download and its SHA-256 check (~12 s here; about 80 s on a 50 Mbit/s line), unpacking (2.9 s), loading (3.2 s on the graphics card, 0.9 s of it planning the encoder; ~0.8 s on the processor). |
| 2. Microphone | none | The level meter. |
| 3. AI cleanup, private (this PC: Qwen3.5 4B) | ~13 s from a local copy | 368 MB runtime + 2.9 GB model (about 75 s at that speed, 9 minutes at 50 Mbit/s), the server starting on the graphics card (5.1 s) and warming up (0.65 s). On a small PC Yap offers a cloud provider first: no download. |
| 4. Tray | none | |
| 5. The first dictation (11 s of speech) | **0.43 s** | From the end of the dictation to its text, cleanup included (0.37 s for the second). On an emulated small PC with no cleanup and speech on the processor: 0.66 s, then 0.30 s. |

What we fixed on the way:

- The speech model **loaded twice** after its download, the second time on
  the window's thread, so the window froze for ~3 s just before step 2.
  It loads once now.
- The button said "Downloading Parakeet V3… 100%" for the ~6 s of checking,
  unpacking and loading; it says "Setting up Parakeet V3…" now.
- A small PC was offered a 1.5 GB private model that adds ~1.6 s or more to every
  dictation, with no time said. It's offered a cloud provider first now,
  with both times, and the private model one click away.
- On a PC without a graphics card, or with only integrated graphics, the
  first minute's dictations ran on DirectML (software rendering or the
  integrated GPU: 1–4 s each) until Yap's own device check moved them to
  the processor. They start on the processor now (0.4 s).

## What these numbers changed

- **Speech starts on the processor without a real graphics card**
  (`hardware.rs`). Before Yap had timed a PC, it put the speech model on
  the graphics card, and DirectML on a PC with no driver or only integrated
  graphics took 1–4 s a dictation for the first minute or more. A graphics
  card with at least 3 GB of its own still starts on the card.
- **Hybrid laptops use their graphics card** (`gpu.rs`). DirectML's default
  device is the first adapter Windows lists, which on a laptop with both is
  usually the integrated GPU. Yap now points DirectML at the adapter with
  the most memory of its own. We couldn't measure this one: there's no
  hybrid laptop here.
- **Cleanup advice by hardware**, shown in the first-run window with both
  times: the on-device model on a graphics card (the 4B where it fits:
  0.16 s), the 1.5B on a processor with 8 or more threads (about 1.1 s),
  and a cloud provider first on a smaller PC (about 0.8 s, against 1.6 s
  or more for the 1.5B).
- **Home's data in one call**, asked for first: Home shows 132 → 73 ms
  after its page starts.

## How the emulation works, and where it falls short

`yap.exe bench` runs each test in **scenarios**:

| Scenario | What it is |
|---|---|
| `gpu` | The graphics card Yap uses (the one with the most memory of its own). |
| `cpu` | The processor with all its threads: what Yap uses when the processor is faster, or when you pick it in Settings → Speech-to-Text → Runs on. |
| `cpu-4t`, `cpu-2t` | **Emulated.** The processor held to 4 or 2 threads, one per physical core: Windows' process affinity, and every runtime (ONNX Runtime, whisper.cpp, llamafile) told the same number of threads. |
| `igpu-4t` | **Emulated.** The integrated GPU inside the processor (DirectML on that adapter) with 4 processor threads. |
| `warp-4t` | **Emulated.** A PC without a graphics driver: DirectML on Windows' software renderer (WARP), 4 threads. Only with `--scenarios …,warp-4t`. |

Where it falls short:

- **Faster cores.** The test PC's cores run at about 5 GHz with a very large
  cache (96 MB L3) and fast memory. A thin laptop's cores at 15–28 W run
  slower, and slow down further when they get hot. Held to 4 threads, this
  desktop is closer to a **good** 4-core laptop than to a budget one.
- **A weak integrated GPU.** This processor's own Radeon graphics has 2
  compute units: a display adapter, not a laptop's gaming-class integrated
  GPU. Intel's Iris Xe and AMD's Radeon 780M are several times bigger. So
  the `igpu-4t` row shows the **weakest** case.
- **Memory.** The PC has 64 GB, so nothing here was short of memory. On an
  8 GB laptop, Windows and a browser leave little room for the 1.5 GB
  on-device AI model.
- **Busy PCs.** Before each run the bench waits for other apps to leave the
  processor under 30 % busy, and records the load just before and after. A
  run that went ahead while the PC was busy has a `*` in the table and
  `loadBefore`/`loadAfter` in the JSON.

## Run it yourself

1. Install Yap and let it download a speech model (the first-run setup
   does this). For the AI cleanup rows, turn on private AI cleanup once so
   the on-device model is installed. For the cloud row, save a cloud
   provider's key in Settings → Language Models.
2. Close what you can: other apps make the numbers slower.
3. Open PowerShell in Yap's folder (by default `%LOCALAPPDATA%\Yap`, or
   where you installed it; a portable Yap reads its own `Data` folder), then run:

   ```powershell
   .\yap.exe bench | Out-Host
   ```

   (`| Out-Host` makes PowerShell wait for it. In Command Prompt:
   `yap.exe bench | more`.)

It takes 10 to 30 minutes. It reads your models and settings and **changes
nothing**: anything it writes goes to a temporary folder that it deletes at
the end. Near the end a throwaway copy of Yap opens its first-run window
for a few seconds to time the cold start. That copy has its own empty data
folder, so your settings are safe. Skip it with `--no-cold-start`.

You get a table on screen and two files in the current folder:
`yap-bench-<date>.txt` (the table) and `yap-bench-<date>.json` (everything).

Useful options (`yap.exe bench --help` lists them all):

| Option | What it does |
|---|---|
| `--quick` | 1 round, 3 and 11 s dictations, graphics card and processor only (a few minutes). |
| `--scenarios gpu,cpu,cpu-4t,cpu-2t,igpu-4t,warp-4t` | Choose the scenarios. |
| `--models parakeet-tdt-0.6b-v3,…` | Choose the speech models (`none` to skip speech-to-text). |
| `--llm <file.gguf>,…` | The on-device models for cleanup (`none` to skip them). |
| `--no-cleanup`, `--no-cloud`, `--no-cold-start` | Skip those parts. |
| `--rounds 5` | More warm runs per length (default 3). |
| `--patience 0` | Don't wait for a quiet processor. |

## Share your numbers

Open an issue at <https://github.com/nayballs/Yap/issues> titled
"Benchmark: *your processor* / *your graphics*" and attach the `.json` file.
Tell us whether it's a laptop, and if it was plugged in.

**What the JSON contains:** Yap's version; your processor's name, cores,
threads and memory (in whole GB); your graphics adapters' names and their
memory; and the timings. Nothing else about you or your PC: no user name, no
file paths, no serial numbers, no driver versions, none of your dictations,
and never an API key (the cloud row names the provider and model only). The
transcripts it times come from a clip in Yap of a voice reading fictional
sentences.

## How each number is measured

- **Speech to text** (`speech` in the JSON): each installed speech model in
  each scenario. *Load* is the model loading. *First* is the first
  dictation after loading, about 11 s long, when DirectML and Vulkan build
  their graphs. Then 3 warm rounds of dictations of about 3, 11 and 30 s
  (the clip Yap uses to choose a model's device, `stt_speed.rs`, cut at
  pauses). Every run is a slightly different length, as real dictations
  are. The table shows each length's **median**, the time from the end of
  your dictation to its text. **WER** is the share of words wrong against
  the script the clip reads.
- **AI cleanup** (`cleanup`): the on-device models on Yap's own llamafile
  server, started the way Yap starts it (the graphics card or `-ngl 0` for
  the processor, the scenario's threads). Yap warms it up the way it does
  after a start, then cleans four short dictations twice, through the
  same code a dictation uses. *First* is the first cleanup; *median* is the
  median of the rest. For the cloud, it runs three cleanups with the
  provider and key in your settings.
- **Cold start** (`coldStart`): a copy of Yap with an empty data folder, as
  on a new install. *First launch* builds its WebView2 profile, as a new
  install's first launch does; *second launch* finds it. Each is timed to
  the first-run window on screen with its page loaded.
