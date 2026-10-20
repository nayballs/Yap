// Stand-in GGUF model files for the e2e suite: a real GGUF header (version
// 3, the metadata a model's context and memory are read from, a tensor
// list) and no weights, so src-tauri/src/gguf.rs reads them exactly as it
// reads a real model — its trained context ("up to 256K"), its shape (the
// memory estimate per context) — while the file stays a few MB. Nothing
// ever loads them: the llamafile engine is an empty placeholder.
import fs from 'node:fs';

const u32 = (n) => {
  const b = Buffer.alloc(4);
  b.writeUInt32LE(n);
  return b;
};
const u64 = (n) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
};
const str = (s) => Buffer.concat([u64(Buffer.byteLength(s)), Buffer.from(s, 'utf8')]);

/**
 * A GGUF header: `meta` = [[key, value]] (numbers as u32, strings, or
 * `{ tokens: n }` for a tokenizer list of n entries); `tensors` =
 * [[name, bytes]] (offsets only: no data is written).
 */
export function ggufHeader(meta, tensors = []) {
  const parts = [Buffer.from('GGUF'), u32(3), u64(tensors.length), u64(meta.length)];
  for (const [key, value] of meta) {
    parts.push(str(key));
    if (typeof value === 'number') parts.push(u32(4), u32(value));
    else if (typeof value === 'string') parts.push(u32(8), str(value));
    else if (value && typeof value.tokens === 'number') {
      parts.push(u32(9), u32(8), u64(value.tokens));
      for (let i = 0; i < value.tokens; i += 1) parts.push(str(`t${i}`));
    }
  }
  let offset = 0;
  for (const [name, bytes] of tensors) {
    parts.push(str(name), u32(1), u64(1), u32(0), u64(offset));
    offset += bytes;
  }
  const head = Buffer.concat(parts);
  // Aligned to 32, then a little "data".
  return Buffer.concat([head, Buffer.alloc(32 - (head.length % 32) + 64)]);
}

/** Qwen3.5-4B's shape (bartowski's Q4_K_M): trained for 256K, hybrid. */
export function qwen35_4b() {
  return ggufHeader(
    [
      ['general.architecture', 'qwen35'],
      ['general.name', 'Qwen3.5 4B'],
      ['qwen35.block_count', 33],
      ['qwen35.context_length', 262144],
      ['qwen35.embedding_length', 2560],
      ['qwen35.attention.head_count', 16],
      ['qwen35.attention.head_count_kv', 4],
      ['qwen35.attention.key_length', 256],
      ['qwen35.attention.value_length', 256],
      ['qwen35.ssm.conv_kernel', 4],
      ['qwen35.ssm.state_size', 128],
      ['qwen35.ssm.group_count', 16],
      ['qwen35.ssm.inner_size', 4096],
      ['qwen35.full_attention_interval', 4],
      ['qwen35.nextn_predict_layers', 1],
      ['tokenizer.ggml.tokens', { tokens: 248320 }],
    ],
    [['token_embd.weight', 521472000], ['blk.0.attn_norm.weight', 10240]]
  );
}

/** Qwen2.5-1.5B-Instruct's shape (Qwen's own Q4_K_M): trained for 32K. */
export function qwen25_1_5b() {
  return ggufHeader(
    [
      ['general.architecture', 'qwen2'],
      ['qwen2.block_count', 28],
      ['qwen2.context_length', 32768],
      ['qwen2.embedding_length', 1536],
      ['qwen2.attention.head_count', 12],
      ['qwen2.attention.head_count_kv', 2],
      ['tokenizer.ggml.tokens', { tokens: 151936 }],
    ],
    [['token_embd.weight', 131272704], ['output.weight', 191000000]]
  );
}

/** Write a stand-in model to `file`. */
export function writeModel(file, header) {
  fs.writeFileSync(file, header);
}
