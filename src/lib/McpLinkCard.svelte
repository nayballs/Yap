<script>
  // Integrations → the way to Settings → MCP (McpSection.svelte), like the
  // "Give your AI access… Go to MCP" row on Wispr Flow's Connectors page.
  import claudeIcon from '../assets/providers/claude.svg';
  import openaiIcon from '../assets/providers/openai.svg';
  import geminiIcon from '../assets/providers/gemini.svg';

  let { onopen } = $props();
</script>

<section class="card" aria-labelledby="mcp-link-title">
  <span class="logos" aria-hidden="true">
    <span class="logo"><img src={claudeIcon} alt="" /></span>
    <span class="logo"><img src={openaiIcon} alt="" /></span>
    <span class="logo"><img src={geminiIcon} alt="" /></span>
  </span>
  <div class="text">
    <h2 id="mcp-link-title">AI apps (MCP)</h2>
    <p>
      Give Claude, ChatGPT, Gemini and Cursor access to your meeting transcripts and notes, on this
      PC. They can't see your dictations.
    </p>
  </div>
  <button class="go" onclick={() => onopen?.()}>Go to MCP</button>
</section>

<style>
  .card {
    display: flex;
    align-items: center;
    gap: 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 14px 16px;
    margin-bottom: 14px;
  }
  .logos {
    display: flex;
    flex: 0 0 auto;
  }
  .logo {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .logo + .logo {
    margin-left: -7px;
  }
  .logo img {
    width: 13px;
    height: 13px;
  }
  .text {
    flex: 1 1 auto;
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
  }
  p {
    margin: 3px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
  .go {
    flex: 0 0 auto;
    height: 32px;
    padding: 0 14px;
    font: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--yap-fg);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    cursor: pointer;
  }
  .go:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s1);
  }
</style>
