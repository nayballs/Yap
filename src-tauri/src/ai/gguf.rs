//! **A GGUF model's header**, read-only: what the on-device model picker
//! needs to say how much text a model can read at once and what a longer
//! context costs (`local_llm::context::context_view`).
//!
//! A GGUF file starts with `GGUF`, a version (2 or 3), the tensor count and
//! the metadata count (u64 each), then the metadata — key (u64 length +
//! UTF-8), value type (u32), value — and the tensors' names, sizes and
//! offsets; the tensors' data follows. Only the header is read: a few
//! scalars kept, everything else skipped by seeking (the tokenizer's
//! 150–250 k strings included: ~10 MB of a 3 GB file, a few ms), never the
//! data. Nothing is written, nothing is run. A file that isn't a GGUF, or
//! claims absurd sizes, is an error, never a hang or a huge allocation.
//!
//! What's kept ([`Shape`]): `general.architecture`, and under that prefix
//! `context_length` (what the model was trained for, llama.cpp's
//! `n_ctx_train`), `block_count`, `embedding_length`,
//! `attention.head_count`, `attention.head_count_kv` (one number, or one
//! per layer: 0 = a layer without attention), `attention.key_length` /
//! `value_length`, `full_attention_interval` (hybrid models: Qwen3.5 has
//! attention in every 4th layer, the rest recurrent), `nextn_predict_layers`
//! (a multi-token-prediction layer llama.cpp doesn't give a cache), the
//! `ssm.*` sizes (the recurrent layers' state), the vocabulary's size
//! (`tokenizer.ggml.tokens`' length: the logits buffer); and from the
//! tensor list where the data starts, the token embedding's size and
//! whether there's an `output.weight` of its own.
//!
//! **The memory estimate** ([`estimate`]), calibrated against llamafile's
//! own log (`… CUDA0 model / KV / RS / compute buffer size = …`) on an RTX
//! 5070 Ti (docs/model-evals.md "Context length" and "llamafile 0.10.6"),
//! for the runtime the sidecar runs ([`Server`]):
//! - the model on the card: the tensors' data, less the token embedding
//!   (llama.cpp keeps it in the processor's memory) — unless the output
//!   shares it (no `output.weight`: then a copy goes to the card). From
//!   llamafile 0.10.6 the multi-token-prediction layer (Qwen3.5's last
//!   block) is only loaded when MTP drafting is on (the 4B: 2,741 MiB
//!   without it, 2,863 with);
//! - the KV cache: per attention layer, K + V = `head_count_kv` ×
//!   (`key_length` + `value_length`) × 2 bytes (f16) per token of context;
//!   one cache for the server's 4 slots (llama.cpp's unified KV);
//! - the recurrent state (hybrid / Mamba layers), per slot: conv
//!   (`conv_kernel` − 1) × (`inner` + 2 × `groups` × `state`) + state
//!   `state` × `inner`, f32, whatever the context;
//! - the compute buffer. **llamafile 0.10.3**: the logits of one 512-token
//!   batch (`vocab` × 512 × 4 bytes) + 2 KiB per token of context (the
//!   attention mask: Qwen3.5-4B 495 → 554 MiB on the card from 8K to 32K,
//!   the 1.5B 322 → 355; at 64K the growth went to the host's buffer
//!   instead, so this errs high, ≤ 2%). **0.10.6** no longer reserves a
//!   whole batch's logits: one batch's feed-forward (2 × `feed_forward_length`
//!   × 512 × 4 bytes) and hidden states (`embedding_length` × 512 × 20
//!   bytes) + 12 MiB + 1 KiB per token of context — the 4B 72 / 96 / 128
//!   MiB at 8K / 32K / 64K (estimated 81 / 105 / 137), Qwen3.5-9B 96 / 120,
//!   Qwen3.5-2B 53, Qwen2.5-1.5B 70 / 94 (estimated 70 / 94);
//! - **MTP** (0.10.6, `--spec-type draft-mtp`, [`Server::mtp`]): the
//!   prediction layer's weights, the recurrent state three times more (a
//!   copy per drafted token to roll back), 36 MiB more compute, and the
//!   draft context: one attention layer's KV at the full context and its
//!   own compute buffer (≤ the main one's) — +864 MiB on the 4B at 8K
//!   (estimated 865), +962 at 32K (985), +1,122 at 64K (1,145), +994 on
//!   the 9B at 8K (1,014).
//!
//! On the processor (no card) the model is all of the tensors' data, and
//! the **context checkpoints** count too ([`Shape::checkpoint_bytes`],
//! 2026-10-30): a hybrid model's server keeps up to `--ctx-checkpoints` (4)
//! a slot, each one slot's recurrent state as the header's `ssm.*` sizes
//! give it — the 4B's 50.25 MiB, ×4 ×2 slots: +402 MiB on the processor,
//! where `--checkpoint-min-step 0` fills them (`local_llm::server_args`).
//! The server keeps them in the PC's memory, so a card's estimate leaves
//! them out (they never take the card's room). A sliding-window model's
//! (Gemma's) are its window's cache, not counted on their own.
//! Sliding-window layers (Gemma) are counted at the full context: the
//! estimate errs high there, never low. Not counted: the CUDA context
//! itself (~230–250 MiB more in `nvidia-smi` than the log's buffers), which
//! the pickers' room for the model leaves aside (`local_llm::context::card_room`).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// The server's parallel slots (llama.cpp's `n_parallel` auto = 4): each
/// keeps its own recurrent state.
pub const SLOTS: u64 = 4;
/// The batch llama.cpp reserves its compute buffer for (`n_ubatch`).
const UBATCH: u64 = 512;
/// The compute buffer's growth per token of context (measured, above):
/// llamafile 0.10.3's, and 0.10.6's.
const COMPUTE_PER_TOKEN_OLD: u64 = 2_048;
const COMPUTE_PER_TOKEN: u64 = 1_024;
/// 0.10.6's compute buffer beyond its feed-forward and hidden states.
const COMPUTE_SLACK: u64 = 12 << 20;
/// MTP's extra compute in the main context (the 4B 34.75 MiB, the 9B
/// 16.75, the 2B 20: the most of them, rounded up).
const MTP_COMPUTE: u64 = 36 << 20;
/// Tokens MTP drafts at a time (llama.cpp's default `n_max`): the recurrent
/// state is kept once more for each, to roll a rejected draft back.
const MTP_DRAFT: u64 = 3;

/// The server a memory estimate is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Server {
    /// llamafile 0.10.6 or newer (llama.cpp b10441): a smaller compute
    /// buffer, and the multi-token-prediction layer left off the card
    /// unless MTP is on. Else 0.10.3's buffers.
    pub current: bool,
    /// MTP speculative decoding on (`--spec-type draft-mtp`, current
    /// runtime only).
    pub mtp: bool,
    /// The KV cache quantised to q8_0 (`-ctk q8_0 -ctv q8_0`): 34 bytes per
    /// 32 values instead of f16's 64, but a bigger compute buffer
    /// ([`Q8_COMPUTE_PER_TOKEN`]). Measured on Qwen3.5-4B (0.10.6, MTP on):
    /// KV 1,024 → 544 MiB at 32K, 2,048 → 1,088 at 64K; the log's buffers
    /// 5,024 → 4,623 and 6,240 → 5,487 MiB (docs/model-evals.md "KV cache q8_0").
    pub kv_q8: bool,
    /// Two slots with a cache of the whole context each (`-np 2
    /// --no-kv-unified`, the processor: `local_llm::Plan::per_slot`): the
    /// KV twice, the recurrent state for 2 slots instead of [`SLOTS`].
    pub per_slot: bool,
}

/// The slots a per-slot server keeps ([`Server::per_slot`]).
pub const PER_SLOT_SLOTS: u64 = 2;

/// The context checkpoints a slot keeps at most (the server's
/// `--ctx-checkpoints`, `local_llm::CTX_CHECKPOINTS`).
const CHECKPOINTS: u64 = crate::ai::local_llm::CTX_CHECKPOINTS as u64;

/// What a q8_0 KV cache adds to the compute buffer per token of context
/// (Qwen3.5-4B on 0.10.6: +80 MiB at 32K, +208 at 64K; this errs high).
const Q8_COMPUTE_PER_TOKEN: u64 = 3_600;

/// What the header says about a model's size and shape.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shape {
    pub arch: String,
    /// The context it was trained for (`<arch>.context_length`).
    pub context_length: Option<u64>,
    pub block_count: u64,
    pub embedding_length: u64,
    /// The feed-forward layers' width (`feed_forward_length`; 0 = unknown).
    pub feed_forward_length: u64,
    pub head_count: u64,
    /// One for every layer, or one per layer (0 = no attention there).
    pub head_count_kv: Vec<u64>,
    pub key_length: Option<u64>,
    pub value_length: Option<u64>,
    /// Hybrid models: attention in every n-th layer, recurrent in the rest.
    pub full_attention_interval: Option<u64>,
    pub nextn_layers: u64,
    pub ssm_conv_kernel: u64,
    pub ssm_inner_size: u64,
    pub ssm_state_size: u64,
    pub ssm_group_count: u64,
    pub vocab_size: u64,
    /// Where the tensors' data starts (the header's size, aligned).
    pub data_start: u64,
    /// The token embedding's bytes (`token_embd.weight`), if listed.
    pub embd_bytes: Option<u64>,
    /// An `output.weight` of its own (else the output shares the embedding).
    pub has_output: bool,
    /// The multi-token-prediction layers' bytes (the last
    /// `nextn_predict_layers` blocks' tensors: Qwen3.5-4B's `blk.32.*`).
    pub nextn_bytes: u64,
}

/// MiB a model takes at one context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub model_mib: u64,
    pub kv_mib: u64,
    pub state_mib: u64,
    pub compute_mib: u64,
    /// The context checkpoints every slot keeps at most, in the PC's memory
    /// ([`Shape::checkpoint_bytes`]): counted on the processor only (0 on
    /// the card, whose memory they don't take).
    pub checkpoint_mib: u64,
    /// MTP drafting's share (0 without it): included in the others' sum.
    pub mtp_mib: u64,
    pub total_mib: u64,
}

impl Shape {
    /// The layers llama.cpp gives a cache (the multi-token-prediction
    /// layers left out).
    fn layers(&self) -> u64 {
        if self.nextn_layers < self.block_count {
            self.block_count - self.nextn_layers
        } else {
            self.block_count
        }
    }

    /// The KV heads of layer `i`; 0 = no attention there.
    fn kv_heads(&self, i: u64) -> u64 {
        let heads = match self.head_count_kv.as_slice() {
            [] => self.head_count,
            [one] => *one,
            per_layer => per_layer.get(i as usize).or(per_layer.last()).copied().unwrap_or(0),
        };
        match self.full_attention_interval {
            Some(n) if n > 0 && !(i + 1).is_multiple_of(n) => 0,
            _ => heads,
        }
    }

    fn head_dims(&self) -> (u64, u64) {
        let k = self
            .key_length
            .filter(|&k| k > 0)
            .unwrap_or_else(|| self.embedding_length.checked_div(self.head_count).unwrap_or(0));
        (k, self.value_length.filter(|&v| v > 0).unwrap_or(k))
    }

    /// Bytes of K + V per token of context (f16).
    pub fn kv_bytes_per_token(&self) -> u64 {
        let (k, v) = self.head_dims();
        (0..self.layers()).map(|i| self.kv_heads(i) * (k + v) * 2).sum()
    }

    /// Layers without attention that keep a recurrent state.
    fn recurrent_layers(&self) -> u64 {
        if self.ssm_state_size == 0 || self.ssm_inner_size == 0 {
            return 0;
        }
        (0..self.layers()).filter(|&i| self.kv_heads(i) == 0).count() as u64
    }

    /// The recurrent state of every slot (f32), whatever the context.
    pub fn state_bytes(&self) -> u64 {
        let conv = self.ssm_conv_kernel.saturating_sub(1)
            * (self.ssm_inner_size + 2 * self.ssm_group_count * self.ssm_state_size);
        let state = self.ssm_state_size * self.ssm_inner_size;
        self.recurrent_layers() * (conv + state) * 4 * SLOTS
    }

    /// llama.cpp's compute buffer at `ctx` on `server`'s runtime: 0.10.3's
    /// is one batch's logits and what grows with the context; 0.10.6's one
    /// batch's feed-forward and hidden states (module docs).
    pub fn compute_bytes(&self, ctx: u64, server: Server) -> u64 {
        if !server.current {
            return self.vocab_size * UBATCH * 4 + ctx * COMPUTE_PER_TOKEN_OLD;
        }
        2 * self.feed_forward_length * UBATCH * 4 + self.embedding_length * UBATCH * 20 + COMPUTE_SLACK + ctx * COMPUTE_PER_TOKEN
    }

    /// One **context checkpoint** of a hybrid model: what llama.cpp's server
    /// copies out of a slot to resume its cache from later
    /// (`llama_state_seq_get_size_ext(…, PARTIAL_ONLY)`: the recurrent part
    /// of one sequence) — one slot's recurrent state, from the header's
    /// `ssm.*` sizes as [`Self::state_bytes`] counts it (Qwen3.5-4B: 24
    /// recurrent layers × 2.1 MiB, 50.25 MiB). 0 for a model without
    /// recurrent layers, or whose header doesn't give their sizes.
    pub fn checkpoint_bytes(&self) -> u64 {
        self.state_bytes() / SLOTS
    }

    /// Recurrent layers beside attention (Qwen3.5's Gated DeltaNet, Mamba
    /// hybrids): llama.cpp resumes such a model's cache only from a
    /// checkpoint.
    pub fn is_hybrid(&self) -> bool {
        self.recurrent_layers() > 0
    }

    /// Can llama.cpp draft with this model's own multi-token-prediction
    /// layer (`--spec-type draft-mtp`)? Qwen3.5 (dense and MoE) carries one
    /// in the GGUFs Yap offers; another architecture with such a layer
    /// isn't counted on.
    pub fn has_mtp(&self) -> bool {
        self.nextn_layers > 0 && self.nextn_bytes > 0 && self.arch.starts_with("qwen35")
    }

    /// The model's bytes on the card (`on_card`) or in the processor's
    /// memory, from a file of `file_bytes`, on `server`'s runtime.
    pub fn model_bytes(&self, file_bytes: u64, on_card: bool, server: Server) -> u64 {
        let data = file_bytes.saturating_sub(self.data_start);
        let on = match (on_card, self.embd_bytes) {
            (true, Some(embd)) if self.has_output => data.saturating_sub(embd),
            _ => data,
        };
        // 0.10.6 leaves the prediction layer off the card unless it drafts.
        if on_card && server.current && !server.mtp {
            on.saturating_sub(self.nextn_bytes)
        } else {
            on
        }
    }

    /// What MTP drafting adds (module docs): the prediction layer's
    /// weights, the recurrent state again per drafted token, its compute,
    /// and the draft context — one attention layer's KV at `ctx` and a
    /// compute buffer like the main one's. 0 when it can't draft.
    pub fn mtp_bytes(&self, ctx: u64) -> u64 {
        if !self.has_mtp() {
            return 0;
        }
        let attention_layers = (0..self.layers()).filter(|&i| self.kv_heads(i) > 0).count() as u64;
        let kv_one_layer = self.kv_bytes_per_token().checked_div(attention_layers).unwrap_or(0) * ctx;
        let draft_compute = self.compute_bytes(ctx, Server { current: true, mtp: false, kv_q8: false, per_slot: false });
        self.nextn_bytes + MTP_DRAFT * self.state_bytes() + MTP_COMPUTE + kv_one_layer + draft_compute
    }
}

const MIB: u64 = 1 << 20;

/// What `shape` in a file of `file_bytes` takes at a context of `ctx`
/// tokens, on the graphics card (`on_card`) or the processor, run by
/// `server`. Pure.
pub fn estimate(shape: &Shape, file_bytes: u64, ctx: u64, on_card: bool, server: Server) -> Estimate {
    let mib = |b: u64| b.div_ceil(MIB);
    let mtp_on = server.current && server.mtp && shape.has_mtp();
    let model = mib(shape.model_bytes(file_bytes, on_card, server));
    let caches = if server.per_slot { PER_SLOT_SLOTS } else { 1 };
    let kv_f16 = shape.kv_bytes_per_token() * ctx * caches;
    let kv = mib(if server.kv_q8 { kv_f16 * 17 / 32 } else { kv_f16 });
    let state = mib(if server.per_slot { shape.state_bytes() / SLOTS * PER_SLOT_SLOTS } else { shape.state_bytes() });
    let compute = mib(shape.compute_bytes(ctx, server) + if server.kv_q8 { ctx * Q8_COMPUTE_PER_TOKEN } else { 0 });
    // MTP's share past the weights the model already counts.
    let mtp = if mtp_on { mib(shape.mtp_bytes(ctx).saturating_sub(shape.nextn_bytes)) } else { 0 };
    // The context checkpoints (`--ctx-checkpoints`): up to that many a slot,
    // kept in the PC's memory by the server — on the processor (where
    // `--checkpoint-min-step 0` fills them) beside everything else; with a
    // card, never on it.
    let slots = if server.per_slot { PER_SLOT_SLOTS } else { SLOTS };
    let checkpoints = if on_card { 0 } else { mib(shape.checkpoint_bytes() * CHECKPOINTS * slots) };
    Estimate {
        model_mib: model,
        kv_mib: kv,
        state_mib: state,
        compute_mib: compute,
        checkpoint_mib: checkpoints,
        mtp_mib: mtp,
        total_mib: model + kv + state + compute + checkpoints + mtp,
    }
}

// ------------------------------------------------------------ reading

/// A metadata value, as far as it's kept.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Uint(u64),
    Text(String),
    /// An array of integers (≤ 4,096 kept, e.g. per-layer head counts).
    Ints(Vec<u64>),
    /// Any other array: only its length.
    Len(u64),
    Other,
}

/// What the tensor list says.
#[derive(Debug, Clone, Copy, Default)]
struct Tensors {
    data_start: u64,
    embd_bytes: Option<u64>,
    has_output: bool,
    /// The bytes of the blocks from `nextn_from` on (the prediction layers).
    nextn_bytes: u64,
}

/// The block a tensor belongs to: `blk.32.attn_norm.weight` → 32. Pure.
fn block_of(name: &str) -> Option<u64> {
    name.strip_prefix("blk.")?.split('.').next()?.parse().ok()
}

const MAX_KEYS: u64 = 100_000;
const MAX_TENSORS: u64 = 1_000_000;
const MAX_KEY_LEN: u64 = 1 << 16;
const MAX_TEXT_KEPT: u64 = 1 << 10;
const MAX_INTS_KEPT: u64 = 4_096;

struct Reader<R> {
    r: R,
}

impl<R: BufRead + Seek> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let mut b = [0u8; N];
        self.r.read_exact(&mut b).map_err(|_| "the file ends inside its header".to_string())?;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn skip(&mut self, n: u64) -> Result<(), String> {
        let n = i64::try_from(n).map_err(|_| "a value claims an impossible size".to_string())?;
        self.r.seek_relative(n).map_err(|e| e.to_string())
    }
    /// A string: kept when short enough (`keep` bytes), else skipped.
    fn text(&mut self, keep: u64) -> Result<Option<String>, String> {
        let n = self.u64()?;
        if n <= keep {
            let mut b = vec![0u8; n as usize];
            self.r.read_exact(&mut b).map_err(|_| "the file ends inside its header".to_string())?;
            return Ok(Some(String::from_utf8_lossy(&b).into_owned()));
        }
        self.skip(n)?;
        Ok(None)
    }

    /// One value of type `t`; `depth` guards nested arrays.
    fn value(&mut self, t: u32, depth: u8) -> Result<Val, String> {
        Ok(match t {
            0 | 1 | 7 => Val::Uint(self.bytes::<1>()?[0] as u64),
            2 | 3 => Val::Uint(u16::from_le_bytes(self.bytes()?) as u64),
            4 => Val::Uint(self.u32()? as u64),
            // A negative i32/i64 isn't a size anyone needs: kept as 0.
            5 => Val::Uint(i32::from_le_bytes(self.bytes()?).max(0) as u64),
            6 => {
                self.skip(4)?;
                Val::Other
            }
            8 => self.text(MAX_TEXT_KEPT)?.map(Val::Text).unwrap_or(Val::Other),
            9 => {
                if depth > 2 {
                    return Err("arrays nested too deep".to_string());
                }
                let et = self.u32()?;
                let n = self.u64()?;
                if n > u32::MAX as u64 {
                    return Err("an array claims an impossible length".to_string());
                }
                let fixed = match et {
                    0 | 1 | 7 => Some(1),
                    2 | 3 => Some(2),
                    4..=6 => Some(4),
                    10..=12 => Some(8),
                    _ => None,
                };
                if matches!(et, 0..=5 | 7 | 10 | 11) && n <= MAX_INTS_KEPT {
                    let mut v = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        v.push(match self.value(et, depth + 1)? {
                            Val::Uint(x) => x,
                            _ => 0,
                        });
                    }
                    Val::Ints(v)
                } else if let Some(size) = fixed {
                    self.skip(n * size)?;
                    Val::Len(n)
                } else if et == 8 {
                    for _ in 0..n {
                        self.text(0)?;
                    }
                    Val::Len(n)
                } else {
                    for _ in 0..n {
                        self.value(et, depth + 1)?;
                    }
                    Val::Len(n)
                }
            }
            10 => Val::Uint(self.u64()?),
            11 => Val::Uint(i64::from_le_bytes(self.bytes()?).max(0) as u64),
            12 => {
                self.skip(8)?;
                Val::Other
            }
            _ => return Err(format!("an unknown value type ({t})")),
        })
    }

    /// The tensor list: each name, dimensions, type and offset into the
    /// data; a tensor's size is the gap to the next tensor's offset (the
    /// embedding's, and the prediction blocks' from `nextn_from` on).
    fn tensors(&mut self, count: u64, alignment: u64, nextn_from: Option<u64>) -> Result<Tensors, String> {
        if count > MAX_TENSORS {
            return Err("the header claims an impossible number of tensors".to_string());
        }
        let mut offsets = Vec::with_capacity(count.min(4_096) as usize);
        let mut embd: Option<u64> = None;
        let mut has_output = false;
        let mut nextn: Vec<u64> = Vec::new();
        for _ in 0..count {
            let name = self.text(256)?.unwrap_or_default();
            let dims = self.u32()?;
            if dims > 8 {
                return Err("a tensor claims too many dimensions".to_string());
            }
            self.skip(8 * dims as u64)?;
            let _type = self.u32()?;
            let offset = self.u64()?;
            match name.as_str() {
                "token_embd.weight" => embd = Some(offset),
                "output.weight" => has_output = true,
                _ => {}
            }
            if nextn_from.is_some_and(|from| block_of(&name).is_some_and(|b| b >= from)) {
                nextn.push(offset);
            }
            offsets.push(offset);
        }
        let here = self.r.stream_position().map_err(|e| e.to_string())?;
        let align = alignment.max(1);
        let data_start = here.div_ceil(align) * align;
        let file_len = self.r.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        let mut sorted = offsets.clone();
        sorted.sort_unstable();
        let size_at = |at: u64| {
            let next = sorted.iter().copied().find(|&o| o > at);
            next.unwrap_or_else(|| file_len.saturating_sub(data_start)).saturating_sub(at)
        };
        let embd_bytes = embd.map(size_at);
        let nextn_bytes = nextn.iter().map(|&at| size_at(at)).sum();
        Ok(Tensors { data_start, embd_bytes, has_output, nextn_bytes })
    }
}

/// The metadata and tensor list of a GGUF stream. Pure (over the stream).
fn read_header<R: BufRead + Seek>(r: R) -> Result<(HashMap<String, Val>, Tensors), String> {
    let mut rd = Reader { r };
    if &rd.bytes::<4>()? != b"GGUF" {
        return Err("not a GGUF file".to_string());
    }
    let version = rd.u32()?;
    if !(2..=3).contains(&version) {
        return Err(format!("GGUF version {version} isn't supported"));
    }
    let tensors = rd.u64()?;
    let count = rd.u64()?;
    if count > MAX_KEYS {
        return Err("the header claims an impossible number of entries".to_string());
    }
    let mut meta = HashMap::new();
    for _ in 0..count {
        let n = rd.u64()?;
        if n > MAX_KEY_LEN {
            return Err("a key claims an impossible length".to_string());
        }
        let mut key = vec![0u8; n as usize];
        rd.r.read_exact(&mut key).map_err(|_| "the file ends inside its header".to_string())?;
        let key = String::from_utf8_lossy(&key).into_owned();
        let t = rd.u32()?;
        let v = rd.value(t, 0)?;
        meta.insert(key, v);
    }
    let alignment = match meta.get("general.alignment") {
        Some(Val::Uint(a)) if *a > 0 => *a,
        _ => 32,
    };
    // The prediction layers are the last `nextn_predict_layers` blocks.
    let arch = match meta.get("general.architecture") {
        Some(Val::Text(a)) => a.clone(),
        _ => String::new(),
    };
    let scalar = |k: &str| match meta.get(&format!("{arch}.{k}")) {
        Some(Val::Uint(v)) => Some(*v),
        _ => None,
    };
    let nextn_from = match (scalar("block_count"), scalar("nextn_predict_layers")) {
        (Some(blocks), Some(n)) if n > 0 && n <= blocks => Some(blocks - n),
        _ => None,
    };
    let t = rd.tensors(tensors, alignment, nextn_from)?;
    Ok((meta, t))
}

/// The [`Shape`] in a GGUF stream.
pub fn read_shape<R: BufRead + Seek>(r: R) -> Result<Shape, String> {
    let (meta, tensors) = read_header(r)?;
    let arch = match meta.get("general.architecture") {
        Some(Val::Text(a)) => a.clone(),
        _ => return Err("the header names no architecture".to_string()),
    };
    let uint = |k: &str| match meta.get(&format!("{arch}.{k}")) {
        Some(Val::Uint(v)) => Some(*v),
        Some(Val::Ints(v)) => v.iter().copied().max(),
        _ => None,
    };
    let head_count_kv = match meta.get(&format!("{arch}.attention.head_count_kv")) {
        Some(Val::Uint(v)) => vec![*v],
        Some(Val::Ints(v)) => v.clone(),
        _ => Vec::new(),
    };
    let vocab_size = match meta.get("tokenizer.ggml.tokens") {
        Some(Val::Len(n)) => *n,
        Some(Val::Ints(v)) => v.len() as u64,
        _ => uint("vocab_size").unwrap_or(0),
    };
    Ok(Shape {
        context_length: uint("context_length").filter(|&c| c > 0),
        block_count: uint("block_count").unwrap_or(0),
        embedding_length: uint("embedding_length").unwrap_or(0),
        feed_forward_length: uint("feed_forward_length").unwrap_or(0),
        head_count: uint("attention.head_count").unwrap_or(0),
        head_count_kv,
        key_length: uint("attention.key_length"),
        value_length: uint("attention.value_length"),
        full_attention_interval: uint("full_attention_interval"),
        nextn_layers: uint("nextn_predict_layers").unwrap_or(0),
        ssm_conv_kernel: uint("ssm.conv_kernel").unwrap_or(0),
        ssm_inner_size: uint("ssm.inner_size").unwrap_or(0),
        ssm_state_size: uint("ssm.state_size").unwrap_or(0),
        ssm_group_count: uint("ssm.group_count").unwrap_or(0),
        vocab_size,
        data_start: tensors.data_start,
        embd_bytes: tensors.embd_bytes,
        has_output: tensors.has_output,
        nextn_bytes: tensors.nextn_bytes,
        arch,
    })
}

/// A file's shape and size.
#[derive(Debug, Clone)]
pub struct ModelFile {
    pub shape: Shape,
    pub bytes: u64,
}

type Cache = HashMap<PathBuf, (u64, Option<SystemTime>, Arc<ModelFile>)>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The shape of the GGUF at `path`, read once per file (again when its size
/// or date changes); `None` when it isn't a readable GGUF (a placeholder, a
/// download in progress).
pub fn model_file(path: &Path) -> Option<Arc<ModelFile>> {
    let meta = std::fs::metadata(path).ok()?;
    let (len, modified) = (meta.len(), meta.modified().ok());
    if let Some(hit) = CACHE.lock().ok().and_then(|c| {
        c.as_ref()?
            .get(path)
            .filter(|(l, m, _)| *l == len && *m == modified)
            .map(|(_, _, f)| f.clone())
    }) {
        return Some(hit);
    }
    let file = std::fs::File::open(path).ok()?;
    let shape = match read_shape(BufReader::with_capacity(256 * 1024, file)) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(file = %path.display(), "Not a readable GGUF header: {e}");
            return None;
        }
    };
    let f = Arc::new(ModelFile { shape, bytes: len });
    if let Ok(mut c) = CACHE.lock() {
        c.get_or_insert_with(HashMap::new).insert(path.to_path_buf(), (len, modified, f.clone()));
    }
    Some(f)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Cursor;

    /// A GGUF header written by hand, for the tests (and the e2e suite's
    /// stand-in models do the same in JS).
    #[derive(Default)]
    pub struct Writer {
        kvs: Vec<u8>,
        count: u64,
        tensors: Vec<(String, u64)>,
        data: u64,
    }

    impl Writer {
        fn key(&mut self, k: &str, t: u32) {
            self.kvs.extend((k.len() as u64).to_le_bytes());
            self.kvs.extend(k.as_bytes());
            self.kvs.extend(t.to_le_bytes());
            self.count += 1;
        }
        pub fn text(mut self, k: &str, v: &str) -> Self {
            self.key(k, 8);
            self.kvs.extend((v.len() as u64).to_le_bytes());
            self.kvs.extend(v.as_bytes());
            self
        }
        pub fn u32(mut self, k: &str, v: u32) -> Self {
            self.key(k, 4);
            self.kvs.extend(v.to_le_bytes());
            self
        }
        pub fn f32(mut self, k: &str, v: f32) -> Self {
            self.key(k, 6);
            self.kvs.extend(v.to_le_bytes());
            self
        }
        pub fn i32s(mut self, k: &str, v: &[i32]) -> Self {
            self.key(k, 9);
            self.kvs.extend(5u32.to_le_bytes());
            self.kvs.extend((v.len() as u64).to_le_bytes());
            for x in v {
                self.kvs.extend(x.to_le_bytes());
            }
            self
        }
        pub fn texts(mut self, k: &str, n: usize) -> Self {
            self.key(k, 9);
            self.kvs.extend(8u32.to_le_bytes());
            self.kvs.extend((n as u64).to_le_bytes());
            for i in 0..n {
                let s = format!("t{i}");
                self.kvs.extend((s.len() as u64).to_le_bytes());
                self.kvs.extend(s.as_bytes());
            }
            self
        }
        /// A tensor of `bytes` (its data isn't written: the file's size is
        /// the test's to say).
        pub fn tensor(mut self, name: &str, bytes: u64) -> Self {
            self.tensors.push((name.to_string(), self.data));
            self.data += bytes;
            self
        }
        pub fn bytes(&self) -> Vec<u8> {
            let mut out = b"GGUF".to_vec();
            out.extend(3u32.to_le_bytes());
            out.extend((self.tensors.len() as u64).to_le_bytes());
            out.extend(self.count.to_le_bytes());
            out.extend(&self.kvs);
            for (name, offset) in &self.tensors {
                out.extend((name.len() as u64).to_le_bytes());
                out.extend(name.as_bytes());
                out.extend(2u32.to_le_bytes());
                out.extend(4096u64.to_le_bytes());
                out.extend(256u64.to_le_bytes());
                out.extend(12u32.to_le_bytes());
                out.extend(offset.to_le_bytes());
            }
            // The data would follow; a little of it, the reader never reads it.
            out.resize(out.len().div_ceil(32) * 32 + 64, 0);
            out
        }
    }

    /// Qwen3.5-4B's header (bartowski's Q4_K_M; tied embeddings: no
    /// output.weight), vocabulary as given.
    pub fn qwen35_4b(vocab: usize) -> Writer {
        Writer::default()
            .text("general.architecture", "qwen35")
            .text("general.name", "Qwen3.5 4B")
            .u32("qwen35.block_count", 33)
            .u32("qwen35.context_length", 262_144)
            .u32("qwen35.embedding_length", 2560)
            .u32("qwen35.feed_forward_length", 9216)
            .u32("qwen35.attention.head_count", 16)
            .u32("qwen35.attention.head_count_kv", 4)
            .i32s("qwen35.rope.dimension_sections", &[11, 11, 10, 0])
            .f32("qwen35.rope.freq_base", 10_000_000.0)
            .u32("qwen35.attention.key_length", 256)
            .u32("qwen35.attention.value_length", 256)
            .u32("qwen35.ssm.conv_kernel", 4)
            .u32("qwen35.ssm.state_size", 128)
            .u32("qwen35.ssm.group_count", 16)
            .u32("qwen35.ssm.time_step_rank", 32)
            .u32("qwen35.ssm.inner_size", 4096)
            .u32("qwen35.full_attention_interval", 4)
            .u32("qwen35.nextn_predict_layers", 1)
            .text("tokenizer.ggml.model", "gpt2")
            .texts("tokenizer.ggml.tokens", vocab)
            .text("tokenizer.chat_template", &"{{ x }}".repeat(2_000))
            .tensor("token_embd.weight", 521_472_000)
            .tensor("blk.0.attn_norm.weight", 10_240)
            // The multi-token-prediction block (its real size, all of
            // blk.32's tensors as one here).
            .tensor("blk.32.nextn.eh_proj.weight", 128_176_128)
            .tensor("output_norm.weight", 10_240)
    }

    /// Qwen2.5-1.5B-Instruct's header (Qwen's own Q4_K_M, with an
    /// output.weight of its own).
    pub fn qwen25_1_5b(vocab: usize) -> Writer {
        Writer::default()
            .text("general.architecture", "qwen2")
            .u32("qwen2.block_count", 28)
            .u32("qwen2.context_length", 32_768)
            .u32("qwen2.embedding_length", 1536)
            .u32("qwen2.feed_forward_length", 8960)
            .u32("qwen2.attention.head_count", 12)
            .u32("qwen2.attention.head_count_kv", 2)
            .texts("tokenizer.ggml.tokens", vocab)
            .tensor("token_embd.weight", 131_272_000)
            .tensor("output.weight", 191_000_000)
            .tensor("blk.0.attn_q.weight", 1_000_000)
    }

    #[test]
    fn the_header_gives_the_context_and_the_shape() {
        let s = read_shape(Cursor::new(qwen35_4b(1_000).bytes())).unwrap();
        assert_eq!(s.arch, "qwen35");
        assert_eq!(s.context_length, Some(262_144));
        assert_eq!((s.block_count, s.nextn_layers, s.head_count, s.head_count_kv.clone()), (33, 1, 16, vec![4]));
        assert_eq!(s.vocab_size, 1_000);
        assert_eq!(s.full_attention_interval, Some(4));
        assert_eq!((s.embd_bytes, s.has_output, s.nextn_bytes), (Some(521_472_000), false, 128_176_128));
        assert!(s.data_start > 0 && s.data_start.is_multiple_of(32));
        let small = read_shape(Cursor::new(qwen25_1_5b(500).bytes())).unwrap();
        assert_eq!((small.arch.as_str(), small.context_length, small.vocab_size), ("qwen2", Some(32_768), 500));
        assert_eq!(small.head_dims(), (128, 128), "embedding / heads when the header has no key_length");
        assert_eq!((small.embd_bytes, small.has_output), (Some(131_272_000), true));
    }

    #[test]
    fn what_isnt_a_gguf_is_an_error_not_a_hang() {
        assert!(read_shape(Cursor::new(Vec::<u8>::new())).is_err(), "an empty placeholder");
        assert!(read_shape(Cursor::new(b"PK\x03\x04 not a model".to_vec())).is_err());
        let mut v1 = qwen25_1_5b(10).bytes();
        v1[4] = 1;
        assert!(read_shape(Cursor::new(v1)).is_err(), "version 1 isn't read");
        // Cut short inside the tokens.
        let whole = qwen25_1_5b(1_000).bytes();
        assert!(read_shape(Cursor::new(whole[..6_000].to_vec())).is_err());
        // A count or length no file could hold.
        let header = |kvs: u64| {
            let mut h = b"GGUF".to_vec();
            h.extend(3u32.to_le_bytes());
            h.extend(0u64.to_le_bytes());
            h.extend(kvs.to_le_bytes());
            h
        };
        assert!(read_shape(Cursor::new(header(u64::MAX))).is_err());
        let mut long_key = header(1);
        long_key.extend((1u64 << 40).to_le_bytes());
        assert!(read_shape(Cursor::new(long_key)).is_err());
        let mut many_tensors = b"GGUF".to_vec();
        many_tensors.extend(3u32.to_le_bytes());
        many_tensors.extend(u64::MAX.to_le_bytes());
        many_tensors.extend(0u64.to_le_bytes());
        assert!(read_shape(Cursor::new(many_tensors)).is_err());
        // No architecture.
        let none = Writer::default().u32("x.block_count", 3).bytes();
        assert!(read_shape(Cursor::new(none)).is_err());
    }

    /// llamafile 0.10.3's buffers; 0.10.6's, without and with MTP.
    const OLD: Server = Server { current: false, mtp: false, kv_q8: false, per_slot: false };
    const NEW: Server = Server { current: true, mtp: false, kv_q8: false, per_slot: false };
    const MTP: Server = Server { current: true, mtp: true, kv_q8: false, per_slot: false };

    /// The real files' sizes (the test PC's %APPDATA%\yap\llm, 2026-10-20).
    const QWEN35_4B_FILE: u64 = 3_013_027_808;
    const QWEN25_1_5B_FILE: u64 = 1_117_320_736;

    /// The synthetic header is smaller than the real one (its vocabulary
    /// list is shorter): put the data where the real file has it.
    fn as_real(mut s: Shape, data_start: u64) -> Shape {
        s.data_start = data_start;
        s
    }

    #[test]
    fn the_estimate_matches_what_llamafile_reported() {
        // Qwen3.5-4B on an RTX 5070 Ti, 4 slots (llamafile's log): model
        // 2,862.99 MiB on the card, KV 256 MiB over 8 layers at 8K (1,024 at
        // 32K), recurrent 201 MiB, compute 495 MiB at 8K (554 at 32K);
        // 3,815 MiB in all at 8K, 4,641 at 32K.
        let s = as_real(read_shape(Cursor::new(qwen35_4b(248_320).bytes())).unwrap(), 10_970_112);
        assert_eq!(s.kv_bytes_per_token(), 8 * 4 * 512 * 2, "8 attention layers of 32");
        let e = estimate(&s, QWEN35_4B_FILE, 8_192, true, OLD);
        assert_eq!(e.model_mib, 2_863, "the whole file: the embedding is copied to the card for the output");
        assert_eq!((e.kv_mib, e.state_mib), (256, 201));
        assert!(e.compute_mib.abs_diff(495) <= 8, "{e:?}");
        assert!(e.total_mib.abs_diff(3_815) <= 20, "{e:?}");
        let big = estimate(&s, QWEN35_4B_FILE, 32_768, true, OLD);
        assert_eq!((big.kv_mib, big.state_mib), (1_024, 201));
        assert!(big.compute_mib.abs_diff(554) <= 8, "{big:?}");
        assert!(big.total_mib.abs_diff(4_641) <= 20, "{big:?}");

        // Qwen2.5-1.5B: 934.70 MiB on the card (the 125 MiB embedding
        // stays on the processor: the output has its own), KV 224 MiB at 8K
        // (896 at 32K: 28 KiB a token), compute 322 (355); no recurrent
        // state; 1,480 MiB in all at 8K, 2,186 at 32K.
        let q = as_real(read_shape(Cursor::new(qwen25_1_5b(151_936).bytes())).unwrap(), 5_931_200);
        let e = estimate(&q, QWEN25_1_5B_FILE, 8_192, true, OLD);
        assert!(e.model_mib.abs_diff(935) <= 2, "{e:?}");
        assert_eq!((e.kv_mib, e.state_mib), (224, 0));
        assert!(e.compute_mib.abs_diff(322) <= 10, "{e:?}");
        assert!(e.total_mib.abs_diff(1_480) <= 20, "{e:?}");
        let big = estimate(&q, QWEN25_1_5B_FILE, 32_768, true, OLD);
        assert_eq!(big.kv_mib, 896);
        assert!(big.total_mib.abs_diff(2_186) <= 20, "{big:?}");
        // On the processor all of it is in the PC's memory.
        assert!(estimate(&q, QWEN25_1_5B_FILE, 8_192, false, OLD).model_mib > e.model_mib + 100);
    }

    #[test]
    fn the_estimate_matches_what_llamafile_0_10_6_reported() {
        // llamafile 0.10.6 on the same card (2026-10-22): the 4B's
        // prediction block (blk.32, 122.24 MiB) stays off the card without
        // MTP — 2,740.75 MiB of model —, compute 72 / 96 / 128 MiB at 8K /
        // 32K / 64K: 3,270 / 4,062 / 5,118 MiB in all.
        let s = as_real(read_shape(Cursor::new(qwen35_4b(248_320).bytes())).unwrap(), 10_969_056);
        assert_eq!((s.nextn_bytes, s.feed_forward_length), (128_176_128, 9_216));
        assert!(s.has_mtp());
        for (ctx, log, compute) in [(8_192, 3_270, 72), (32_768, 4_062, 96), (65_536, 5_118, 128)] {
            let e = estimate(&s, QWEN35_4B_FILE, ctx, true, NEW);
            assert_eq!(e.model_mib, 2_741, "{e:?}");
            assert_eq!((e.state_mib, e.mtp_mib), (201, 0));
            // Errs high, by little.
            assert!((compute..=compute + 12).contains(&e.compute_mib), "{ctx}: {e:?}");
            assert!((log..=log + 20).contains(&e.total_mib), "{ctx}: {e:?}");
        }
        // With MTP: the block on the card, the recurrent state x4 (804
        // MiB), 35 MiB more compute, the draft context's KV and compute —
        // 4,134 / 5,024 / 6,240 MiB in the log (+864 / +962 / +1,122).
        for (ctx, log) in [(8_192, 4_134), (32_768, 5_024), (65_536, 6_240)] {
            let e = estimate(&s, QWEN35_4B_FILE, ctx, true, MTP);
            assert_eq!(e.model_mib, 2_863, "{e:?}");
            assert!((log..=log + 50).contains(&e.total_mib), "{ctx}: {e:?}");
        }
        // MTP only on the runtime it's measured on, and only for a model
        // that can draft (the 1.5B can't).
        assert_eq!(
            estimate(&s, QWEN35_4B_FILE, 8_192, true, Server { current: false, mtp: true, kv_q8: false, per_slot: false }),
            estimate(&s, QWEN35_4B_FILE, 8_192, true, OLD)
        );
        // A q8_0 KV cache (2026-10-25, with MTP as the evaluations ran it):
        // KV 544 / 1,088 MiB at 32K / 64K, compute +80 / +208 MiB; the log's
        // buffers 4,623 / 5,487 MiB (f16: 5,024 / 6,240).
        for (ctx, kv, f16_log, q8_log) in [(32_768, 544, 5_024, 4_623), (65_536, 1_088, 6_240, 5_487)] {
            let q8 = estimate(&s, QWEN35_4B_FILE, ctx, true, Server { kv_q8: true, ..MTP });
            assert_eq!(q8.kv_mib, kv, "{ctx}: {q8:?}");
            assert!((q8_log..=q8_log + 100).contains(&q8.total_mib), "{ctx}: {q8:?}");
            let f16 = estimate(&s, QWEN35_4B_FILE, ctx, true, MTP);
            assert!(f16.total_mib - q8.total_mib >= (f16_log - q8_log) * 7 / 10, "{ctx}: saves about what the log says");
        }

        // Qwen2.5-1.5B: compute 69.5 / 93.5 MiB at 8K / 32K: 1,228 / 1,920
        // MiB in all; no MTP.
        let q = as_real(read_shape(Cursor::new(qwen25_1_5b(151_936).bytes())).unwrap(), 5_931_200);
        assert!(!q.has_mtp());
        for (ctx, log) in [(8_192, 1_228), (32_768, 1_920)] {
            let e = estimate(&q, QWEN25_1_5B_FILE, ctx, true, NEW);
            assert!((log..=log + 10).contains(&e.total_mib), "{ctx}: {e:?}");
            assert_eq!(estimate(&q, QWEN25_1_5B_FILE, ctx, true, MTP), e);
        }
    }

    /// The processor's server keeps up to 4 context checkpoints a slot
    /// (`--checkpoint-min-step 0` fills them, 2026-10-29), each one slot's
    /// recurrent state: until 2026-10-30 the estimate left them out.
    #[test]
    fn the_processors_estimate_counts_the_context_checkpoints() {
        let s = as_real(read_shape(Cursor::new(qwen35_4b(248_320).bytes())).unwrap(), 10_969_056);
        // One slot's recurrent state, from the header: 24 recurrent layers ×
        // (3 × 8,192 conv + 128 × 4,096 state) f32 = 50.25 MiB.
        assert_eq!(s.checkpoint_bytes(), 24 * (3 * 8_192 + 128 * 4_096) * 4);
        assert_eq!(s.checkpoint_bytes(), s.state_bytes() / SLOTS);
        let per_slot = Server { current: true, mtp: false, kv_q8: false, per_slot: true };
        let cpu = estimate(&s, QWEN35_4B_FILE, 8_192, false, per_slot);
        // 4 a slot, 2 slots.
        assert_eq!(cpu.checkpoint_mib, 402, "{cpu:?}");
        assert_eq!(cpu.total_mib, cpu.model_mib + cpu.kv_mib + cpu.state_mib + cpu.compute_mib + 402);
        // The older runtime's 4 shared slots: 4 a slot too.
        assert_eq!(estimate(&s, QWEN35_4B_FILE, 8_192, false, OLD).checkpoint_mib, 804);
        // On the card they're in the PC's memory, never the card's.
        let card = estimate(&s, QWEN35_4B_FILE, 8_192, true, NEW);
        assert_eq!(card.checkpoint_mib, 0);
        // A model without recurrent layers keeps none.
        let q = as_real(read_shape(Cursor::new(qwen25_1_5b(151_936).bytes())).unwrap(), 5_931_200);
        assert_eq!(q.checkpoint_bytes(), 0);
        assert_eq!(estimate(&q, QWEN25_1_5B_FILE, 8_192, false, per_slot).checkpoint_mib, 0);
    }

    #[test]
    fn per_layer_head_counts_mark_the_layers_with_attention() {
        // LFM2-style: a head_count_kv array with 0 where a layer has none.
        let s = Shape {
            arch: "lfm2".into(),
            block_count: 4,
            embedding_length: 1024,
            head_count: 16,
            head_count_kv: vec![0, 8, 0, 8],
            ..Default::default()
        };
        assert_eq!(s.kv_bytes_per_token(), 2 * 8 * (64 + 64) * 2);
        assert_eq!(s.state_bytes(), 0, "no ssm sizes, no state");
        // No tensor list: the model is the whole file.
        assert_eq!(s.model_bytes(1_000, true, OLD), 1_000);
    }

    /// This PC's on-device models, read-only (`cargo test gguf -- --ignored
    /// --nocapture`; `YAP_GGUF_DIR` for another folder): each header and
    /// what it would take at each context, on llamafile 0.10.3 and 0.10.6
    /// (with MTP where it can draft).
    #[test]
    #[ignore]
    fn this_pcs_models() {
        let dir = std::env::var_os("YAP_GGUF_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| dirs::data_dir().unwrap().join("yap").join("llm"));
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gguf")) {
                let started = std::time::Instant::now();
                let Some(f) = model_file(&p) else { continue };
                println!("{} ({} ms): {:?}", p.display(), started.elapsed().as_millis(), f.shape);
                for ctx in [4_096u64, 8_192, 16_384, 32_768, 65_536, 131_072, 262_144] {
                    let old = estimate(&f.shape, f.bytes, ctx, true, Server::default());
                    let new = estimate(&f.shape, f.bytes, ctx, true, Server { current: true, mtp: false, kv_q8: false, per_slot: false });
                    let mtp = estimate(&f.shape, f.bytes, ctx, true, Server { current: true, mtp: true, kv_q8: false, per_slot: false });
                    println!("  {ctx}: 0.10.3 {} · 0.10.6 {} · +MTP {} ({:?})", old.total_mib, new.total_mib, mtp.total_mib, mtp);
                }
            }
        }
    }

    #[test]
    fn a_files_shape_is_read_once_and_again_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("yap-gguf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("m.gguf");
        std::fs::write(&p, qwen25_1_5b(10).bytes()).unwrap();
        let a = model_file(&p).unwrap();
        assert_eq!(a.shape.context_length, Some(32_768));
        assert!(Arc::ptr_eq(&a, &model_file(&p).unwrap()), "cached");
        std::fs::write(&p, qwen35_4b(10).bytes()).unwrap();
        assert_eq!(model_file(&p).unwrap().shape.context_length, Some(262_144), "a new file, read again");
        std::fs::write(&p, b"").unwrap();
        assert!(model_file(&p).is_none(), "a placeholder");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
